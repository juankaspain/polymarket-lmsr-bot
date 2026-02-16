//! Binance WebSocket Feed - Primary BTC/ETH Price Source
//!
//! Connects to Binance's real-time trade stream via WebSocket
//! for sub-10ms price updates. Used as the primary oracle for
//! the LMSR pricing model.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::connect_async;
use tracing::{debug, error, info, instrument, warn};

use crate::domain::trade::Asset;

/// A price tick from Binance for internal routing.
#[derive(Debug, Clone)]
pub struct BinanceTick {
    /// Trading pair symbol (e.g., "BTCUSDT").
    pub symbol: String,
    /// Latest trade price.
    pub price: f64,
    /// Timestamp in Unix milliseconds.
    pub timestamp_ms: u64,
    /// Trade quantity.
    pub quantity: f64,
}

/// Binance WebSocket aggregate trade message.
#[derive(Debug, Deserialize)]
struct AggTradeMsg {
    /// Symbol.
    s: String,
    /// Price as string.
    p: String,
    /// Quantity as string.
    q: String,
    /// Trade time (Unix ms).
    #[serde(rename = "T")]
    trade_time: u64,
}

/// Binance real-time price feed via WebSocket.
///
/// Subscribes to aggTrade streams for BTC/USDT and ETH/USDT.
/// Emits ticks through a broadcast channel for downstream consumers.
pub struct BinanceFeed {
    /// Broadcast sender for price ticks.
    tick_tx: broadcast::Sender<BinanceTick>,
    /// Last known prices per asset (for dedup/debounce).
    last_prices: Arc<RwLock<HashMap<String, f64>>>,
    /// WebSocket URL.
    ws_url: String,
    /// Minimum price change to emit (debounce delta < 0.5%).
    min_delta_pct: f64,
}

impl BinanceFeed {
    /// Create a new Binance feed with default WebSocket endpoint.
    pub fn new() -> Self {
        let (tick_tx, _) = broadcast::channel(4096);

        Self {
            tick_tx,
            last_prices: Arc::new(RwLock::new(HashMap::new())),
            ws_url: "wss://stream.binance.com:9443/ws/btcusdt@aggTrade/ethusdt@aggTrade"
                .to_string(),
            min_delta_pct: 0.005, // 0.5% debounce per checklist
        }
    }

    /// Get a receiver for price ticks.
    pub fn subscribe(&self) -> broadcast::Receiver<BinanceTick> {
        self.tick_tx.subscribe()
    }

    /// Map a Binance symbol to our Asset enum.
    pub fn symbol_to_asset(symbol: &str) -> Option<Asset> {
        match symbol {
            "BTCUSDT" => Some(Asset::BTC),
            "ETHUSDT" => Some(Asset::ETH),
            _ => None,
        }
    }

    /// Run the WebSocket connection loop.
    ///
    /// Reconnects automatically on disconnect. Uses event-driven
    /// architecture (tokio::select!) — never polls on interval.
    #[instrument(skip(self, shutdown_rx))]
    pub async fn run(
        &self,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<()> {
        info!(url = %self.ws_url, "Connecting to Binance WebSocket");

        loop {
            match self.connect_and_stream(&mut shutdown_rx).await {
                Ok(()) => {
                    info!("Binance feed shut down gracefully");
                    return Ok(());
                }
                Err(e) => {
                    warn!(error = %e, "Binance WebSocket disconnected, reconnecting in 5s");
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    /// Single connection session: connect, stream, exit on error or shutdown.
    async fn connect_and_stream(
        &self,
        shutdown_rx: &mut broadcast::Receiver<()>,
    ) -> Result<()> {
        let (ws_stream, _) = connect_async(&self.ws_url)
            .await
            .context("Binance WebSocket connection failed")?;

        let (mut _write, mut read) = ws_stream.split();

        info!("Binance WebSocket connected");

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("Shutdown signal received in Binance feed");
                    return Ok(());
                }
                msg = read.next() => {
                    match msg {
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                            if let Err(e) = self.handle_message(&text).await {
                                debug!(error = %e, "Failed to parse Binance message");
                            }
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(data))) => {
                            // Pong is handled automatically by tungstenite
                            debug!(len = data.len(), "Binance ping received");
                        }
                        Some(Err(e)) => {
                            return Err(anyhow::anyhow!("WebSocket error: {e}"));
                        }
                        None => {
                            return Err(anyhow::anyhow!("WebSocket stream ended"));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Parse and emit a single WebSocket message.
    async fn handle_message(&self, text: &str) -> Result<()> {
        let msg: AggTradeMsg =
            serde_json::from_str(text).context("Invalid aggTrade JSON")?;

        let price: f64 = msg.p.parse().context("Invalid price")?;
        let quantity: f64 = msg.q.parse().context("Invalid quantity")?;

        // Debounce: skip if delta < 0.5% from last emitted price
        {
            let last = self.last_prices.read().await;
            if let Some(&last_price) = last.get(&msg.s) {
                let delta_pct = ((price - last_price) / last_price).abs();
                if delta_pct < self.min_delta_pct {
                    return Ok(());
                }
            }
        }

        // Update last price
        {
            let mut last = self.last_prices.write().await;
            last.insert(msg.s.clone(), price);
        }

        let tick = BinanceTick {
            symbol: msg.s,
            price,
            timestamp_ms: msg.trade_time,
            quantity,
        };

        // Broadcast (ignore if no receivers)
        let _ = self.tick_tx.send(tick);

        Ok(())
    }
}
