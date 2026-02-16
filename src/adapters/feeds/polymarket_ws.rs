//! Polymarket CLOB WebSocket Feed — Primary Market Data Source
//!
//! Connects to the Polymarket CLOB WebSocket API and emits `PriceUpdate`
//! events via broadcast channels. Implements the `MarketFeed` port trait
//! so the domain/usecases layer never depends on transport details.
//!
//! Features:
//! - Per-token broadcast channels with 4096 buffer
//! - Debounce: skip updates where delta < 0.5% (checklist)
//! - Auto-reconnect on disconnect (5s backoff)
//! - Event-driven via tokio::select! (NEVER polling)

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::Deserialize;
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::connect_async;
use tracing::{debug, error, info, instrument, warn};

use crate::config::ApiConfig;
use crate::domain::trade::{MarketId, TokenId};
use crate::ports::market_feed::{MarketFeed, OrderBookSnapshot, PriceUpdate};

/// Raw order book message from Polymarket CLOB WebSocket.
#[derive(Debug, Deserialize)]
struct WsBookMessage {
    /// Market/asset identifier.
    #[serde(default)]
    market: String,
    /// Asset (token) identifier.
    #[serde(default)]
    asset_id: String,
    /// Best bid entries: [[price, size], ...].
    #[serde(default)]
    bids: Vec<Vec<String>>,
    /// Best ask entries: [[price, size], ...].
    #[serde(default)]
    asks: Vec<Vec<String>>,
    /// Server timestamp (Unix ms).
    #[serde(default)]
    timestamp: u64,
}

/// Internal state for a single token subscription.
struct TokenState {
    /// Broadcast sender for this token's price updates.
    tx: broadcast::Sender<PriceUpdate>,
    /// Last emitted mid-price for debounce.
    last_mid: Option<f64>,
    /// Last full order book snapshot.
    last_snapshot: Option<OrderBookSnapshot>,
}

/// Polymarket CLOB WebSocket feed adapter.
///
/// Implements `MarketFeed` port trait. Connects to the CLOB WS endpoint,
/// parses order book updates, and broadcasts `PriceUpdate` events to
/// subscribers. Uses debounce (skip delta < 0.5%) per checklist.
pub struct PolymarketFeed {
    /// Per-token subscription state.
    tokens: Arc<RwLock<HashMap<TokenId, TokenState>>>,
    /// WebSocket URL from config.
    ws_url: String,
    /// Minimum price delta to emit (0.5% = 0.005).
    min_delta_pct: f64,
}

impl PolymarketFeed {
    /// Create a new Polymarket feed from API config.
    pub fn new(config: &ApiConfig) -> Self {
        Self {
            tokens: Arc::new(RwLock::new(HashMap::new())),
            ws_url: config.clob_ws_url.clone(),
            min_delta_pct: 0.005,
        }
    }

    /// Ensure a token has a broadcast channel allocated.
    async fn ensure_token(&self, token_id: &TokenId) {
        let mut tokens = self.tokens.write().await;
        tokens.entry(token_id.clone()).or_insert_with(|| {
            let (tx, _) = broadcast::channel(4096);
            TokenState {
                tx,
                last_mid: None,
                last_snapshot: None,
            }
        });
    }

