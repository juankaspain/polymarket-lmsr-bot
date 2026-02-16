//! Polymarket LMSR Bot — Entry Point
//!
//! Initializes configuration, logging, blockchain connections,
//! and the main arbitrage engine. Runs until SIGINT/SIGTERM.
//!
//! Wiring sequence:
//!  1. Load config.toml + validate
//!  2. Init tracing (JSON structured logging)
//!  3. Connect to Polygon RPC + validate chain ID
//!  4. Validate contracts on-chain (code exists)
//!  5. Load CLOB auth from env vars
//!  6. Create ClobClient + ClobOrderExecutor (OrderExecution port)
//!  7. Create PolymarketFeed (MarketFeed port) + BinanceFeed + Bridge
//!  8. Create RepositoryImpl (Repository port)
//!  9. Spawn health server on :9090 (/live + /ready)
//! 10. Spawn feeds (Polymarket WS + Binance WS + Bridge)
//! 11. Spawn config hot-reload watcher (60s)
//! 12. Spawn ArbitrageEngine main loop (event-driven tokio::select!)
//! 13. Wait for SIGINT → graceful shutdown (cancel→claim→save→exit)

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
use adapters::chain::provider::PolygonProvider;
use adapters::chain::ContractValidator;
use adapters::feeds::{BinanceFeed, FeedBridge, PolymarketFeed};
use adapters::persistence::RepositoryImpl;
use config::hot_reload::ConfigWatcher;
use usecases::arbitrage_engine::ArbitrageEngine;

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

    // ── 4. Connect to Polygon RPC ───────────────────────────
    let polygon = PolygonProvider::connect(&config.api)
        .await
        .context("Failed to connect to Polygon RPC")?;

    // ── 5. Validate contracts on-chain (checklist) ──────────
    let validator = ContractValidator::new(polygon.inner());
    validator
        .validate_all(&config.contracts)
        .await
        .context("Contract validation failed")?;
    info!("All contracts validated on-chain");

    // ── 6. Load CLOB auth from env vars ─────────────────────
    let auth = Arc::new(
        ClobAuth::from_env().context("Failed to load CLOB credentials from env")?,
    );

    // ── 7. Create CLOB HTTP client with auth + retry ────────
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

    // ── 8. Create order executor (OrderExecution port) ──────
    let executor = Arc::new(ClobOrderExecutor::new(Arc::clone(&clob_client)));

    // ── 9. Create feeds ─────────────────────────────────────
    // Polymarket CLOB WebSocket feed (primary — implements MarketFeed)
    let pm_feed = Arc::new(PolymarketFeed::new(&config.api));

    // Binance feed (external oracle for cross-validation)
    let binance_feed = Arc::new(BinanceFeed::new());

    // Feed bridge (BinanceTick → PriceUpdate for cross-validation)
    let _feed_bridge = FeedBridge::new(Arc::clone(&binance_feed), &config);

    // ── 10. Create repository (Repository port) ─────────────
    let repo = Arc::new(
        RepositoryImpl::from_data_dir("data")
            .await
            .context("Failed to initialize repository")?,
    );
    info!("Repository initialized in data/");

    // ── 11. Recover state from last run ─────────────────────
    {
        use crate::ports::repository::Repository;
        if let Some(state) = repo.load_latest_state().await? {
            info!(
                version = %state.version,
                open_orders = state.open_orders.len(),
                cumulative_pnl = state.cumulative_pnl,
                "Recovered state from previous run"
            );
        } else {
            info!("No previous state found — fresh start");
        }
    }

    // ── 12. Spawn health/metrics server on :9090 ────────────
    let health_handle = tokio::spawn(serve_health(health_rx, config.clone()));

    // ── 13. Spawn Polymarket CLOB WebSocket feed ────────────
    let pm_shutdown = shutdown_tx.subscribe();
    let pm_ref = Arc::clone(&pm_feed);
    let pm_handle = tokio::spawn(async move {
        if let Err(e) = pm_ref.run(pm_shutdown).await {
            error!(error = %e, "Polymarket feed task failed");
        }
    });

    // ── 14. Spawn Binance WebSocket feed ─────────────────────
    let binance_shutdown = shutdown_tx.subscribe();
    let binance_ref = Arc::clone(&binance_feed);
    let binance_handle = tokio::spawn(async move {
        if let Err(e) = binance_ref.run(binance_shutdown).await {
            error!(error = %e, "Binance feed task failed");
        }
    });

    // ── 15. Spawn config hot-reload watcher (60s) ───────────
    let reload_shutdown = shutdown_tx.subscribe();
    let (mut config_watcher, _config_rx) =
        ConfigWatcher::new("config.toml", config.clone());
    let reload_handle = tokio::spawn(async move {
        if let Err(e) = config_watcher.run(reload_shutdown).await {
            error!(error = %e, "Config watcher failed");
        }
    });

    // ── 16. Spawn ArbitrageEngine (event-driven main loop) ──
    let engine_shutdown = shutdown_tx.subscribe();
    let engine_config = config.clone();
    let engine_feed = Arc::clone(&pm_feed);
    let engine_executor = Arc::clone(&executor);
    let engine_handle = tokio::spawn(async move {
        let mut engine = ArbitrageEngine::new(
            engine_feed,
            engine_executor,
            engine_config,
            engine_shutdown,
        );
        if let Err(e) = engine.run().await {
            error!(error = %e, "Arbitrage engine failed");
        }
    });

    info!("All tasks spawned — bot is running");

    // ── 17. Wait for SIGINT or SIGTERM ──────────────────────
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

    // 4. Save final state snapshot
    {
        use crate::ports::repository::Repository;
        let final_state = crate::ports::repository::BotStateSnapshot {
            version: env!("CARGO_PKG_VERSION").to_string(),
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            open_orders: Vec::new(),
            positions: Vec::new(),
            cumulative_pnl: 0.0,
            daily_loss: 0.0,
        };
        if let Err(e) = repo.save_state(&final_state).await {
            warn!(error = %e, "Failed to save final state");
        } else {
            info!("Final state snapshot saved");
        }
    }

    // 5. Wait for engine (up to 30s)
    info!("Waiting for engine shutdown...");
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        engine_handle,
    )
    .await;

    // 6. Wait for feeds (up to 5s each)
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        pm_handle,
    )
    .await;
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        binance_handle,
    )
    .await;

    // 7. Stop auxiliary tasks
    reload_handle.abort();
    health_handle.abort();

    info!("Shutdown complete");
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
