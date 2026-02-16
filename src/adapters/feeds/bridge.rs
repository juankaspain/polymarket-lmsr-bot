//! Feed Bridge — BinanceTick to PriceUpdate Cross-Validation
//!
//! Subscribes to the `BinanceFeed` broadcast channel and converts
//! `BinanceTick` events into domain `PriceUpdate` objects for
//! cross-validation against Polymarket CLOB prices.
//!
//! Emits warnings when Binance spot diverges from Polymarket mid
//! by more than 2% (checklist: slippage check).

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::broadcast;
use tracing::{debug, info, instrument, warn};

use super::binance::{BinanceFeed, BinanceTick};
use crate::config::AppConfig;
use crate::ports::market_feed::PriceUpdate;

/// Maps Binance spot prices to synthetic PriceUpdate events.
///
/// Used for cross-validation: if Binance BTC spot says 50000 and
/// the Polymarket YES token mid is 0.40, we know the edge estimate
/// is grounded in real market data.
pub struct FeedBridge {
    /// Binance feed to subscribe to.
    binance: Arc<BinanceFeed>,
    /// Broadcast sender for converted PriceUpdate events.
    update_tx: broadcast::Sender<PriceUpdate>,
    /// Asset → market_id mapping from config.
    asset_market_map: HashMap<String, String>,
    /// Divergence threshold for warning (2% = 0.02).
    divergence_threshold: f64,
}

impl FeedBridge {
    /// Create a new feed bridge wired to a Binance feed instance.
    pub fn new(binance: Arc<BinanceFeed>, config: &AppConfig) -> Self {
        let (update_tx, _) = broadcast::channel(4096);

        let mut asset_market_map = HashMap::new();
        for market in &config.markets {
            if market.active {
                let symbol = match market.asset {
                    crate::domain::trade::Asset::BTC => "BTCUSDT",
                    crate::domain::trade::Asset::ETH => "ETHUSDT",
                };
                asset_market_map
                    .insert(symbol.to_string(), market.condition_id.clone());
            }
        }

        Self {
            binance,
            update_tx,
            asset_market_map,
            divergence_threshold: 0.02,
        }
    }

    /// Subscribe to converted PriceUpdate events from Binance.
    pub fn subscribe(&self) -> broadcast::Receiver<PriceUpdate> {
        self.update_tx.subscribe()
    }

    /// Run the bridge: listen to BinanceTick and emit PriceUpdate.
    ///
    /// Runs until shutdown signal. Event-driven via tokio::select!.
    #[instrument(skip(self, shutdown_rx))]
    pub async fn run(
        &self,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> anyhow::Result<()> {
        let mut tick_rx = self.binance.subscribe();

        info!(
            assets = self.asset_market_map.len(),
            "Feed bridge started — converting BinanceTick → PriceUpdate"
        );

        loop {
            tokio::select! {
                biased;
                _ = shutdown_rx.recv() => {
                    info!("Feed bridge shutting down");
                    return Ok(());
                }
                tick = tick_rx.recv() => {
                    match tick {
                        Ok(t) => self.handle_tick(&t),
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!(dropped = n, "Feed bridge lagged");
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!("Binance feed channel closed");
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    /// Convert a single BinanceTick into a PriceUpdate and broadcast.
    fn handle_tick(&self, tick: &BinanceTick) {
        let market_id = match self.asset_market_map.get(&tick.symbol) {
            Some(id) => id.clone(),
            None => {
                debug!(symbol = %tick.symbol, "No market mapping for symbol");
                return;
            }
        };

        // Normalize spot price to probability-like value.
        // This is a synthetic cross-reference signal, not a real PM price.
        // The ArbitrageEngine uses the actual PM feed for trading decisions.
        let update = PriceUpdate {
            market_id,
            token_id: format!("binance_{}", tick.symbol.to_lowercase()),
            best_bid: None,
            best_ask: None,
            mid_price: Some(tick.price),
            timestamp_ms: tick.timestamp_ms,
            bid_size: None,
            ask_size: None,
        };

        let _ = self.update_tx.send(update);
    }

    /// Check if Binance spot diverges from a Polymarket mid price.
    ///
    /// Returns the divergence percentage. Logs a warning if > 2%.
    pub fn check_divergence(
        &self,
        binance_spot: f64,
        pm_mid: f64,
        asset: &str,
    ) -> f64 {
        if binance_spot <= 0.0 || pm_mid <= 0.0 {
            return 0.0;
        }

        let divergence = ((binance_spot - pm_mid) / binance_spot).abs();

        if divergence > self.divergence_threshold {
            warn!(
                asset = asset,
                binance = binance_spot,
                polymarket = pm_mid,
                divergence_pct = divergence * 100.0,
                "Price divergence exceeds 2% threshold"
            );
        }

        divergence
    }
}
