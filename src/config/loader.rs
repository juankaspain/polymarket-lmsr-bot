//! Configuration Loader - File Loading and Validation
//!
//! Handles loading `config.toml`, validating all parameters,
//! and providing clear error messages for misconfiguration.

use std::path::Path;

use anyhow::{Context, Result};
use tracing::info;

use super::AppConfig;

/// Load and validate configuration from a TOML file.
///
/// # Arguments
/// * `path` - Path to the config.toml file
///
/// # Errors
/// Returns detailed error if:
/// - File doesn't exist or can't be read
/// - TOML parsing fails
/// - Validation rules are violated
pub fn load_config(path: &str) -> Result<AppConfig> {
  let path = Path::new(path);

  let content = std::fs::read_to_string(path)
    .with_context(|| format!("Failed to read config file: {}", path.display()))?;

  let config: AppConfig = toml::from_str(&content)
    .with_context(|| "Failed to parse config.toml")?;

  validate_config(&config)?;

  info!(
    markets = config.markets.len(),
    liquidity = config.lmsr.liquidity_parameter,
    kelly = config.lmsr.kelly_fraction,
    "Configuration loaded successfully"
  );

  Ok(config)
}

/// Validate all configuration parameters.
///
/// Checks for:
/// - Positive numeric values where required
/// - Valid probability ranges (0..1)
/// - Sensible risk limits
/// - Non-empty market definitions
fn validate_config(config: &AppConfig) -> Result<()> {
  // Market validation
  anyhow::ensure!(
    !config.markets.is_empty(),
    "At least one market must be configured"
  );

  for (i, market) in config.markets.iter().enumerate() {
    anyhow::ensure!(
      !market.condition_id.is_empty(),
      "Market {} ({}) has empty condition_id",
      i,
      market.name
    );
    anyhow::ensure!(
      !market.yes_token_id.is_empty(),
      "Market {} ({}) has empty yes_token_id",
      i,
      market.name
    );
    anyhow::ensure!(
      !market.no_token_id.is_empty(),
      "Market {} ({}) has empty no_token_id",
      i,
      market.name
    );
  }

  // LMSR validation
  anyhow::ensure!(
    config.lmsr.liquidity_parameter > 0.0,
    "LMSR liquidity_parameter must be positive, got {}",
    config.lmsr.liquidity_parameter
  );
  anyhow::ensure!(
    config.lmsr.min_edge >= 0.0 && config.lmsr.min_edge < 1.0,
    "LMSR min_edge must be in [0, 1), got {}",
    config.lmsr.min_edge
  );
  anyhow::ensure!(
    config.lmsr.kelly_fraction > 0.0 && config.lmsr.kelly_fraction <= 1.0,
    "Kelly fraction must be in (0, 1], got {}",
    config.lmsr.kelly_fraction
  );

  // Risk validation
  anyhow::ensure!(
    config.risk.max_daily_loss_fraction > 0.0
      && config.risk.max_daily_loss_fraction <= 1.0,
    "max_daily_loss_fraction must be in (0, 1], got {}",
    config.risk.max_daily_loss_fraction
  );
  anyhow::ensure!(
    config.risk.max_position_size > 0.0,
    "max_position_size must be positive"
  );
  anyhow::ensure!(
    config.risk.min_bankroll > 0.0,
    "min_bankroll must be positive"
  );

  // Rate limit validation
  anyhow::ensure!(
    config.rate_limits.max_orders_per_minute > 0
      && config.rate_limits.max_orders_per_minute <= 50,
    "max_orders_per_minute must be in (0, 50], got {}",
    config.rate_limits.max_orders_per_minute
  );

  // API validation
  anyhow::ensure!(
    !config.api.clob_url.is_empty(),
    "CLOB API URL must not be empty"
  );
  anyhow::ensure!(
    !config.api.ws_url.is_empty(),
    "WebSocket URL must not be empty"
  );

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_load_nonexistent_file() {
    let result = load_config("nonexistent.toml");
    assert!(result.is_err());
  }
}
