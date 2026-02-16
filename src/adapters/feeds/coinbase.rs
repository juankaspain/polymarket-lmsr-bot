//! Coinbase WebSocket Feed - Secondary BTC/ETH Price Source
//!
//! Provides cross-validation prices from Coinbase for the
//! Bayesian estimator. Helps detect feed anomalies and provides
//! redundancy if Binance disconnects.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::connect_async;
use tracing::{debug, info, instrument, warn};

/// A price tick from Coinbase.
#[derive(Debug, Clone)]
pub struct CoinbaseTick {
    /// Product ID (e.g., "BTC-USD").
    pub product_id: String,
    /// Latest trade price.
    pub price: f64,
    /// Timestamp in Unix milliseconds.
    pub timestamp_ms: u64,
}

/// Coinbase WebSocket subscribe message.
#[derive(Serialize)]
struct SubscribeMsg {
    #[serde(rename = "type")]
    msg_type: String,
    product_ids: Vec<String>,
    channels: Vec<String>,
}

/// Coinbase WebSocket ticker message.
#[derive(Debug, Deserialize)]
struct TickerMsg {
    #[serde(rename = "type")]
    msg_type: String,
    product_id: Option<String>,
    price: Option<String>,
    time: Option<String>,
}

/// Coinbase real-time price feed via WebSocket.
///
/// Subscribes to the ticker channel for BTC-USD and ETH-USD.
/// Used as secondary price source for cross-validation.
pub struct CoinbaseFeed {
    /// Broadcast sender for price ticks.
    tick_tx: broadcast::Sender<CoinbaseTick>,
    /// Last known prices (for debounce).
    last_prices: Arc<RwLock<HashMap<String, f64>>>,
    /// Minimum delta to emit (0.5% debounce).
    min_delta_pct: f64,
}

impl CoinbaseFeed {
    /// Create a new Coinbase feed.
    pub fn new() -> Self {
        let (tick_tx, _) = broadcast::channel(4096);

        Self {
            tick_tx,
            last_prices: Arc::new(RwLock::new(HashMap::new())),
            min_delta_pct: 0.005,
        }
    }

    /// Get a receiver for Coinbase price ticks.
    pub fn subscribe(&self) -> broadcast::Receiver<CoinbaseTick> {
        self.tick_tx.subscribe()
    }

    /// Run the WebSocket connection loop with auto-reconnect.
    #[instrument(skip(self, shutdown_rx))]
    pub async fn run(
        &self,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<()> {
        let ws_url = "wss://ws-feed.exchange.coinbase.com";

        info!(url = ws_url, "Connecting to Coinbase WebSocket");

        loop {
            match self.connect_and_stream(ws_url, &mut shutdown_rx).await {
                Ok(()) => {
                    info!("Coinbase feed shut down gracefully");
                    return Ok(());
                }
                Err(e) => {
                    warn!(error = %e, "Coinbase WebSocket disconnected, reconnecting in 5s");
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    /// Single connection session.
    async fn connect_and_stream(
        &self,
        ws_url: &str,
        shutdown_rx: &mut broadcast::Receiver<()>,
    ) -> Result<()> {
        let (ws_stream, _) = connect_async(ws_url)
            .await
            .context("Coinbase WebSocket connection failed")?;

        let (mut write, mut read) = ws_stream.split();

        // Subscribe to ticker channel
        let subscribe = SubscribeMsg {
            msg_type: "subscribe".to_string(),
            product_ids: vec!["BTC-USD".to_string(), "ETH-USD".to_string()],
            channels: vec!["ticker".to_string()],
        };

        let sub_json = serde_json::to_string(&subscribe)?;
        use futures_util::SinkExt;
        write
            .send(tokio_tungstenite::tungstenite::Message::Text(sub_json))
            .await
            .context("Failed to send subscribe")?;

        info!("Coinbase WebSocket subscribed to BTC-USD, ETH-USD");

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    return Ok(());
                }
                msg = read.next() => {
                    match msg {
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                            let _ = self.handle_message(&text).await;
                        }
                        Some(Err(e)) => {
                            return Err(anyhow::anyhow!("Coinbase WS error: {e}"));
                        }
                        None => {
                            return Err(anyhow::anyhow!("Coinbase WS stream ended"));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Parse and emit a ticker message.
    async fn handle_message(&self, text: &str) -> Result<()> {
        let msg: TickerMsg = serde_json::from_str(text)?;

        if msg.msg_type != "ticker" {
            return Ok(());
        }

        let product_id = msg.product_id.context("Missing product_id")?;
        let price: f64 = msg
            .price
            .context("Missing price")?
            .parse()
            .context("Invalid price")?;

        // Debounce
        {
            let last = self.last_prices.read().await;
            if let Some(&last_price) = last.get(&product_id) {
                let delta = ((price - last_price) / last_price).abs();
                if delta < self.min_delta_pct {
                    return Ok(());
                }
            }
        }

        {
            let mut last = self.last_prices.write().await;
            last.insert(product_id.clone(), price);
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let tick = CoinbaseTick {
            product_id,
            price,
            timestamp_ms: now_ms,
        };

        let _ = self.tick_tx.send(tick);
        Ok(())
    }
}
