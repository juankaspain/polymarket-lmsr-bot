//! Repository Implementation — Concrete Adapter for the Repository Port
//!
//! Wraps `StateStore` (atomic JSON snapshots) and `TradeLogger` (JSONL
//! append-only files) into a single struct that implements the
//! `Repository` trait from `crate::ports::repository`.
//!
//! This is the hexagonal architecture glue: the domain/usecases layer
//! only knows about the `Repository` trait, never about files or JSON.

use anyhow::Result;
use async_trait::async_trait;

use super::state::StateStore;
use super::trades::TradeLogger;
use crate::ports::repository::{
    BotStateSnapshot, DailyPnl, Repository, TradeRecord,
};

/// Concrete repository adapter combining state and trade persistence.
///
/// Delegates to `StateStore` for crash-recovery snapshots and
/// `TradeLogger` for append-only trade/PnL records.
pub struct RepositoryImpl {
    /// Atomic JSON state store.
    state_store: StateStore,
    /// JSONL trade logger.
    trade_logger: TradeLogger,
}

impl RepositoryImpl {
    /// Create a new repository from existing store and logger instances.
    pub fn new(state_store: StateStore, trade_logger: TradeLogger) -> Self {
        Self {
            state_store,
            trade_logger,
        }
    }

    /// Create a new repository with a data directory path.
    ///
    /// Initializes both the state store and trade logger in the
    /// given directory, creating subdirectories as needed.
    pub async fn from_data_dir(data_dir: &str) -> Result<Self> {
        let state_store = StateStore::new(data_dir).await?;
        let trade_logger = TradeLogger::new(data_dir).await?;
        Ok(Self::new(state_store, trade_logger))
    }
}

#[async_trait]
impl Repository for RepositoryImpl {
    async fn save_trade(&self, record: &TradeRecord) -> Result<()> {
        self.trade_logger.append_trade(record).await
    }

    async fn load_trades(&self) -> Result<Vec<TradeRecord>> {
        self.trade_logger.load_all_trades().await
    }

    async fn load_trades_range(
        &self,
        from_ms: u64,
        to_ms: u64,
    ) -> Result<Vec<TradeRecord>> {
        self.trade_logger.load_trades_range(from_ms, to_ms).await
    }

    async fn save_state(&self, state: &BotStateSnapshot) -> Result<()> {
        self.state_store.save(state).await
    }

    async fn load_latest_state(&self) -> Result<Option<BotStateSnapshot>> {
        self.state_store.load().await
    }

    async fn save_daily_pnl(&self, pnl: &DailyPnl) -> Result<()> {
        self.trade_logger.save_daily_pnl(pnl).await
    }

    async fn load_daily_pnl(&self) -> Result<Vec<DailyPnl>> {
        self.trade_logger.load_daily_pnl().await
    }

    async fn is_healthy(&self) -> bool {
        self.state_store.is_healthy().await
            && self.trade_logger.is_healthy().await
    }
}
