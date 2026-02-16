//! Core trading domain types.
//!
//! Defines all business entities: assets, markets, orders, trades, and positions.
//! These types are the foundation of the hexagonal architecture's inner ring.
//!
//! Exposes two API surfaces:
//! - Rich types (Decimal, Uuid, DateTime) for domain-internal logic
//! - Lightweight aliases and f64-based structs for ports/adapters boundary

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ────────────────────────────────────────────
// Type aliases consumed by ports and adapters
// ────────────────────────────────────────────

/// Lightweight token identifier used at the ports boundary.
pub type TokenId = String;

/// Lightweight order identifier used at the ports boundary.
pub type OrderId = String;

/// Lightweight market / condition identifier used at the ports boundary.
pub type MarketId = String;

// ────────────────────────────────────────────
// Enums shared across domain and ports
// ────────────────────────────────────────────

/// Supported trading assets on Polymarket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Asset {
    /// Bitcoin 5-minute prediction markets
    BTC,
    /// Ethereum 5-minute prediction markets
    ETH,
}

impl std::fmt::Display for Asset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BTC => write!(f, "BTC"),
            Self::ETH => write!(f, "ETH"),
        }
    }
}

/// Trade side — canonical enum used by both domain and ports.
///
/// Ports and usecases reference this as `TradeSide`.
/// Domain-internal code may also use `OrderSide` (alias below).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeSide {
    Buy,
    Sell,
}

impl std::fmt::Display for TradeSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Buy => write!(f, "BUY"),
            Self::Sell => write!(f, "SELL"),
        }
    }
}

/// Backward-compatible alias so existing domain code using `OrderSide` compiles.
pub type OrderSide = TradeSide;

/// Order type configuration.
///
/// `Gtc` is the primary maker-only type (post-only implied).
/// `Gtd` carries an explicit expiration in seconds (90 s per checklist).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    /// Good-til-cancelled, post-only (maker). Primary order type.
    Gtc,
    /// Good-til-date with expiration. Used for time-sensitive markets.
    Gtd { expiration_secs: u64 },
}

/// Lifecycle status of an order (domain-internal rich version).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    /// Order created locally, not yet sent
    Pending,
    /// Sent to CLOB, awaiting placement
    Submitted,
    /// Resting on the order book (maker)
    Open,
    /// Partially filled
    PartiallyFilled,
    /// Completely filled
    Filled,
    /// Cancelled by us or expired
    Cancelled,
    /// Rejected by the CLOB
    Rejected,
}

// ────────────────────────────────────────────
// Lightweight Order struct for ports/adapters
// ────────────────────────────────────────────

/// Lightweight order representation used at the ports boundary.
///
/// This is the struct that `OrderExecution` trait methods accept
/// and that `OrderManager` constructs. It uses `f64`/`String`
/// for frictionless serialization to the CLOB REST API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    /// CLOB-assigned order ID (empty until submitted).
    pub id: OrderId,
    /// Token ID (YES or NO outcome).
    pub token_id: TokenId,
    /// Buy or sell.
    pub side: TradeSide,
    /// Price in USDC (0.01 – 0.99).
    pub price: f64,
    /// Size in contracts.
    pub size: f64,
    /// Order type.
    pub order_type: OrderType,
    /// Whether this is a post-only (maker) order.
    pub post_only: bool,
    /// Creation timestamp in Unix milliseconds.
    pub timestamp_ms: u64,
}

impl Order {
    /// Create a new pending maker order with sensible defaults.
    pub fn new_maker(
        token_id: TokenId,
        side: TradeSide,
        price: f64,
        size: f64,
    ) -> Self {
        Self {
            id: String::new(),
            token_id,
            side,
            price,
            size,
            order_type: OrderType::Gtc,
            post_only: true,
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }
    }
}

// ────────────────────────────────────────────
// Rich domain types (Decimal/Uuid) for internal logic
// ────────────────────────────────────────────

/// Rich order representation for domain-internal accounting.
///
/// Used by the `Trade` and `Position` structs that need
/// precise decimal arithmetic and UUID tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RichOrder {
    /// Internal order ID
    pub id: Uuid,
    /// Market this order belongs to
    pub condition_id: String,
    /// Token ID (YES or NO outcome)
    pub token_id: String,
    /// Buy or sell
    pub side: TradeSide,
    /// Price in USDC (0.01 to 0.99 for prediction markets)
    pub price: Decimal,
    /// Size in contracts
    pub size: Decimal,
    /// Order type
    pub order_type: OrderType,
    /// Current lifecycle status
    pub status: OrderStatus,
    /// CLOB-assigned order ID (once submitted)
    pub clob_order_id: Option<String>,
    /// Timestamp when the order was created
    pub created_at: DateTime<Utc>,
    /// Timestamp when the order was last updated
    pub updated_at: DateTime<Utc>,
    /// Associated asset
    pub asset: Asset,
}

impl RichOrder {
    /// Creates a new pending order with maker-first defaults.
    pub fn new_maker(
        condition_id: String,
        token_id: String,
        side: TradeSide,
        price: Decimal,
        size: Decimal,
        asset: Asset,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            condition_id,
            token_id,
            side,
            price,
            size,
            order_type: OrderType::Gtc,
            status: OrderStatus::Pending,
            clob_order_id: None,
            created_at: now,
            updated_at: now,
            asset,
        }
    }

    /// Convert rich order into lightweight boundary Order.
    pub fn to_boundary_order(&self) -> Order {
        use rust_decimal::prelude::*;
        Order {
            id: self.clob_order_id.clone().unwrap_or_default(),
            token_id: self.token_id.clone(),
            side: self.side,
            price: self.price.to_f64().unwrap_or(0.0),
            size: self.size.to_f64().unwrap_or(0.0),
            order_type: self.order_type,
            post_only: true,
            timestamp_ms: self.created_at.timestamp_millis() as u64,
        }
    }
}

