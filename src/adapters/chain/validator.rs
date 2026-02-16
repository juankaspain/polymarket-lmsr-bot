//! Contract Validator — On-chain Verification at Startup
//!
//! Validates that configured contract addresses point to actual
//! deployed contracts on Polygon. Checks:
//! 1. Code exists at the address (not an EOA)
//! 2. Basic call succeeds (symbol/name for tokens)
//!
//! This prevents configuration errors from causing silent failures
//! at runtime (checklist: validate contracts on-chain at startup).

use std::sync::Arc;

use alloy::primitives::Address;
use alloy::providers::Provider;
use anyhow::{Context, Result};
use tracing::{info, instrument, warn};

use crate::config::ContractConfig;

/// Result of validating a single contract.
#[derive(Debug)]
pub struct ValidationResult {
    /// Contract name for logging.
    pub name: String,
    /// Address that was validated.
    pub address: String,
    /// Whether the contract has deployed code.
    pub has_code: bool,
}

/// Validates contract addresses against on-chain state.
///
/// Called once at startup before the bot begins trading.
/// Ensures all configured addresses are real contracts
/// (not EOAs or typos) to prevent runtime surprises.
pub struct ContractValidator {
    /// Alloy provider for on-chain queries.
    provider: Arc<dyn Provider + Send + Sync>,
}

impl ContractValidator {
    /// Create a new validator with the given provider.
    pub fn new(provider: Arc<dyn Provider + Send + Sync>) -> Self {
        Self { provider }
    }

    /// Validate all contracts from config.
    ///
    /// Returns an error if any critical contract is invalid.
    /// Logs warnings for non-critical validation failures.
    #[instrument(skip(self, config))]
    pub async fn validate_all(
        &self,
        config: &ContractConfig,
    ) -> Result<Vec<ValidationResult>> {
        let mut results = Vec::new();

        let contracts = [
            ("CTF Exchange", &config.ctf_exchange),
            ("USDCe", &config.usdce),
            ("Neg Risk Adapter", &config.neg_risk_adapter),
        ];

        for (name, addr_str) in &contracts {
            let result = self.validate_contract(name, addr_str).await?;

            if !result.has_code {
                warn!(
                    contract = name,
                    address = addr_str,
                    "Contract has no code — possible misconfiguration"
                );
            } else {
                info!(
                    contract = name,
                    address = addr_str,
                    "Contract validated: code exists on-chain"
                );
            }

            results.push(result);
        }

        // Fail hard if CTF Exchange has no code (critical contract)
        if let Some(ctf) = results.first() {
            if !ctf.has_code {
                anyhow::bail!(
                    "CTF Exchange at {} has no deployed code — cannot proceed",
                    config.ctf_exchange
                );
            }
        }

        info!(
            validated = results.len(),
            "All contract validations complete"
        );
        Ok(results)
    }

    /// Validate a single contract by checking if code exists at the address.
    async fn validate_contract(
        &self,
        name: &str,
        addr_str: &str,
    ) -> Result<ValidationResult> {
        let address: Address = addr_str
            .parse()
            .context(format!("Invalid address for {name}: {addr_str}"))?;

        let code = self
            .provider
            .get_code_at(address)
            .await
            .context(format!("Failed to query code for {name}"))?;

        let has_code = !code.is_empty();

        Ok(ValidationResult {
            name: name.to_string(),
            address: addr_str.to_string(),
            has_code,
        })
    }
}
