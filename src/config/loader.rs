//! Configuration loader — reads and validates `config.toml`.
//!
//! Loads the TOML configuration file from the given path and
//! deserializes it into `AppConfig`. Validates critical fields
//! and merges with environment variable overrides.

use anyhow::{Context, Result};
use tracing::info;

use super::AppConfig;

/// Load and validate configuration from a TOML file.
///
/// Reads the file at `path`, deserializes into `AppConfig`,
/// and performs basic validation (non-empty URLs, positive params).
pub fn load_config(path: &str) -> Result<AppConfig> {
    let content = std::fs::read_to_string(path)
        .context(format!("Failed to read config file: {path}"))?;

    let config: AppConfig =
        toml::from_str(&content).context("Failed to parse config.toml")?;

    validate_config(&config)?;

    info!(path = path, "Configuration loaded successfully");
    Ok(config)
}

/// Validate critical configuration fields.
fn validate_config(config: &AppConfig) -> Result<()> {
    anyhow::ensure!(
        !config.api.clob_base_url.is_empty(),
        "api.clob_base_url must not be empty"
    );
    anyhow::ensure!(
        !config.api.rpc_url.is_empty(),
        "api.rpc_url must not be empty"
    );
    anyhow::ensure!(
        config.lmsr.liquidity_parameter > 0.0,
        "lmsr.liquidity_parameter must be positive"
    );
    anyhow::ensure!(
        config.lmsr.kelly_fraction > 0.0 && config.lmsr.kelly_fraction <= 1.0,
        "lmsr.kelly_fraction must be in (0, 1]"
    );
    anyhow::ensure!(
        config.risk.max_daily_loss_fraction > 0.0
            && config.risk.max_daily_loss_fraction <= 1.0,
        "risk.max_daily_loss_fraction must be in (0, 1]"
    );
    anyhow::ensure!(
        !config.contracts.ctf_exchange.is_empty(),
        "contracts.ctf_exchange must not be empty"
    );
    anyhow::ensure!(
        !config.strategy.assets.is_empty(),
        "strategy.assets must contain at least one asset"
    );

    Ok(())
}
