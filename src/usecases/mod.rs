//! Use Cases Layer - Application Business Logic
//!
//! Orchestrates domain logic with port interfaces to implement
//! the bot's core workflows. Each use case is a self-contained
//! business operation.
//!
//! Use cases:
//! - `ArbitrageEngine`: Main pricing + quoting loop
//! - `OrderManager`: Order lifecycle management
//! - `RiskManager`: Position limits, circuit breakers, daily loss
//! - `Settlement`: Batch redemption of resolved markets
//! - `WalletManager`: Balance tracking and USDC management

pub mod arbitrage_engine;
pub mod order_manager;
pub mod risk_manager;
pub mod settlement;
pub mod wallet_manager;
