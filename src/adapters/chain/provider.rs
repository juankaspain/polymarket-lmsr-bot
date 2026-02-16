//! Polygon RPC Provider - alloy-rs 0.9 Connection Management
//!
//! Manages the connection to the Polygon PoS chain via alloy-rs.
//! Validates RPC connectivity at startup and exposes a shared provider
//! instance for all on-chain operations.

use std::sync::Arc;

use alloy::providers::{Provider, ProviderBuilder, RootProvider};
use alloy::network::Ethereum;
use alloy::transports::http::Http;
use anyhow::{Context, Result};
use reqwest::Client;
use tracing::{info, instrument};

use crate::config::ApiConfig;

/// Shared Polygon RPC provider backed by alloy-rs 0.9.
///
/// All chain adapters share a single provider instance to avoid
/// redundant connections and enable connection pooling.
pub struct PolygonProvider {
    /// The alloy HTTP provider connected to Polygon RPC.
    provider: Arc<RootProvider<Http<Client>>>,
    /// RPC endpoint URL (for diagnostics, never logged with secrets).
    #[allow(dead_code)]
    rpc_url: String,
}

impl PolygonProvider {
    /// Connect to Polygon RPC and validate the chain ID.
    ///
    /// Reads the RPC URL from config. The URL itself comes from
    /// `config.toml` (never hardcoded). Validates chain ID = 137
    /// (Polygon mainnet) at startup.
    #[instrument(skip_all)]
    pub async fn connect(config: &ApiConfig) -> Result<Self> {
        let rpc_url = config.rpc_url.clone();

        // NOTE: on_http() is synchronous in alloy 0.9 — no .await
        let provider = ProviderBuilder::new()
            .on_http(rpc_url.parse().context("Invalid RPC URL")?);

        let provider = Arc::new(provider);

        // Validate chain ID at startup
        let chain_id = provider
            .get_chain_id()
            .await
            .context("Failed to query chain ID")?;

        if chain_id != 137 {
            anyhow::bail!(
                "Expected Polygon mainnet (chain_id=137), got {chain_id}"
            );
        }

        info!(chain_id, "Connected to Polygon RPC");

        Ok(Self { provider, rpc_url })
    }

    /// Get a shared reference to the alloy provider.
    pub fn inner(&self) -> Arc<RootProvider<Http<Client>>> {
        Arc::clone(&self.provider)
    }

    /// Check if the RPC connection is healthy via a lightweight call.
    pub async fn is_healthy(&self) -> bool {
        self.provider.get_block_number().await.is_ok()
    }
}
