//! Configuration module — TOML-based bot configuration.
//!
//! All configuration comes from `config.toml` (never hardcoded).
//! Secrets come from environment variables (never in config files).

pub mod hot_reload;
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
    /// Wallet allocation parameters (checklist: hot 20%, cold 80%).
    #[serde(default)]
    pub wallet: WalletConfig,
    /// Settlement parameters (batch redeem timing).
    #[serde(default)]
    pub settlement: SettlementConfig,
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

/// Wallet allocation parameters (checklist: hot 20%, cold 80%).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletConfig {
    /// Hot wallet allocation fraction (default 0.20).
    #[serde(default = "default_hot_fraction")]
    pub hot_fraction: f64,
    /// Minimum MATIC balance for gas (default 0.5).
    #[serde(default = "default_min_matic")]
    pub min_matic_balance: f64,
    /// Alert threshold: warn if hot wallet exceeds this fraction.
    #[serde(default = "default_hot_alert")]
    pub hot_alert_threshold: f64,
}

impl Default for WalletConfig {
    fn default() -> Self {
        Self {
            hot_fraction: 0.20,
            min_matic_balance: 0.5,
            hot_alert_threshold: 0.30,
        }
    }
}

fn default_hot_fraction() -> f64 { 0.20 }
fn default_min_matic() -> f64 { 0.5 }
fn default_hot_alert() -> f64 { 0.30 }

/// Settlement parameters for batch redemption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementConfig {
    /// Hour (UTC) to run batch redemption (default 4 = 4 AM).
    #[serde(default = "default_redeem_hour")]
    pub batch_redeem_hour_utc: u32,
    /// Maximum gas price (gwei) for redemption (default 35).
    #[serde(default = "default_max_gas")]
    pub max_gas_gwei: f64,
    /// EIP-1559 priority fee tip (gwei, default 30).
    #[serde(default = "default_tip")]
    pub tip_gwei: f64,
    /// EIP-1559 max fee cap (gwei, default 50).
    #[serde(default = "default_max_fee")]
    pub max_fee_gwei: f64,
}

impl Default for SettlementConfig {
    fn default() -> Self {
        Self {
            batch_redeem_hour_utc: 4,
            max_gas_gwei: 35.0,
            tip_gwei: 30.0,
            max_fee_gwei: 50.0,
        }
    }
}

fn default_redeem_hour() -> u32 { 4 }
fn default_max_gas() -> f64 { 35.0 }
fn default_tip() -> f64 { 30.0 }
fn default_max_fee() -> f64 { 50.0 }
