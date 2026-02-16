//! Market Data Feed Adapters - Real-time Price Streaming
//!
//! Provides WebSocket-based price feeds from:
//! - Polymarket: Primary CLOB order book feed (implements MarketFeed port)
//! - Binance: External BTC/ETH spot price oracle
//! - Coinbase: Secondary feed for price cross-validation
//! - Bridge: Converts BinanceTick → PriceUpdate for cross-validation
//! - Task Supervisor: Manages feed lifecycle with auto-reconnect

pub mod binance;
pub mod bridge;
pub mod coinbase;
pub mod polymarket_ws;
pub mod task_supervisor;

pub use binance::BinanceFeed;
pub use bridge::FeedBridge;
pub use coinbase::CoinbaseFeed;
pub use polymarket_ws::PolymarketFeed;
pub use task_supervisor::FeedSupervisor;
