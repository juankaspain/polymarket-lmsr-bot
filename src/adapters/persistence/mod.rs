//! Persistence Adapters - JSONL-based File Storage
//!
//! Implements the Repository port using append-only JSONL files
//! for trade logs and atomic JSON snapshots for bot state.
//! No database dependency — lightweight and crash-recoverable.

pub mod repository_impl;
pub mod state;
pub mod trades;

pub use repository_impl::RepositoryImpl;
pub use state::StateStore;
pub use trades::TradeLogger;