    /// Run the WebSocket connection loop with auto-reconnect.
    ///
    /// Listens for order book updates and broadcasts `PriceUpdate` events.
    /// Uses tokio::select! for event-driven processing (NEVER polling).
    #[instrument(skip(self, shutdown_rx))]
    pub async fn run(
        &self,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<()> {
        info!(url = %self.ws_url, "Connecting to Polymarket CLOB WebSocket");

        loop {
            match self.connect_and_stream(&mut shutdown_rx).await {
                Ok(()) => {
                    info!("Polymarket feed shut down gracefully");
                    return Ok(());
                }
                Err(e) => {
                    warn!(error = %e, "Polymarket WS disconnected, reconnecting in 5s");
                    // Check shutdown before sleeping
                    tokio::select! {
                        _ = shutdown_rx.recv() => return Ok(()),
                        _ = tokio::time::sleep(tokio::time::Duration::from_secs(5)) => {},
                    }
                }
            }
        }
    }

    /// Single WebSocket session: connect, subscribe, stream until error or shutdown.
    async fn connect_and_stream(
        &self,
        shutdown_rx: &mut broadcast::Receiver<()>,
    ) -> Result<()> {
        let (ws_stream, _) = connect_async(&self.ws_url)
            .await
            .context("Polymarket WebSocket connection failed")?;

        let (_write, mut read) = ws_stream.split();

        info!("Polymarket CLOB WebSocket connected");

        loop {
            tokio::select! {
                biased;
                _ = shutdown_rx.recv() => {
                    info!("Shutdown signal in Polymarket feed");
                    return Ok(());
                }
                msg = read.next() => {
                    match msg {
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                            if let Err(e) = self.handle_message(text.as_ref()).await {
                                debug!(error = %e, "Failed to parse Polymarket message");
                            }
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(_))) => {
                            debug!("Polymarket ping received");
                        }
                        Some(Err(e)) => {
                            return Err(anyhow::anyhow!("Polymarket WS error: {e}"));
                        }
                        None => {
                            return Err(anyhow::anyhow!("Polymarket WS stream ended"));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Parse a WebSocket message and emit PriceUpdate if delta exceeds threshold.
    async fn handle_message(&self, text: &str) -> Result<()> {
        let msg: WsBookMessage =
            serde_json::from_str(text).context("Invalid Polymarket WS JSON")?;

        let token_id = if msg.asset_id.is_empty() {
            msg.market.clone()
        } else {
            msg.asset_id.clone()
        };

        if token_id.is_empty() {
            return Ok(());
        }

        // Parse best bid/ask from the order book arrays
        let best_bid = msg
            .bids
            .first()
            .and_then(|entry| entry.first())
            .and_then(|p| p.parse::<f64>().ok());

        let best_ask = msg
            .asks
            .first()
            .and_then(|entry| entry.first())
            .and_then(|p| p.parse::<f64>().ok());

        let bid_size = msg
            .bids
            .first()
            .and_then(|entry| entry.get(1))
            .and_then(|s| s.parse::<f64>().ok());

        let ask_size = msg
            .asks
            .first()
            .and_then(|entry| entry.get(1))
            .and_then(|s| s.parse::<f64>().ok());

        let mid_price = match (best_bid, best_ask) {
            (Some(b), Some(a)) => Some((b + a) / 2.0),
            _ => None,
        };

        // Debounce: skip if delta < 0.5%
        let mut tokens = self.tokens.write().await;
        let state = tokens.entry(token_id.clone()).or_insert_with(|| {
            let (tx, _) = broadcast::channel(4096);
            TokenState {
                tx,
                last_mid: None,
                last_snapshot: None,
            }
        });

        if let (Some(mid), Some(last)) = (mid_price, state.last_mid) {
            let delta = ((mid - last) / last).abs();
            if delta < self.min_delta_pct {
                return Ok(());
            }
        }

        state.last_mid = mid_price;

        // Store snapshot
        let bids: Vec<(f64, f64)> = msg
            .bids
            .iter()
            .filter_map(|entry| {
                let price = entry.first()?.parse::<f64>().ok()?;
                let size = entry.get(1)?.parse::<f64>().ok()?;
                Some((price, size))
            })
            .collect();

        let asks: Vec<(f64, f64)> = msg
            .asks
            .iter()
            .filter_map(|entry| {
                let price = entry.first()?.parse::<f64>().ok()?;
                let size = entry.get(1)?.parse::<f64>().ok()?;
                Some((price, size))
            })
            .collect();

        state.last_snapshot = Some(OrderBookSnapshot {
            token_id: token_id.clone(),
            bids: bids.clone(),
            asks: asks.clone(),
            sequence: msg.timestamp,
            timestamp_ms: msg.timestamp,
        });

        let update = PriceUpdate {
            market_id: msg.market,
            token_id: token_id.clone(),
            best_bid,
            best_ask,
            mid_price,
            timestamp_ms: msg.timestamp,
            bid_size,
            ask_size,
        };

        // Broadcast (ignore if no receivers)
        let _ = state.tx.send(update);

        Ok(())
    }
}

#[async_trait]
impl MarketFeed for PolymarketFeed {
    fn subscribe(&self, token_id: &TokenId) -> broadcast::Receiver<PriceUpdate> {
        // We need a blocking approach since this is a sync fn.
        // Use try_write to avoid deadlock, or create channel on-the-fly.
        let mut tokens = self.tokens.blocking_write();
        let state = tokens.entry(token_id.clone()).or_insert_with(|| {
            let (tx, _) = broadcast::channel(4096);
            TokenState {
                tx,
                last_mid: None,
                last_snapshot: None,
            }
        });
        state.tx.subscribe()
    }

    async fn get_order_book(
        &self,
        token_id: &TokenId,
    ) -> Result<OrderBookSnapshot> {
        let tokens = self.tokens.read().await;
        tokens
            .get(token_id)
            .and_then(|s| s.last_snapshot.clone())
            .ok_or_else(|| anyhow::anyhow!("No order book snapshot for {token_id}"))
    }

    fn subscribe_many(
        &self,
        token_ids: &[TokenId],
    ) -> Vec<broadcast::Receiver<PriceUpdate>> {
        let mut tokens = self.tokens.blocking_write();
        token_ids
            .iter()
            .map(|tid| {
                let state = tokens.entry(tid.clone()).or_insert_with(|| {
                    let (tx, _) = broadcast::channel(4096);
                    TokenState {
                        tx,
                        last_mid: None,
                        last_snapshot: None,
                    }
                });
                state.tx.subscribe()
            })
            .collect()
    }

    async fn is_healthy(&self) -> bool {
        let tokens = self.tokens.read().await;
        // Healthy if we have at least one token with a recent snapshot
        tokens.values().any(|s| s.last_snapshot.is_some())
    }

    async fn last_price(&self, token_id: &TokenId) -> Option<PriceUpdate> {
        let tokens = self.tokens.read().await;
        let state = tokens.get(token_id)?;
        let snapshot = state.last_snapshot.as_ref()?;

        let best_bid = snapshot.bids.first().map(|(p, _)| *p);
        let best_ask = snapshot.asks.first().map(|(p, _)| *p);
        let mid_price = match (best_bid, best_ask) {
            (Some(b), Some(a)) => Some((b + a) / 2.0),
            _ => None,
        };

        Some(PriceUpdate {
            market_id: String::new(),
            token_id: token_id.clone(),
            best_bid,
            best_ask,
            mid_price,
            timestamp_ms: snapshot.timestamp_ms,
            bid_size: snapshot.bids.first().map(|(_, s)| *s),
            ask_size: snapshot.asks.first().map(|(_, s)| *s),
        })
    }
}
