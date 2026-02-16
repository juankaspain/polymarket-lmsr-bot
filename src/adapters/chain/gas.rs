//! Gas Oracle - EIP-1559 Fee Estimation for Polygon
//!
//! Monitors gas prices on Polygon to optimize on-chain transaction
//! timing. Batch redemptions are only executed when gas < 35 gwei.
//! Uses EIP-1559 with priority fee (tip) of 30 gwei and max fee of 50 gwei.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use alloy::providers::Provider;
use anyhow::{Context, Result};
use tracing::{debug, instrument};

use super::provider::PolygonProvider;

/// EIP-1559 gas parameters for Polygon transactions.
#[derive(Debug, Clone, Copy)]
pub struct GasParams {
    /// Base fee from the latest block (gwei).
    pub base_fee_gwei: f64,
    /// Priority fee / tip (default 30 gwei per checklist).
    pub priority_fee_gwei: f64,
    /// Maximum fee per gas (default 50 gwei per checklist).
    pub max_fee_gwei: f64,
}

impl Default for GasParams {
    fn default() -> Self {
        Self {
            base_fee_gwei: 30.0,
            priority_fee_gwei: 30.0,
            max_fee_gwei: 50.0,
        }
    }
}

/// Gas price oracle for Polygon EIP-1559 transactions.
///
/// Provides real-time gas estimates and enforces the 35 gwei
/// threshold for batch redemption timing (scheduled @4AM UTC).
pub struct GasOracle {
    /// Shared Polygon provider.
    provider: Arc<PolygonProvider>,
    /// Cached gas price in gwei × 100 (for atomic integer ops).
    cached_gas_x100: AtomicU64,
    /// Gas price threshold for batch operations (gwei).
    redeem_threshold_gwei: f64,
}

impl GasOracle {
    /// Create a new gas oracle with default 35 gwei threshold.
    pub fn new(provider: Arc<PolygonProvider>) -> Self {
        Self {
            provider,
            cached_gas_x100: AtomicU64::new(3000), // 30.0 gwei default
            redeem_threshold_gwei: 35.0,
        }
    }

    /// Get the current gas price in gwei from the RPC node.
    #[instrument(skip(self))]
    pub async fn current_gas_gwei(&self) -> Result<f64> {
        let inner = self.provider.inner();

        let gas_price = inner
            .get_gas_price()
            .await
            .context("Failed to query gas price")?;

        // Convert wei to gwei (1 gwei = 1e9 wei)
        let gwei = gas_price as f64 / 1_000_000_000.0;

        // Cache for quick access
        self.cached_gas_x100
            .store((gwei * 100.0) as u64, Ordering::Relaxed);

        debug!(gas_gwei = gwei, "Gas price updated");
        Ok(gwei)
    }

    /// Get cached gas price without RPC call (fast path).
    pub fn cached_gas_gwei(&self) -> f64 {
        self.cached_gas_x100.load(Ordering::Relaxed) as f64 / 100.0
    }

    /// Check if gas is low enough for batch redemption.
    pub async fn is_gas_acceptable_for_redeem(&self) -> Result<bool> {
        let gwei = self.current_gas_gwei().await?;
        Ok(gwei <= self.redeem_threshold_gwei)
    }

    /// Get EIP-1559 gas parameters for a transaction.
    ///
    /// Uses priority fee of 30 gwei and max fee of 50 gwei
    /// per the Space checklist requirements.
    pub async fn eip1559_params(&self) -> Result<GasParams> {
        let base_fee = self.current_gas_gwei().await?;

        Ok(GasParams {
            base_fee_gwei: base_fee,
            priority_fee_gwei: 30.0, // Checklist: tip 30 gwei
            max_fee_gwei: 50.0,       // Checklist: max 50 gwei
        })
    }
}
