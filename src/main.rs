//! Polymarket LMSR Bot — Entry Point
//!
//! Initializes configuration, logging, blockchain connections,
//! and the main arbitrage engine. Runs until SIGINT/SIGTERM.

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

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration from config.toml
    let config = config::loader::load_config("config.toml")
        .context("Failed to load configuration")?;

    // Initialize tracing subscriber with JSON output
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
        dry_run = config.bot.dry_run,
        markets = config.markets.len(),
        "Starting Polymarket LMSR Bot"
    );

    // Shutdown signal handling
    let (shutdown_tx, _shutdown_rx) = broadcast::channel::<()>(1);
    let (health_tx, health_rx) = watch::channel(true);

    // Spawn health/metrics server
    let health_handle = tokio::spawn(serve_health(health_rx, config.clone()));

    // Spawn main engine
    let shutdown_rx_engine = shutdown_tx.subscribe();
    let engine_config = config.clone();
    let engine_handle = tokio::spawn(async move {
        if let Err(e) = run_engine(engine_config, shutdown_rx_engine).await {
            error!(error = %e, "Engine failed");
        }
    });

    // Wait for SIGINT or SIGTERM
    tokio::select! {
        _ = signal::ctrl_c() => {
            info!("SIGINT received, initiating graceful shutdown");
        }
    }

    // Graceful shutdown sequence per checklist:
    // 1. Signal all tasks to stop
    let _ = shutdown_tx.send(());

    // 2. Mark health as unhealthy so readiness probe fails
    let _ = health_tx.send(false);

    // 3. Wait for engine to finish (cancel orders + claim + save state)
    info!("Waiting for engine to complete shutdown...");
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        engine_handle,
    )
    .await;

    // 4. Stop health server
    health_handle.abort();

    info!("Shutdown complete");
    Ok(())
}

/// Run the main arbitrage engine.
async fn run_engine(
    _config: config::AppConfig,
    mut _shutdown_rx: broadcast::Receiver<()>,
) -> Result<()> {
    // TODO: Initialize chain provider, CLOB client, feeds, and ArbitrageEngine
    // This is the integration point where adapters are wired to ports.
    //
    // Example wiring (pseudocode):
    //   let provider = PolygonProvider::connect(&config.api).await?;
    //   let gas_oracle = GasOracle::new(Arc::new(provider));
    //   let clob_client = ClobClient::new(&config.api)?;
    //   let feed = ... ;
    //   let execution = ... ;
    //   let mut engine = ArbitrageEngine::new(feed, execution, config, shutdown_rx);
    //   engine.run().await
    warn!("Engine wiring not yet complete — dry run mode");
    Ok(())
}

/// Serve health and metrics endpoints on :9090.
async fn serve_health(
    health_rx: watch::Receiver<bool>,
    _config: config::AppConfig,
) -> Result<()> {
    use axum::{Router, routing::get, extract::State, http::StatusCode};

    let app = Router::new()
        .route("/live", get(|| async { StatusCode::OK }))
        .route(
            "/ready",
            get(move |State(rx): State<watch::Receiver<bool>>| async move {
                if *rx.borrow() {
                    StatusCode::OK
                } else {
                    StatusCode::SERVICE_UNAVAILABLE
                }
            }),
        )
        .with_state(health_rx);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:9090").await?;
    info!("Health server listening on :9090");
    axum::serve(listener, app).await?;
    Ok(())
}
