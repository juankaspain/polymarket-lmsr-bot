//! Ports Layer - Hexagonal Architecture Boundaries
//!
//! Defines the interfaces (traits) that the domain/usecases layer
//! requires from the outside world. Adapters implement these traits.
//!
//! Port categories:
//! - `MarketFeed`: Real-time market data streaming
//! - `OrderExecution`: Order placement and management via CLOB
//! - `Repository`: State persistence (JSONL-based)

pub mod execution;
pub mod market_feed;
pub mod repository;
