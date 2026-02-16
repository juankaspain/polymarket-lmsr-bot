//! Ports Layer - Hexagonal Architecture Boundaries
//!
//! Defines the interfaces (traits) that the domain/usecases layer
//! requires from the outside world. Adapters implement these traits.
//!
//! Port categories:
//! - `MarketFeed`: Real-time market data streaming
//! - `OrderExecution`: Order placement and management via CLOB
//! - `ChainClient`: On-chain CTF operations (batch redeem)
//! - `Repository`: State persistence (JSONL-based)
//! - `OrderExecutor`: High-level quoting orchestration

pub mod chain_client;
pub mod execution;
pub mod market_feed;
pub mod order_executor;
pub mod repository;
