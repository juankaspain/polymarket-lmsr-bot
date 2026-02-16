//! Config Hot-Reload — Watch config.toml for Changes Every 60s
//!
//! Periodically re-reads config.toml and compares with the current
//! config. If changes are detected, broadcasts the new config via
//! a `tokio::sync::watch` channel. Consumers can subscribe to
//! receive updated config without restarting the bot.
//!
//! Checklist: hot-reload 60s A/B testing.

use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::{broadcast, watch};
use tracing::{debug, info, instrument, warn};

use super::AppConfig;

/// Watches config.toml for changes and broadcasts updates.
///
/// Polls the config file every 60 seconds (not a filesystem watcher,
/// which has portability issues across Linux/macOS/Docker volumes).
/// Compares serialized TOML to detect meaningful changes.
pub struct ConfigWatcher {
    /// Path to config.toml.
    config_path: String,
    /// Watch channel sender for config updates.
    config_tx: watch::Sender<AppConfig>,
    /// Last known serialized config (for diff detection).
    last_hash: Option<u64>,
}

impl ConfigWatcher {
    /// Create a new config watcher.
    ///
    /// Returns the watcher and a watch::Receiver that consumers
    /// can use to get notified of config changes.
    pub fn new(
        config_path: &str,
        initial_config: AppConfig,
    ) -> (Self, watch::Receiver<AppConfig>) {
        let (config_tx, config_rx) = watch::channel(initial_config);

        let watcher = Self {
            config_path: config_path.to_string(),
            config_tx,
            last_hash: None,
        };

        (watcher, config_rx)
    }

    /// Run the config watcher loop.
    ///
    /// Checks config.toml every 60 seconds. On change, reloads
    /// and broadcasts the new config. Runs until shutdown.
    #[instrument(skip(self, shutdown_rx))]
    pub async fn run(
        &mut self,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<()> {
        info!(
            path = %self.config_path,
            "Config watcher started — checking every 60s"
        );

        // Compute initial hash
        self.last_hash = self.compute_hash().await;

        loop {
            tokio::select! {
                biased;
                _ = shutdown_rx.recv() => {
                    info!("Config watcher shutting down");
                    return Ok(());
                }
                _ = tokio::time::sleep(Duration::from_secs(60)) => {
                    self.check_and_reload().await;
                }
            }
        }
    }

    /// Check if config has changed and reload if so.
    async fn check_and_reload(&mut self) {
        let new_hash = self.compute_hash().await;

        if new_hash == self.last_hash {
            debug!("Config unchanged");
            return;
        }

        info!("Config change detected, reloading...");

        match super::loader::load_config(&self.config_path) {
            Ok(new_config) => {
                self.last_hash = new_hash;
                if self.config_tx.send(new_config).is_err() {
                    warn!("No config watchers — update dropped");
                } else {
                    info!("Config reloaded successfully");
                }
            }
            Err(e) => {
                warn!(
                    error = %e,
                    "Failed to reload config — keeping current"
                );
            }
        }
    }

    /// Compute a simple hash of the config file contents for diff detection.
    async fn compute_hash(&self) -> Option<u64> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let content = tokio::fs::read_to_string(&self.config_path)
            .await
            .ok()?;

        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        Some(hasher.finish())
    }
}
