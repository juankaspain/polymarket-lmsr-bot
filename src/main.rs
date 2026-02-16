//! Polymarket LMSR Arbitrage Bot - Entry Point
//!
//! Version: 0.1.0
//!
//! High-frequency arbitrage bot for Polymarket prediction markets.
//! Uses LMSR pricing model, maker-first strategy (0% fees + rebates),
//! event-driven architecture, and professional risk management.

// Platform-specific allocator: jemalloc on Linux, system default on Windows
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

mod adapters;
mod config;
mod domain;
mod ports;
mod usecases;

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::signal;
use tokio::sync::{broadcast, watch};
use tracing::{error, info, warn};

/// Application version from Cargo.toml.
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file
    dotenvy::dotenv().ok();

    // Initialize structured logging
    let log_filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    tracing_subscriber::fmt()
        .with_env_filter(&log_filter)
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    info!(
        version = VERSION,
        pid = std::process::id(),
        "Starting Polymarket LMSR Arbitrage Bot"
    );

    // Load configuration
    let config = config::loader::load_config()
        .context("Failed to load configuration")?;

    info!(
        mode = ?config.bot.mode,
        assets = ?config.strategy.assets,
        "Configuration loaded successfully"
    );

    // Create shutdown channels
    let (shutdown_tx, _) = broadcast::channel::<()>(1);
    let (health_tx, health_rx) = watch::channel(true);

    // Spawn health check server
    let health_port = config.metrics.health_port;
    let health_handle = tokio::spawn(async move {
        info!(port = health_port, "Health check server starting");
        // Health server will be implemented in adapters/metrics/health.rs
    });

    // Log startup summary
    info!(
        strategy = "maker-first (0% fees + rebates)",
        kelly = "quarter-Kelly (0.25x)",
        rate_limit = config.rate_limits.max_orders_per_minute,
        max_daily_loss = %config.risk.max_daily_loss_fraction,
        "Bot configuration summary"
    );

    info!("Bot is ready. Waiting for market events...");

    // Wait for shutdown signal (Ctrl+C or SIGTERM)
    match signal::ctrl_c().await {
        Ok(()) => {
            info!("Shutdown signal received, initiating graceful shutdown...");
        }
        Err(e) => {
            error!(error = %e, "Failed to listen for shutdown signal");
        }
    }

    // Graceful shutdown sequence
    info!("Step 1/4: Cancelling open orders...");
    // TODO: Cancel all open orders via CLOB API

    info!("Step 2/4: Saving state to disk...");
    // TODO: Persist current state to JSONL

    info!("Step 3/4: Flushing metrics...");
    // TODO: Final metrics export

    info!("Step 4/4: Closing connections...");
    let _ = shutdown_tx.send(());
    drop(health_tx);

    info!(version = VERSION, "Bot shutdown complete. Goodbye!");
    Ok(())
}
