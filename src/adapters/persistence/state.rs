//! State Store - Atomic JSON Bot State Persistence
//!
//! Saves bot state snapshots to `state.json` using atomic writes
//! (write to tmp file, then rename). This guarantees crash safety
//! and prevents partial writes from corrupting state.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tokio::fs;
use tracing::{info, instrument, warn};

use crate::ports::repository::BotStateSnapshot;

/// Atomic JSON state store for crash recovery.
///
/// State is written to a temporary file first, then atomically
/// renamed to `state.json`. This ensures the file is always
/// either the old or new version, never a partial write.
pub struct StateStore {
    /// Path to state.json.
    state_path: PathBuf,
    /// Temporary path for atomic writes.
    tmp_path: PathBuf,
}

impl StateStore {
    /// Create a new state store in the given data directory.
    ///
    /// Creates the directory if it doesn't exist.
    pub async fn new(data_dir: &str) -> Result<Self> {
        let dir = Path::new(data_dir);
        fs::create_dir_all(dir)
            .await
            .context("Failed to create data directory")?;

        Ok(Self {
            state_path: dir.join("state.json"),
            tmp_path: dir.join("state.json.tmp"),
        })
    }

    /// Save a state snapshot atomically (tmp → rename).
    ///
    /// Serializes the snapshot to JSON, writes to a temp file,
    /// then renames to the final path. This guarantees crash safety.
    #[instrument(skip(self, state))]
    pub async fn save(&self, state: &BotStateSnapshot) -> Result<()> {
        let json = serde_json::to_string_pretty(state)
            .context("Failed to serialize state")?;

        // Write to tmp file
        fs::write(&self.tmp_path, &json)
            .await
            .context("Failed to write tmp state file")?;

        // Atomic rename
        fs::rename(&self.tmp_path, &self.state_path)
            .await
            .context("Failed to rename state file")?;

        info!(
            path = %self.state_path.display(),
            version = %state.version,
            "State snapshot saved"
        );

        Ok(())
    }

    /// Load the most recent state snapshot.
    ///
    /// Returns `None` if no state file exists (first startup).
    #[instrument(skip(self))]
    pub async fn load(&self) -> Result<Option<BotStateSnapshot>> {
        if !self.state_path.exists() {
            info!("No state file found, starting fresh");
            return Ok(None);
        }

        let json = fs::read_to_string(&self.state_path)
            .await
            .context("Failed to read state file")?;

        let state: BotStateSnapshot =
            serde_json::from_str(&json).context("Failed to parse state JSON")?;

        info!(
            version = %state.version,
            open_orders = state.open_orders.len(),
            "State snapshot loaded"
        );

        Ok(Some(state))
    }

    /// Check if the state file exists and is readable.
    pub async fn is_healthy(&self) -> bool {
        if !self.state_path.exists() {
            return true; // First run is OK
        }
        fs::metadata(&self.state_path).await.is_ok()
    }
}
