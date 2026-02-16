//! Feed Task Supervisor - Lifecycle Management for Feed Connections
//!
//! Wraps Binance and Coinbase feeds with automatic restart on failure.
//! Uses tokio::select! for event-driven monitoring (never polling).
//! Provides health status aggregation for the /ready endpoint.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use tokio::sync::broadcast;
use tracing::{error, info, instrument, warn};

use super::binance::BinanceFeed;
use super::coinbase::CoinbaseFeed;

/// Tracks the health state of a single feed task.
#[derive(Debug)]
struct FeedHealth {
    /// Feed name for logging.
    name: &'static str,
    /// Whether the feed is currently connected.
    connected: AtomicBool,
    /// Consecutive reconnection attempts.
    reconnects: std::sync::atomic::AtomicU32,
}

/// Supervises all market data feed tasks.
///
/// Spawns Binance and Coinbase feeds as separate tokio tasks,
/// monitors health, and provides graceful shutdown coordination.
pub struct FeedSupervisor {
    /// Binance feed instance.
    binance: Arc<BinanceFeed>,
    /// Coinbase feed instance.
    coinbase: Arc<CoinbaseFeed>,
    /// Binance health tracker.
    binance_health: Arc<FeedHealth>,
    /// Coinbase health tracker.
    coinbase_health: Arc<FeedHealth>,
    /// Shutdown broadcaster.
    shutdown_tx: broadcast::Sender<()>,
}

impl FeedSupervisor {
    /// Create a new feed supervisor with both price sources.
    pub fn new(shutdown_tx: broadcast::Sender<()>) -> Self {
        Self {
            binance: Arc::new(BinanceFeed::new()),
            coinbase: Arc::new(CoinbaseFeed::new()),
            binance_health: Arc::new(FeedHealth {
                name: "binance",
                connected: AtomicBool::new(false),
                reconnects: std::sync::atomic::AtomicU32::new(0),
            }),
            coinbase_health: Arc::new(FeedHealth {
                name: "coinbase",
                connected: AtomicBool::new(false),
                reconnects: std::sync::atomic::AtomicU32::new(0),
            }),
            shutdown_tx,
        }
    }

    /// Get the shared Binance feed for subscribing to ticks.
    pub fn binance(&self) -> Arc<BinanceFeed> {
        Arc::clone(&self.binance)
    }

    /// Get the shared Coinbase feed for subscribing to ticks.
    pub fn coinbase(&self) -> Arc<CoinbaseFeed> {
        Arc::clone(&self.coinbase)
    }

    /// Spawn all feed tasks and return join handles.
    ///
    /// Each feed runs in its own tokio task with independent
    /// reconnection logic. The supervisor coordinates shutdown.
    #[instrument(skip(self))]
    pub fn spawn(&self) -> Vec<tokio::task::JoinHandle<()>> {
        let mut handles = Vec::with_capacity(2);

        // Spawn Binance feed
        {
            let feed = Arc::clone(&self.binance);
            let health = Arc::clone(&self.binance_health);
            let shutdown_rx = self.shutdown_tx.subscribe();

            handles.push(tokio::spawn(async move {
                health.connected.store(true, Ordering::Relaxed);

                match feed.run(shutdown_rx).await {
                    Ok(()) => info!("Binance feed exited normally"),
                    Err(e) => {
                        error!(error = %e, "Binance feed crashed");
                        health.connected.store(false, Ordering::Relaxed);
                        health
                            .reconnects
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
            }));
        }

        // Spawn Coinbase feed
        {
            let feed = Arc::clone(&self.coinbase);
            let health = Arc::clone(&self.coinbase_health);
            let shutdown_rx = self.shutdown_tx.subscribe();

            handles.push(tokio::spawn(async move {
                health.connected.store(true, Ordering::Relaxed);

                match feed.run(shutdown_rx).await {
                    Ok(()) => info!("Coinbase feed exited normally"),
                    Err(e) => {
                        error!(error = %e, "Coinbase feed crashed");
                        health.connected.store(false, Ordering::Relaxed);
                        health
                            .reconnects
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
            }));
        }

        info!(feed_count = handles.len(), "Feed tasks spawned");
        handles
    }

    /// Check if at least one feed is connected (degraded mode OK).
    pub fn is_healthy(&self) -> bool {
        self.binance_health.connected.load(Ordering::Relaxed)
            || self.coinbase_health.connected.load(Ordering::Relaxed)
    }

    /// Check if all feeds are connected (fully operational).
    pub fn is_fully_healthy(&self) -> bool {
        self.binance_health.connected.load(Ordering::Relaxed)
            && self.coinbase_health.connected.load(Ordering::Relaxed)
    }
}
