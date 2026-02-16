//! Repository Port - State Persistence Interface
//!
//! Defines traits for persisting bot state using JSONL files.
//! No database dependency - lightweight append-only log format
//! optimized for audit trails and crash recovery.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::trade::{MarketId, Order, OrderId};

/// A single trade record for persistence and auditing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
  /// Unique trade identifier.
  pub id: String,
  /// Associated order ID.
  pub order_id: OrderId,
  /// Market this trade belongs to.
  pub market_id: MarketId,
  /// Trade side (Buy/Sell).
  pub side: String,
  /// Execution price.
  pub price: f64,
  /// Trade size.
  pub size: f64,
  /// LMSR fair value at time of trade.
  pub lmsr_fair_value: f64,
  /// Edge captured (price vs fair value).
  pub edge: f64,
  /// Kelly fraction used for sizing.
  pub kelly_fraction: f64,
  /// Fees paid (should be 0 for maker).
  pub fees: f64,
  /// Timestamp (Unix ms).
  pub timestamp_ms: u64,
}

/// Daily P&L summary for risk monitoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyPnl {
  /// Date string (YYYY-MM-DD).
  pub date: String,
  /// Realized P&L for the day.
  pub realized_pnl: f64,
  /// Unrealized P&L (mark-to-market).
  pub unrealized_pnl: f64,
  /// Total number of trades.
  pub trade_count: u64,
  /// Total volume traded.
  pub volume: f64,
  /// Maximum drawdown during the day.
  pub max_drawdown: f64,
}

/// Bot state snapshot for crash recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotStateSnapshot {
  /// Version of the state format.
  pub version: String,
  /// Timestamp of snapshot (Unix ms).
  pub timestamp_ms: u64,
  /// Currently open orders.
  pub open_orders: Vec<Order>,
  /// Current position sizes per token.
  pub positions: Vec<(String, f64)>,
  /// Cumulative P&L.
  pub cumulative_pnl: f64,
  /// Daily loss so far.
  pub daily_loss: f64,
}

/// Trait for state persistence providers.
///
/// Uses JSONL (JSON Lines) format for append-only logging.
/// Each line is a self-contained JSON record, making it easy
/// to parse, stream, and recover from partial writes.
#[async_trait]
pub trait Repository: Send + Sync + 'static {
  /// Append a trade record to the trade log.
  async fn save_trade(&self, record: &TradeRecord) -> anyhow::Result<()>;

  /// Load all trade records (for recovery/analysis).
  async fn load_trades(&self) -> anyhow::Result<Vec<TradeRecord>>;

  /// Load trades for a specific date range.
  async fn load_trades_range(
    &self,
    from_ms: u64,
    to_ms: u64,
  ) -> anyhow::Result<Vec<TradeRecord>>;

  /// Save a bot state snapshot (for crash recovery).
  async fn save_state(&self, state: &BotStateSnapshot) -> anyhow::Result<()>;

  /// Load the most recent bot state snapshot.
  async fn load_latest_state(&self) -> anyhow::Result<Option<BotStateSnapshot>>;

  /// Save daily P&L record.
  async fn save_daily_pnl(&self, pnl: &DailyPnl) -> anyhow::Result<()>;

  /// Load daily P&L history.
  async fn load_daily_pnl(&self) -> anyhow::Result<Vec<DailyPnl>>;

  /// Check if the repository is healthy (disk space, permissions).
  async fn is_healthy(&self) -> bool;
}
