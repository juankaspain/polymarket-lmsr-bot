//! Configuration module — TOML-based bot configuration.
//!
//! All configuration comes from `config.toml` (never hardcoded).
//! Secrets come from environment variables (never in config files).

pub mod loader;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::domain::trade::{Asset, BotMode};

/// Top-level application configuration loaded from `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Bot identity and operating mode.
    pub bot: BotConfig,
    /// API and RPC connection settings.
    pub api: ApiConfig,
    /// LMSR model parameters.
    pub lmsr: LmsrConfig,
    /// Risk management parameters.
    pub risk: RiskConfig,
    /// Rate limiting parameters.
    pub rate_limits: RateLimitConfig,
    /// Contract addresses (loaded from config, validated on-chain).
    pub contracts: ContractConfig,
    /// Active market definitions.
    pub markets: Vec<MarketConfig>,
    /// Strategy parameters (multi-asset).
    pub strategy: StrategyConfig,
}

/// Bot identity and operational settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    /// Human-readable bot name for logging.
    pub name: String,
    /// Log level filter (e.g. "info", "debug").
    pub log_level: String,
    /// If true, simulate trades without real execution.
    pub dry_run: bool,
    /// Operating mode: Paper or Live.
    pub mode: BotMode,
}

/// Strategy configuration for multi-asset trading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// Assets to trade (BTC, ETH).
    pub assets: Vec<Asset>,
    /// Debounce interval in milliseconds (checklist: 1000ms).
    pub debounce_ms: u64,
    /// Minimum price delta to act on (checklist: 0.5%).
    pub min_delta_pct: f64,
}

/// API endpoint configuration (URLs from config, secrets from env).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    /// CLOB REST API base URL.
    pub clob_base_url: String,
    /// CLOB WebSocket URL.
    pub clob_ws_url: String,
    /// Polygon RPC URL.
    pub rpc_url: String,
    /// Request timeout in milliseconds.
    pub timeout_ms: u64,
}

/// LMSR model and pricing parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LmsrConfig {
    /// Liquidity parameter (b > 0).
    pub liquidity_parameter: f64,
    /// Kelly fraction (0.25 = quarter-Kelly).
    pub kelly_fraction: f64,
    /// Minimum edge (%) to trigger a trade.
    pub min_edge: f64,
    /// Bayesian EWMA prior weight (alpha).
    pub prior_weight: Decimal,
}

/// Risk management configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    /// Maximum daily loss as fraction of bankroll.
    pub max_daily_loss_fraction: f64,
    /// Maximum position size in USDC per market.
    pub max_position_size: f64,
    /// Maximum total exposure in USDC.
    pub max_total_exposure: f64,
    /// Minimum bankroll to continue trading.
    pub min_bankroll: f64,
    /// Consecutive losses to trigger circuit breaker.
    pub circuit_breaker_losses: u32,
    /// Cooldown period in seconds after circuit breaker.
    pub cooldown_seconds: u64,
}

/// Rate limiting configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum orders per minute (checklist: 50, hard limit 60).
    pub max_orders_per_minute: u32,
    /// Maximum orders per batch request (checklist: 15).
    pub max_orders_per_batch: u32,
    /// Minimum interval between orders in milliseconds.
    pub min_interval_ms: u64,
}

/// On-chain contract addresses (validated at startup).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractConfig {
    /// CTF Exchange contract address.
    pub ctf_exchange: String,
    /// USDCe token contract address.
    pub usdce: String,
    /// Neg Risk Adapter contract address.
    pub neg_risk_adapter: String,
}

/// Individual market configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketConfig {
    /// Unique condition ID from Polymarket.
    pub condition_id: String,
    /// YES outcome token ID.
    pub yes_token_id: String,
    /// NO outcome token ID.
    pub no_token_id: String,
    /// Associated asset (BTC or ETH).
    pub asset: Asset,
    /// Whether this market is actively traded.
    pub active: bool,
}
