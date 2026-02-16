//! Chain Client Port - On-chain Interaction Interface
//!
//! Defines the trait for interacting with the Polygon blockchain
//! for CTF (Conditional Token Framework) operations like batch
//! redemption and balance queries. Uses alloy-rs.

use async_trait::async_trait;

use crate::domain::trade::TokenId;

/// On-chain token balance information.
#[derive(Debug, Clone)]
pub struct TokenBalance {
  /// Token identifier.
  pub token_id: TokenId,
  /// Balance in atomic units.
  pub balance_raw: u128,
  /// Balance in human-readable units (divided by decimals).
  pub balance: f64,
}

/// Result of a batch redemption operation.
#[derive(Debug, Clone)]
pub struct RedemptionResult {
  /// Transaction hash.
  pub tx_hash: String,
  /// Number of positions redeemed.
  pub positions_redeemed: usize,
  /// Total USDC recovered.
  pub usdc_recovered: f64,
  /// Gas cost in MATIC.
  pub gas_cost_matic: f64,
}

/// Trait for on-chain interactions via alloy-rs.
///
/// Handles CTF contract calls for position management
/// and batch redemption of resolved markets.
#[async_trait]
pub trait ChainClient: Send + Sync + 'static {
  /// Get the USDC balance of the bot's wallet.
  async fn usdc_balance(&self) -> anyhow::Result<f64>;

  /// Get the CTF token balance for a specific outcome token.
  async fn token_balance(&self, token_id: &TokenId) -> anyhow::Result<TokenBalance>;

  /// Batch redeem resolved positions for USDC.
  ///
  /// Automatically detects resolved markets and redeems
  /// winning positions. Optimized for gas efficiency by
  /// batching multiple redemptions into a single transaction.
  async fn batch_redeem(&self, token_ids: &[TokenId]) -> anyhow::Result<RedemptionResult>;

  /// Check if a market's condition has been resolved.
  async fn is_condition_resolved(&self, condition_id: &str) -> anyhow::Result<bool>;

  /// Get the current gas price on Polygon.
  async fn gas_price_gwei(&self) -> anyhow::Result<f64>;

  /// Check if the chain client connection is healthy.
  async fn is_healthy(&self) -> bool;
}
