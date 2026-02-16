//! Order Execution Port - CLOB Order Management Interface
//!
//! Defines the trait for placing, cancelling, and managing orders
//! on the Polymarket CLOB (Central Limit Order Book).
//!
//! Key design decisions:
//! - Maker-only orders (GTC + post-only) for 0% fees + rebates
//! - Batch operations for efficient order management
//! - Rate-limit aware interface (50 orders/minute)

use async_trait::async_trait;

use crate::domain::trade::{Order, OrderId, TokenId, TradeSide};

/// Result of an order placement attempt.
#[derive(Debug, Clone)]
pub struct OrderPlacement {
  /// Assigned order ID from the CLOB.
  pub order_id: OrderId,
  /// Whether the order was accepted.
  pub accepted: bool,
  /// Rejection reason if not accepted.
  pub rejection_reason: Option<String>,
  /// Server timestamp of placement (Unix ms).
  pub timestamp_ms: u64,
}

/// Result of an order cancellation attempt.
#[derive(Debug, Clone)]
pub struct OrderCancellation {
  /// The order ID that was cancelled.
  pub order_id: OrderId,
  /// Whether cancellation succeeded.
  pub success: bool,
  /// Error message if cancellation failed.
  pub error: Option<String>,
}

/// Current status of an open order.
#[derive(Debug, Clone)]
pub enum OrderStatus {
  /// Order is live on the book.
  Open {
    /// Remaining unfilled size.
    remaining_size: f64,
    /// Original order size.
    original_size: f64,
  },
  /// Order was fully filled.
  Filled {
    /// Average fill price.
    avg_price: f64,
    /// Total filled size.
    filled_size: f64,
  },
  /// Order was partially filled.
  PartiallyFilled {
    /// Size already filled.
    filled_size: f64,
    /// Remaining size on book.
    remaining_size: f64,
    /// Average fill price so far.
    avg_price: f64,
  },
  /// Order was cancelled.
  Cancelled,
  /// Order status unknown.
  Unknown,
}

/// Trait for order execution providers.
///
/// Implementors connect to the Polymarket CLOB API and handle
/// the full order lifecycle. All orders MUST be maker-only
/// (GTC + post-only) to ensure 0% fees and potential rebates.
#[async_trait]
pub trait OrderExecution: Send + Sync + 'static {
  /// Place a single maker order on the CLOB.
  ///
  /// Orders are always GTC (Good-Till-Cancel) with post-only flag
  /// to guarantee maker execution (0% fee + rebates).
  ///
  /// # Errors
  /// Returns error if the order is rejected or rate-limited.
  async fn place_order(&self, order: &Order) -> anyhow::Result<OrderPlacement>;

  /// Cancel a single order by ID.
  async fn cancel_order(&self, order_id: &OrderId) -> anyhow::Result<OrderCancellation>;

  /// Cancel all open orders (used during graceful shutdown).
  ///
  /// Returns the number of orders successfully cancelled.
  async fn cancel_all_orders(&self) -> anyhow::Result<usize>;

  /// Cancel all orders for a specific token.
  async fn cancel_orders_for_token(
    &self,
    token_id: &TokenId,
  ) -> anyhow::Result<Vec<OrderCancellation>>;

  /// Get the current status of an order.
  async fn get_order_status(&self, order_id: &OrderId) -> anyhow::Result<OrderStatus>;

  /// Get all currently open orders.
  async fn get_open_orders(&self) -> anyhow::Result<Vec<Order>>;

  /// Check available balance for a given side.
  ///
  /// Returns the USDC balance available for trading.
  async fn available_balance(&self, side: TradeSide) -> anyhow::Result<f64>;

  /// Check if the execution connection is healthy.
  async fn is_healthy(&self) -> bool;

  /// Get the current rate limit status.
  ///
  /// Returns (remaining_requests, reset_time_ms).
  async fn rate_limit_status(&self) -> (u32, u64);
}
