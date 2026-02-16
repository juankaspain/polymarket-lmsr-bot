//! Adapters Layer - Hexagonal Architecture Outer Ring
//!
//! Implements the port traits defined in `crate::ports` with concrete
//! external dependencies (HTTP clients, WebSockets, blockchain RPC,
//! file I/O). Each sub-module groups adapters by infrastructure concern.
//!
//! Adapter categories:
//! - `api`: Polymarket CLOB REST API client and auth
//! - `chain`: Polygon blockchain interaction via alloy-rs
//! - `feeds`: Real-time market data (Binance, Coinbase WebSockets)
//! - `metrics`: Prometheus metrics export and health checks
//! - `persistence`: JSONL trade logging and state snapshots

pub mod api;
pub mod chain;
pub mod feeds;
pub mod metrics;
pub mod persistence;
