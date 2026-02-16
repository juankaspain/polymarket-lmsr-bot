//! Configuration Module - TOML-based Bot Configuration
//!
//! Loads and validates configuration from `config.toml` with
//! environment variable overrides via `.env` files.
//! All contract addresses and market parameters are externalized
//! here - nothing is hardcoded in the domain layer.

pub mod loader;

use serde::Deserialize;

/// Top-level bot configuration.
///
/// Loaded from `config.toml` at startup. All fields are validated
/// before the bot begins operation.
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
  /// Bot identity and metadata.
  pub bot: BotConfig,
  /// Market definitions and token mappings.
  pub markets: Vec<MarketConfig>,
  /// LMSR pricing model parameters.
  pub lmsr: LmsrConfig,
  /// Risk management parameters.
  pub risk: RiskConfig,
  /// Rate limiting configuration.
  pub rate_limits: RateLimitConfig,
  /// Polymarket API endpoints.
  pub api: ApiConfig,
  /// Metrics and monitoring.
  pub metrics: MetricsConfig,
  /// Persistence configuration.
  pub persistence: PersistenceConfig,
}

/// Bot identity configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct BotConfig {
  /// Human-readable bot name.
  pub name: String,
  /// Log level (trace, debug, info, warn, error).
  #[serde(default = "default_log_level")]
  pub log_level: String,
  /// Enable dry-run mode (no real orders).
  #[serde(default)]
  pub dry_run: bool,
}

/// Individual market configuration.
///
/// Each market maps a condition ID to its YES/NO token IDs.
/// Contract addresses are ALWAYS in config - never hardcoded.
#[derive(Debug, Clone, Deserialize)]
pub struct MarketConfig {
  /// Human-readable market name.
  pub name: String,
  /// Polymarket condition ID.
  pub condition_id: String,
  /// YES outcome token ID.
  pub yes_token_id: String,
  /// NO outcome token ID.
  pub no_token_id: String,
  /// Whether this market is actively traded.
  #[serde(default = "default_true")]
  pub active: bool,
  /// Market-specific LMSR liquidity override.
  pub liquidity_override: Option<f64>,
}

/// LMSR pricing model configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct LmsrConfig {
  /// Liquidity parameter (b). Higher = tighter spreads.
  pub liquidity_parameter: f64,
  /// Minimum edge required to place an order (after fees).
  pub min_edge: f64,
  /// Maximum spread in basis points.
  pub max_spread_bps: f64,
  /// Kelly fraction multiplier (0.25 = quarter-Kelly).
  #[serde(default = "default_kelly_fraction")]
  pub kelly_fraction: f64,
  /// Bayesian prior weight for probability estimation.
  #[serde(default = "default_prior_weight")]
  pub prior_weight: f64,
}

/// Risk management configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct RiskConfig {
  /// Maximum daily loss as fraction of bankroll (e.g., 0.02 = 2%).
  pub max_daily_loss_fraction: f64,
  /// Maximum position size per market in USDC.
  pub max_position_size: f64,
  /// Maximum total exposure across all markets.
  pub max_total_exposure: f64,
  /// Minimum bankroll to continue trading.
  pub min_bankroll: f64,
  /// Circuit breaker: consecutive losses before pause.
  #[serde(default = "default_circuit_breaker")]
  pub circuit_breaker_losses: u32,
  /// Cool-down period after circuit breaker (seconds).
  #[serde(default = "default_cooldown")]
  pub cooldown_seconds: u64,
}

/// Rate limiting configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
  /// Maximum orders per minute (Polymarket limit: 50).
  #[serde(default = "default_max_orders")]
  pub max_orders_per_minute: u32,
  /// Minimum interval between API calls (milliseconds).
  #[serde(default = "default_min_interval")]
  pub min_interval_ms: u64,
}

/// API endpoint configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiConfig {
  /// CLOB REST API base URL.
  pub clob_url: String,
  /// WebSocket feed URL.
  pub ws_url: String,
  /// Polygon RPC endpoint.
  pub rpc_url: String,
  /// Request timeout in seconds.
  #[serde(default = "default_timeout")]
  pub timeout_seconds: u64,
}

/// Metrics and monitoring configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct MetricsConfig {
  /// Enable Prometheus metrics export.
  #[serde(default = "default_true")]
  pub enabled: bool,
  /// Metrics server bind address.
  #[serde(default = "default_metrics_addr")]
  pub bind_address: String,
  /// Health check endpoint port.
  #[serde(default = "default_health_port")]
  pub health_port: u16,
}

/// Persistence configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct PersistenceConfig {
  /// Directory for JSONL trade logs.
  #[serde(default = "default_data_dir")]
  pub data_dir: String,
  /// State snapshot interval (seconds).
  #[serde(default = "default_snapshot_interval")]
  pub snapshot_interval_seconds: u64,
  /// Maximum log file size before rotation (bytes).
  #[serde(default = "default_max_log_size")]
  pub max_log_size_bytes: u64,
}

// Default value functions for serde

fn default_log_level() -> String {
  "info".to_string()
}

fn default_true() -> bool {
  true
}

fn default_kelly_fraction() -> f64 {
  0.25
}

fn default_prior_weight() -> f64 {
  0.5
}

fn default_circuit_breaker() -> u32 {
  5
}

fn default_cooldown() -> u64 {
  300
}

fn default_max_orders() -> u32 {
  50
}

fn default_min_interval() -> u64 {
  100
}

fn default_timeout() -> u64 {
  30
}

fn default_metrics_addr() -> String {
  "0.0.0.0:9090".to_string()
}

fn default_health_port() -> u16 {
  8080
}

fn default_data_dir() -> String {
  "data".to_string()
}

fn default_snapshot_interval() -> u64 {
  60
}

fn default_max_log_size() -> u64 {
  10_485_760 // 10 MB
}