/// A completed trade record for audit logging (JSONL persistence).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    /// Internal trade ID
    pub id: Uuid,
    /// Order that generated this trade
    pub order_id: Uuid,
    /// Market condition ID
    pub condition_id: String,
    /// Asset traded
    pub asset: Asset,
    /// Buy or sell
    pub side: TradeSide,
    /// Execution price
    pub price: Decimal,
    /// Executed size
    pub size: Decimal,
    /// Fee paid (should be 0 for maker, negative for rebates)
    pub fee: Decimal,
    /// Net PnL from this trade (realized)
    pub pnl: Decimal,
    /// Was this a maker fill?
    pub is_maker: bool,
    /// Execution timestamp
    pub executed_at: DateTime<Utc>,
}

/// An open position in a market.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    /// Market condition ID
    pub condition_id: String,
    /// Token ID held
    pub token_id: String,
    /// Asset
    pub asset: Asset,
    /// Number of contracts held (positive = long, negative = short)
    pub size: Decimal,
    /// Average entry price
    pub avg_entry_price: Decimal,
    /// Unrealized PnL at current market price
    pub unrealized_pnl: Decimal,
    /// When position was opened
    pub opened_at: DateTime<Utc>,
    /// Whether the market has resolved
    pub resolved: bool,
}

/// A Polymarket prediction market instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Market {
    /// Unique condition ID from Polymarket
    pub condition_id: String,
    /// Token ID for the YES outcome
    pub token_id_yes: String,
    /// Token ID for the NO outcome
    pub token_id_no: String,
    /// Asset this market tracks
    pub asset: Asset,
    /// Market question
    pub question: String,
    /// Resolution timestamp
    pub end_time: DateTime<Utc>,
    /// Whether the market is currently active
    pub active: bool,
}

/// A price tick from an external feed (Binance, Coinbase).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceTick {
    /// Asset this tick is for
    pub asset: Asset,
    /// Current price in USD
    pub price: Decimal,
    /// Timestamp of the tick
    pub timestamp: DateTime<Utc>,
    /// Source feed name
    pub source: String,
}

/// Snapshot of the Polymarket order book for a market.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookSnapshot {
    /// Market condition ID
    pub condition_id: String,
    /// Token ID
    pub token_id: String,
    /// Best bid price
    pub best_bid: Option<Decimal>,
    /// Best ask price
    pub best_ask: Option<Decimal>,
    /// Spread (ask - bid)
    pub spread: Option<Decimal>,
    /// Timestamp of snapshot
    pub timestamp: DateTime<Utc>,
}

impl OrderBookSnapshot {
    /// Calculates the mid price if both bid and ask exist.
    pub fn mid_price(&self) -> Option<Decimal> {
        match (self.best_bid, self.best_ask) {
            (Some(bid), Some(ask)) => Some((bid + ask) / Decimal::TWO),
            _ => None,
        }
    }
}

/// Bot operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BotMode {
    /// Simulated trading (no real orders)
    Paper,
    /// Real money trading
    Live,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_order_new_maker_defaults() {
        let order = Order::new_maker(
            "token_yes".to_string(),
            TradeSide::Buy,
            0.45,
            10.0,
        );
        assert_eq!(order.order_type, OrderType::Gtc);
        assert!(order.post_only);
        assert_eq!(order.price, 0.45);
        assert!(order.id.is_empty());
    }

    #[test]
    fn test_rich_order_new_maker_defaults() {
        let order = RichOrder::new_maker(
            "condition_123".to_string(),
            "token_yes".to_string(),
            TradeSide::Buy,
            dec!(0.45),
            dec!(10.0),
            Asset::BTC,
        );
        assert_eq!(order.status, OrderStatus::Pending);
        assert_eq!(order.order_type, OrderType::Gtc);
        assert!(order.clob_order_id.is_none());
    }

    #[test]
    fn test_rich_to_boundary() {
        let rich = RichOrder::new_maker(
            "cond".to_string(),
            "tok".to_string(),
            TradeSide::Sell,
            dec!(0.60),
            dec!(5.0),
            Asset::ETH,
        );
        let boundary = rich.to_boundary_order();
        assert_eq!(boundary.token_id, "tok");
        assert_eq!(boundary.side, TradeSide::Sell);
        assert!((boundary.price - 0.60).abs() < 0.001);
    }

    #[test]
    fn test_orderbook_mid_price() {
        let ob = OrderBookSnapshot {
            condition_id: "test".to_string(),
            token_id: "token".to_string(),
            best_bid: Some(dec!(0.40)),
            best_ask: Some(dec!(0.50)),
            spread: Some(dec!(0.10)),
            timestamp: Utc::now(),
        };
        assert_eq!(ob.mid_price(), Some(dec!(0.45)));
    }

    #[test]
    fn test_orderbook_mid_price_no_bid() {
        let ob = OrderBookSnapshot {
            condition_id: "test".to_string(),
            token_id: "token".to_string(),
            best_bid: None,
            best_ask: Some(dec!(0.50)),
            spread: None,
            timestamp: Utc::now(),
        };
        assert_eq!(ob.mid_price(), None);
    }

    #[test]
    fn test_asset_display() {
        assert_eq!(format!("{}", Asset::BTC), "BTC");
        assert_eq!(format!("{}", Asset::ETH), "ETH");
    }

    #[test]
    fn test_trade_side_display() {
        assert_eq!(format!("{}", TradeSide::Buy), "BUY");
        assert_eq!(format!("{}", TradeSide::Sell), "SELL");
    }
}
