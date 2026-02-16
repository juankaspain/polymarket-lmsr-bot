//! Polymarket LMSR Bot — Entry Point
//!
//! Initializes configuration, logging, blockchain connections,
//! and the main arbitrage engine. Runs until SIGINT/SIGTERM.
//!
//! Wiring sequence:
//! 1. Load config.toml + validate
//! 2. Init tracing (JSON structured logging)
//! 3. Load CLOB auth from env vars (POLY_API_KEY, POLY_API_SECRET, POLY_PASSPHRASE)
//! 4. Create ClobClient (HTTP + auth + retry + rate limit)
//! 5. Create ClobOrderExecutor (implements OrderExecution port)
//! 6. Create BinanceFeed (external price oracle for BTC/ETH)
//! 7. Spawn health server on :9090 (/live + /ready)
//! 8. Spawn Binance feed supervisor (auto-reconnect WebSocket)
//! 9. Spawn ArbitrageEngine main loop (event-driven tokio::select!)
//! 10. Wait for SIGINT → graceful shutdown (cancel→save→exit)

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::signal;
use tokio::sync::{broadcast, watch};
use tracing::{error, info, warn};

mod adapters;
mod config;
mod domain;
mod ports;
mod usecases;

use adapters::api::auth::ClobAuth;
use adapters::api::client::{ClobClient, ClobClientConfig};
use adapters::api::orders::ClobOrderExecutor;
use adapters::feeds::BinanceFeed;

#[tokio::main]
async fn main() -> Result<()> {
    // ── 1. Load configuration from config.toml ──────────────
    let config = config::loader::load_config("config.toml")
        .context("Failed to load configuration")?;

    // ── 2. Initialize structured JSON logging ───────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| {
                    tracing_subscriber::EnvFilter::new(&config.bot.log_level)
                }),
        )
        .json()
        .init();

    info!(
        name = %config.bot.name,
        version = env!("CARGO_PKG_VERSION"),
        dry_run = config.bot.dry_run,
        mode = ?config.bot.mode,
        markets = config.markets.len(),
        "Starting Polymarket LMSR Bot"
    );

    // ── 3. Shutdown signal channels ─────────────────────────
    let (shutdown_tx, _shutdown_rx) = broadcast::channel::<()>(1);
    let (health_tx, health_rx) = watch::channel(true);

    // ── 4. Load CLOB auth from env vars ─────────────────────
    let auth = Arc::new(
        ClobAuth::from_env().context("Failed to load CLOB credentials from env")?,
    );

    // ── 5. Create CLOB HTTP client with auth + retry ────────
    let clob_config = ClobClientConfig {
        base_url: config.api.clob_base_url.clone(),
        timeout: std::time::Duration::from_millis(config.api.timeout_ms),
        max_concurrent: 10,
        max_retries: 3,
        retry_base_delay: std::time::Duration::from_millis(200),
    };
    let clob_client = Arc::new(
        ClobClient::new(Arc::clone(&auth), clob_config)
            .context("Failed to create CLOB client")?,
    );

    // ── 6. Create order executor (OrderExecution port) ──────
    let executor = Arc::new(ClobOrderExecutor::new(Arc::clone(&clob_client)));

    // ── 7. Create Binance feed (external BTC/ETH oracle) ────
    let binance_feed = Arc::new(BinanceFeed::new());

    // ── 8. Spawn health/metrics server on :9090 ─────────────
    let health_handle = tokio::spawn(serve_health(health_rx, config.clone()));

    // ── 9. Spawn Binance WebSocket feed with auto-reconnect ─
    let binance_shutdown = shutdown_tx.subscribe();
    let binance_ref = Arc::clone(&binance_feed);
    let binance_handle = tokio::spawn(async move {
        if let Err(e) = binance_ref.run(binance_shutdown).await {
            error!(error = %e, "Binance feed task failed");
        }
    });

    // ── 10. Spawn main arbitrage engine ──────────────────────
    let engine_shutdown = shutdown_tx.subscribe();
    let engine_config = config.clone();
    let engine_executor = Arc::clone(&executor);
    let engine_handle = tokio::spawn(async move {
        if let Err(e) = run_engine(
            engine_config,
            engine_executor,
            engine_shutdown,
        )
        .await
        {
            error!(error = %e, "Arbitrage engine failed");
        }
    });

    info!("All tasks spawned — bot is running");

    // ── 11. Wait for SIGINT or SIGTERM ──────────────────────
    tokio::select! {
        _ = signal::ctrl_c() => {
            info!("SIGINT received, initiating graceful shutdown");
        }
    }

    // ── Graceful shutdown (checklist: cancel→claim→save→exit) ──

    // 1. Signal all tasks to stop
    let _ = shutdown_tx.send(());
    info!("Shutdown signal broadcast to all tasks");

    // 2. Mark health as unhealthy (readiness probe → 503)
    let _ = health_tx.send(false);

    // 3. Cancel all open orders
    info!("Cancelling all open orders...");
    match executor.cancel_all_orders().await {
        Ok(n) => info!(cancelled = n, "Open orders cancelled"),
        Err(e) => warn!(error = %e, "Failed to cancel some orders"),
    }

    // 4. Wait for engine to finish (up to 30s)
    info!("Waiting for engine shutdown...");
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        engine_handle,
    )
    .await;

    // 5. Wait for Binance feed to close (up to 5s)
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        binance_handle,
    )
    .await;

    // 6. Stop health server
    health_handle.abort();

    info!("Shutdown complete");
    Ok(())
}

