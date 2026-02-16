//! Trade Logger - Append-only JSONL Trade Records
//!
//! Persists trade records to daily JSONL files in the format
//! `trades/YYYY-MM-DD.jsonl`. Each line is a self-contained JSON
//! record for easy parsing, streaming, and crash recovery.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tracing::{info, instrument};

use crate::ports::repository::{DailyPnl, TradeRecord};

/// Append-only JSONL trade logger with daily file rotation.
///
/// Trade files are named `trades/YYYY-MM-DD.jsonl` and each line
/// is a complete JSON object. This format is optimized for:
/// - Append-only writes (no read-modify-write)
/// - Line-by-line streaming for analysis
/// - Natural daily partitioning
pub struct TradeLogger {
    /// Base directory for trade files.
    trades_dir: PathBuf,
    /// Directory for PnL summaries.
    pnl_dir: PathBuf,
}

impl TradeLogger {
    /// Create a new trade logger in the given data directory.
    pub async fn new(data_dir: &str) -> Result<Self> {
        let trades_dir = Path::new(data_dir).join("trades");
        let pnl_dir = Path::new(data_dir).join("pnl");

        fs::create_dir_all(&trades_dir)
            .await
            .context("Failed to create trades directory")?;
        fs::create_dir_all(&pnl_dir)
            .await
            .context("Failed to create pnl directory")?;

        Ok(Self {
            trades_dir,
            pnl_dir,
        })
    }

    /// Append a trade record to today's JSONL file.
    #[instrument(skip(self, record), fields(trade_id = %record.id))]
    pub async fn append_trade(&self, record: &TradeRecord) -> Result<()> {
        let date = Utc::now().format("%Y-%m-%d").to_string();
        let path = self.trades_dir.join(format!("{date}.jsonl"));

        let mut json = serde_json::to_string(record)
            .context("Failed to serialize trade record")?;
        json.push('\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .context("Failed to open trade log file")?;

        file.write_all(json.as_bytes())
            .await
            .context("Failed to write trade record")?;

        file.flush().await.context("Failed to flush trade log")?;

        Ok(())
    }

    /// Load all trade records from all daily files.
    #[instrument(skip(self))]
    pub async fn load_all_trades(&self) -> Result<Vec<TradeRecord>> {
        let mut trades = Vec::new();
        let mut entries = fs::read_dir(&self.trades_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "jsonl") {
                let content = fs::read_to_string(&path).await?;
                for line in content.lines() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<TradeRecord>(line) {
                        Ok(record) => trades.push(record),
                        Err(e) => {
                            tracing::warn!(
                                file = %path.display(),
                                error = %e,
                                "Skipping malformed trade record"
                            );
                        }
                    }
                }
            }
        }

        trades.sort_by_key(|t| t.timestamp_ms);
        info!(count = trades.len(), "Loaded trade records");
        Ok(trades)
    }

    /// Load trades within a timestamp range (inclusive).
    pub async fn load_trades_range(
        &self,
        from_ms: u64,
        to_ms: u64,
    ) -> Result<Vec<TradeRecord>> {
        let all = self.load_all_trades().await?;
        Ok(all
            .into_iter()
            .filter(|t| t.timestamp_ms >= from_ms && t.timestamp_ms <= to_ms)
            .collect())
    }

    /// Save a daily PnL summary.
    #[instrument(skip(self, pnl), fields(date = %pnl.date))]
    pub async fn save_daily_pnl(&self, pnl: &DailyPnl) -> Result<()> {
        let path = self.pnl_dir.join("daily_pnl.jsonl");

        let mut json = serde_json::to_string(pnl)
            .context("Failed to serialize daily PnL")?;
        json.push('\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;

        file.write_all(json.as_bytes()).await?;
        file.flush().await?;

        Ok(())
    }

    /// Load all daily PnL records.
    pub async fn load_daily_pnl(&self) -> Result<Vec<DailyPnl>> {
        let path = self.pnl_dir.join("daily_pnl.jsonl");

        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&path).await?;
        let mut records = Vec::new();

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(pnl) = serde_json::from_str::<DailyPnl>(line) {
                records.push(pnl);
            }
        }

        Ok(records)
    }

    /// Check if the trades directory is writable.
    pub async fn is_healthy(&self) -> bool {
        let test_path = self.trades_dir.join(".health_check");
        let result = fs::write(&test_path, b"ok").await;
        let _ = fs::remove_file(&test_path).await;
        result.is_ok()
    }
}
