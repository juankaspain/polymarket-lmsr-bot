//! Core trading domain types.
//!
//! Defines all business entities: assets, markets, orders, trades, and positions.
//! These types are the foundation of the hexagonal architecture's inner ring.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    /// Market question (e.g., "Will BTC be above $50,000 at 16:05 UTC?")
    pub question: String,
    /// Resolution timestamp
    pub end_time: DateTime<Utc>,
    /// Whether the market is currently active
    pub active: bool,
}

/// Order side: buy or sell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

impl std::fmt::Display for OrderSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Buy => write!(f, "BUY"),
            Self::Sell => write!(f, "SELL"),
        }
    }
}

/// Order type configuration.
///
/// We use GTC (Good-Til-Cancelled) with post-only to guarantee maker status.
/// This ensures 0% fees + rebates per the maker-first strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    /// Good-til-cancelled, post-only (maker). Primary order type.
    GtcPostOnly,
    /// Good-til-date with expiration. Used for time-sensitive markets.
    Gtd { expiration_secs: u64 },
}

/// Lifecycle status of an order.
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

/// A trading order to be placed on Polymarket CLOB.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    /// Internal order ID
    pub id: Uuid,
    /// Market this order belongs to
    pub condition_id: String,
    /// Token ID (YES or NO outcome)
    pub token_id: String,
    /// Buy or sell
    pub side: OrderSide,
    /// Price in USDC (0.01 to 0.99 for prediction markets)
    pub price: Decimal,
    /// Size in contracts
    pub size: Decimal,
    /// Order type (GTC post-only for maker)
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

impl Order {
    /// Creates a new pending order with maker-first defaults.
    pub fn new_maker(
        condition_id: String,
        token_id: String,
        side: OrderSide,
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
            order_type: OrderType::GtcPostOnly,
            status: OrderStatus::Pending,
            clob_order_id: None,
            created_at: now,
            updated_at: now,
            asset,
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
    pub side: OrderSide,
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
            "condition_123".to_string(),
            "token_yes".to_string(),
            OrderSide::Buy,
            dec!(0.45),
            dec!(10.0),
            Asset::BTC,
        );

        assert_eq!(order.status, OrderStatus::Pending);
        assert_eq!(order.order_type, OrderType::GtcPostOnly);
        assert_eq!(order.side, OrderSide::Buy);
        assert_eq!(order.price, dec!(0.45));
        assert!(order.clob_order_id.is_none());
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
}
