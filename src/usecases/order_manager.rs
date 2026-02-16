//! Order Manager - Order Lifecycle Management
//!
//! Manages the full lifecycle of maker orders:
//! - Placing GTC post-only orders (0% fee + rebates)
//! - Tracking open orders
//! - Cancelling stale orders
//! - Rate limiting (50 orders/min)
//! - Graceful shutdown (cancel all)

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use tracing::{debug, info, instrument, warn};

use crate::config::AppConfig;
use crate::domain::trade::{Order, OrderId, OrderType, TradeSide, TokenId};
use crate::ports::execution::{OrderExecution, OrderPlacement};

/// Manages order placement with rate limiting and tracking.
pub struct OrderManager<E: OrderExecution> {
  /// Execution port.
  execution: Arc<E>,
  /// Currently tracked open orders.
  open_orders: HashMap<OrderId, Order>,
  /// Rate limiter: timestamps of recent orders.
  order_timestamps: Vec<Instant>,
  /// Maximum orders per minute.
  max_orders_per_minute: u32,
  /// Minimum interval between orders (ms).
  min_interval_ms: u64,
  /// Last order time.
  last_order_time: Option<Instant>,
}

impl<E: OrderExecution> OrderManager<E> {
  /// Create a new order manager.
  pub fn new(execution: Arc<E>, config: &AppConfig) -> Self {
    Self {
      execution,
      open_orders: HashMap::new(),
      order_timestamps: Vec::new(),
      max_orders_per_minute: config.rate_limits.max_orders_per_minute,
      min_interval_ms: config.rate_limits.min_interval_ms,
      last_order_time: None,
    }
  }

  /// Place a maker-only GTC order.
  ///
  /// All orders are post-only to guarantee maker execution
  /// (0% fee + potential rebates). Rate limiting is enforced.
  #[instrument(skip(self), fields(token = %token_id, price, size))]
  pub async fn place_maker_order(
    &mut self,
    token_id: &TokenId,
    price: f64,
    size: f64,
    is_buy: bool,
  ) -> Result<Option<OrderPlacement>> {
    // Rate limit check
    if !self.check_rate_limit() {
      debug!("Rate limit reached, skipping order");
      return Ok(None);
    }

    // Enforce minimum interval
    if let Some(last) = self.last_order_time {
      let elapsed = last.elapsed().as_millis() as u64;
      if elapsed < self.min_interval_ms {
        debug!(
          elapsed_ms = elapsed,
          min_ms = self.min_interval_ms,
          "Minimum interval not met"
        );
        return Ok(None);
      }
    }

    let side = if is_buy {
      TradeSide::Buy
    } else {
      TradeSide::Sell
    };

    let order = Order {
      id: String::new(), // Assigned by CLOB
      token_id: token_id.clone(),
      side,
      price,
      size,
      order_type: OrderType::Gtc,
      post_only: true,
      timestamp_ms: std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64,
    };

    let result = self.execution.place_order(&order).await?;

    if result.accepted {
      let mut tracked = order;
      tracked.id = result.order_id.clone();
      self.open_orders.insert(result.order_id.clone(), tracked);
      self.record_order();
      info!(
        order_id = %result.order_id,
        "Maker order placed successfully"
      );
    } else {
      warn!(
        reason = ?result.rejection_reason,
        "Order rejected"
      );
    }

    Ok(Some(result))
  }

  /// Cancel all open orders (for graceful shutdown).
  #[instrument(skip(self))]
  pub async fn cancel_all(&mut self) -> Result<usize> {
    let count = self.execution.cancel_all_orders().await?;
    self.open_orders.clear();
    info!(cancelled = count, "All orders cancelled");
    Ok(count)
  }

  /// Get the number of currently tracked open orders.
  pub fn open_order_count(&self) -> usize {
    self.open_orders.len()
  }

  /// Check if we're within rate limits.
  fn check_rate_limit(&mut self) -> bool {
    let now = Instant::now();
    // Remove timestamps older than 1 minute
    self
      .order_timestamps
      .retain(|t| now.duration_since(*t).as_secs() < 60);

    self.order_timestamps.len() < self.max_orders_per_minute as usize
  }

  /// Record an order placement for rate limiting.
  fn record_order(&mut self) {
    let now = Instant::now();
    self.order_timestamps.push(now);
    self.last_order_time = Some(now);
  }
}