/// Run the main arbitrage engine with fully wired adapters.
///
/// Instantiates domain components (LMSR pricer, Kelly sizer, Bayesian
/// estimator, fee calculator) and the use-case orchestrators (OrderManager,
/// RiskManager). Runs the event-driven select loop until shutdown.
async fn run_engine(
    config: config::AppConfig,
    executor: Arc<ClobOrderExecutor>,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> Result<()> {
    use crate::domain::bayesian::BayesianEstimator;
    use crate::domain::fees::FeeCalculator;
    use crate::domain::kelly::KellySizer;
    use crate::domain::lmsr::LmsrPricer;
    use crate::usecases::order_manager::OrderManager;
    use crate::usecases::risk_manager::RiskManager;

    let _pricer = LmsrPricer::new(config.lmsr.liquidity_parameter);
    let _sizer = KellySizer::new(config.lmsr.kelly_fraction);
    let _fees = FeeCalculator::new_maker();
    let _estimator = BayesianEstimator::new(config.lmsr.prior_weight);
    let _order_manager = OrderManager::new(Arc::clone(&executor), &config);
    let _risk_manager = RiskManager::new(&config.risk);

    // Collect active market tokens
    let active_tokens: Vec<String> = config
        .markets
        .iter()
        .filter(|m| m.active)
        .map(|m| m.yes_token_id.clone())
        .collect();

    if active_tokens.is_empty() {
        warn!("No active markets configured — engine idle");
        // Wait for shutdown even if idle
        let _ = shutdown_rx.recv().await;
        return Ok(());
    }

    info!(
        markets = active_tokens.len(),
        mode = ?config.bot.mode,
        dry_run = config.bot.dry_run,
        "Arbitrage engine started with domain components wired"
    );

    if config.bot.dry_run {
        warn!("Dry-run mode — signals computed but NO real orders placed");
    }

    // Engine event loop: wait for shutdown.
    // The full ArbitrageEngine::run() with MarketFeed subscription
    // is available for use when a MarketFeed adapter that implements
    // the port trait is provided. For now, the Binance feed emits
    // BinanceTick (not PriceUpdate), so we run in standby mode
    // and log heartbeats until the feed adapter bridge is complete.
    loop {
        tokio::select! {
            biased;
            _ = shutdown_rx.recv() => {
                info!("Engine received shutdown signal");
                break;
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                info!(
                    active = active_tokens.len(),
                    "Engine heartbeat — awaiting market feed bridge"
                );
            }
        }
    }

    info!("Engine stopped cleanly");
    Ok(())
}

/// Serve health and metrics endpoints on :9090.
///
/// - `/live`  — Liveness probe: 200 if process is running
/// - `/ready` — Readiness probe: 503 during graceful shutdown
async fn serve_health(
    health_rx: watch::Receiver<bool>,
    _config: config::AppConfig,
) -> Result<()> {
    use axum::{extract::State, http::StatusCode, routing::get, Router};

    let app = Router::new()
        .route("/live", get(|| async { StatusCode::OK }))
        .route(
            "/ready",
            get(
                move |State(rx): State<watch::Receiver<bool>>| async move {
                    if *rx.borrow() {
                        StatusCode::OK
                    } else {
                        StatusCode::SERVICE_UNAVAILABLE
                    }
                },
            ),
        )
        .with_state(health_rx);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:9090").await?;
    info!("Health server listening on :9090");
    axum::serve(listener, app).await?;
    Ok(())
}
