//! Market Feed Port - Real-time Market Data Interface
//!
//! Defines the trait for receiving real-time market data updates
//! from prediction market platforms (e.g., Polymarket WebSocket).

use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::domain::trade::{MarketId, TokenId};

/// Real-time price update from the order book.
#[derive(Debug, Clone)]
pub struct PriceUpdate {
  /// Market condition identifier.
  pub market_id: MarketId,
  /// Token identifier (YES or NO outcome).
  pub token_id: TokenId,
  /// Best bid price (highest buy order).
  pub best_bid: Option<f64>,
  /// Best ask price (lowest sell order).
  pub best_ask: Option<f64>,
  /// Mid-market price derived from bid/ask.
  pub mid_price: Option<f64>,
  /// Timestamp of the update (Unix ms).
  pub timestamp_ms: u64,
  /// Total volume at best bid.
  pub bid_size: Option<f64>,
  /// Total volume at best ask.
  pub ask_size: Option<f64>,
}

/// Order book snapshot for a single token.
#[derive(Debug, Clone)]
pub struct OrderBookSnapshot {
  /// Token identifier.
  pub token_id: TokenId,
  /// Bid levels sorted by price descending: (price, size).
  pub bids: Vec<(f64, f64)>,
  /// Ask levels sorted by price ascending: (price, size).
  pub asks: Vec<(f64, f64)>,
  /// Sequence number for ordering.
  pub sequence: u64,
  /// Snapshot timestamp (Unix ms).
  pub timestamp_ms: u64,
}

/// Trait for market data feed providers.
///
/// Implementors connect to real-time data sources (WebSocket, polling)
/// and emit price updates via a broadcast channel. The hexagonal
/// architecture ensures the domain never depends on transport details.
#[async_trait]
pub trait MarketFeed: Send + Sync + 'static {
  /// Subscribe to a specific token's price updates.
  ///
  /// Returns a broadcast receiver that emits `PriceUpdate` events
  /// whenever the order book changes for the given token.
  fn subscribe(&self, token_id: &TokenId) -> broadcast::Receiver<PriceUpdate>;

  /// Get the current order book snapshot for a token.
  ///
  /// Used for initial state recovery and periodic reconciliation.
  async fn get_order_book(&self, token_id: &TokenId) -> anyhow::Result<OrderBookSnapshot>;

  /// Subscribe to multiple tokens at once (batch subscription).
  ///
  /// More efficient than individual subscriptions for market pairs.
  fn subscribe_many(&self, token_ids: &[TokenId]) -> Vec<broadcast::Receiver<PriceUpdate>>;

  /// Check if the feed connection is healthy.
  async fn is_healthy(&self) -> bool;

  /// Get the last known price for a token without subscribing.
  async fn last_price(&self, token_id: &TokenId) -> Option<PriceUpdate>;
}
