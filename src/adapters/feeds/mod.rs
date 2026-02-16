//! Market Data Feed Adapters - Real-time Price Streaming
//!
//! Provides WebSocket-based price feeds from:
//! - Binance: Primary BTC/ETH spot price feed
//! - Coinbase: Secondary feed for price cross-validation
//! - Task Supervisor: Manages feed lifecycle with auto-reconnect

pub mod binance;
pub mod coinbase;
pub mod task_supervisor;

pub use binance::BinanceFeed;
pub use coinbase::CoinbaseFeed;
pub use task_supervisor::FeedSupervisor;
