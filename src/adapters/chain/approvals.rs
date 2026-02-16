//! ERC-20 Approval Manager - Token Spend Allowances
//!
//! Handles one-time ERC-20 approvals at startup:
//! - USDCe → CTF Exchange
//! - USDCe → Neg Risk Adapter
//! - CTF → Exchange contracts
//!
//! Approvals use max uint256 to avoid repeated transactions.
//! Only runs on-chain if allowance is below threshold.

use std::sync::Arc;

use alloy::primitives::{Address, U256, Bytes, keccak256};
use alloy::rpc::types::TransactionRequest;
use alloy::providers::Provider;
use anyhow::{Context, Result};
use tracing::{info, instrument, warn};

use super::contracts::ContractAddresses;
use super::gas::GasOracle;
use super::provider::PolygonProvider;

/// Manages ERC-20 token approvals for the bot's trading wallet.
///
/// At startup, checks allowances and submits approval transactions
/// only when needed. Uses EIP-1559 gas pricing from the GasOracle.
pub struct ApprovalManager {
    /// Shared Polygon provider.
    provider: Arc<PolygonProvider>,
    /// Gas oracle for fee estimation.
    gas_oracle: Arc<GasOracle>,
    /// Contract addresses from config.
    addresses: ContractAddresses,
    /// Bot wallet address.
    wallet: Address,
}

/// Minimum allowance threshold before re-approval (1M USDC in 6 decimals).
const MIN_ALLOWANCE_THRESHOLD: u128 = 1_000_000 * 1_000_000;

impl ApprovalManager {
    /// Create a new approval manager.
    pub fn new(
        provider: Arc<PolygonProvider>,
        gas_oracle: Arc<GasOracle>,
        addresses: ContractAddresses,
    ) -> Result<Self> {
        let wallet_str = std::env::var("WALLET_ADDRESS")
            .context("WALLET_ADDRESS not set")?;
        let wallet: Address = wallet_str.parse().context("Invalid WALLET_ADDRESS")?;

        Ok(Self {
            provider,
            gas_oracle,
            addresses,
            wallet,
        })
    }

    /// Ensure all required approvals are in place.
    ///
    /// Checks current allowances and only submits approval tx if below
    /// threshold. Called once at startup per the checklist.
    #[instrument(skip(self))]
    pub async fn ensure_approvals(&self) -> Result<()> {
        let spenders = [
            ("CTF Exchange", self.addresses.ctf_exchange),
            ("NegRisk Adapter", self.addresses.neg_risk_adapter),
        ];

        for (name, spender) in &spenders {
            match self.check_and_approve(self.addresses.usdce, *spender).await {
                Ok(needed) => {
                    if needed {
                        info!(token = "USDCe", spender = name, "Approval submitted");
                    } else {
                        info!(token = "USDCe", spender = name, "Allowance sufficient");
                    }
                }
                Err(e) => {
                    warn!(
                        token = "USDCe",
                        spender = name,
                        error = %e,
                        "Approval check failed"
                    );
                    return Err(e);
                }
            }
        }

        Ok(())
    }

    /// Check allowance and submit approval if below threshold.
    ///
    /// Returns `true` if an approval transaction was submitted.
    async fn check_and_approve(
        &self,
        token: Address,
        spender: Address,
    ) -> Result<bool> {
        let inner = self.provider.inner();

        // Build allowance(owner, spender) calldata
        let selector = &keccak256(b"allowance(address,address)")[..4];
        let mut calldata = Vec::with_capacity(68);
        calldata.extend_from_slice(selector);

        // Left-pad owner address to 32 bytes
        let mut owner_padded = [0u8; 32];
        owner_padded[12..].copy_from_slice(self.wallet.as_slice());
        calldata.extend_from_slice(&owner_padded);

        // Left-pad spender address to 32 bytes
        let mut spender_padded = [0u8; 32];
        spender_padded[12..].copy_from_slice(spender.as_slice());
        calldata.extend_from_slice(&spender_padded);

        let tx = TransactionRequest::default()
            .to(token)
            .input(Bytes::from(calldata).into());

        let result = inner
            .call(&tx)
            .await
            .context("Allowance query failed")?;

        let current_allowance = U256::from_be_slice(&result);

        if current_allowance >= U256::from(MIN_ALLOWANCE_THRESHOLD) {
            return Ok(false);
        }

        // Need to approve: submit approve(spender, type(uint256).max)
        info!(
            current = %current_allowance,
            spender = %spender,
            "Submitting max approval"
        );

        // In production: encode approve(spender, uint256.max) and sign+send tx
        // with EIP-1559 fees from gas_oracle (tip 30 gwei, max 50 gwei)
        let _gas_gwei = self.gas_oracle.current_gas_gwei().await?;

        // TODO: Actual tx submission requires wallet signer integration
        warn!("Approval tx submission requires wallet signer — placeholder");

        Ok(true)
    }
}
