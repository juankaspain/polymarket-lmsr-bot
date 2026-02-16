//! Order Executor Port - High-level Order Orchestration
//!
//! Re-exports the `OrderExecution` trait and provides additional
//! orchestration types for the market maker use case.
//!
//! This module bridges the gap between raw CLOB execution and
//! the market maker's quoting logic by providing quote-level
//! abstractions.

pub use crate::ports::execution::{
  OrderCancellation, OrderExecution, OrderPlacement, OrderStatus,
};

use crate::domain::trade::{TokenId, TradeSide};

/// A two-sided quote (bid + ask) for a single token.
#[derive(Debug, Clone)]
pub struct Quote {
  /// Token being quoted.
  pub token_id: TokenId,
  /// Bid price (our buy price).
  pub bid_price: f64,
  /// Ask price (our sell price).
  pub ask_price: f64,
  /// Bid size in USDC.
  pub bid_size: f64,
  /// Ask size in USDC.
  pub ask_size: f64,
  /// Spread in basis points.
  pub spread_bps: f64,
}

impl Quote {
  /// Calculate the mid-price of this quote.
  pub fn mid_price(&self) -> f64 {
    (self.bid_price + self.ask_price) / 2.0
  }

  /// Check if the quote has a positive spread.
  pub fn is_valid(&self) -> bool {
    self.ask_price > self.bid_price
      && self.bid_size > 0.0
      && self.ask_size > 0.0
  }
}

/// Result of updating quotes for a market pair.
#[derive(Debug, Clone)]
pub struct QuoteUpdateResult {
  /// Number of orders cancelled.
  pub cancelled: usize,
  /// Number of new orders placed.
  pub placed: usize,
  /// Any errors during the update.
  pub errors: Vec<String>,
  /// Time taken for the update (microseconds).
  pub latency_us: u64,
}

/// Represents the desired position for a single side.
#[derive(Debug, Clone)]
pub struct DesiredOrder {
  /// Token to trade.
  pub token_id: TokenId,
  /// Buy or Sell.
  pub side: TradeSide,
  /// Target price.
  pub price: f64,
  /// Target size.
  pub size: f64,
}
