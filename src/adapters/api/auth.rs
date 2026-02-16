//! CLOB API Authentication - EIP-712 Signature-based Auth
//!
//! Handles Polymarket API authentication using EIP-712 typed data
//! signatures. The bot signs API credentials and order payloads
//! using the configured private key.

use std::time::{SystemTime, UNIX_EPOCH};

use alloy::signers::local::PrivateKeySigner;
use alloy::signers::Signer;
use anyhow::{Context, Result};
use tracing::debug;

/// API authentication credentials derived from wallet signature.
#[derive(Debug, Clone)]
pub struct ApiCredentials {
  /// API key provided by Polymarket.
  pub api_key: String,
  /// API secret for HMAC signing.
  pub api_secret: String,
  /// Passphrase for additional auth layer.
  pub api_passphrase: String,
}

/// Manages EIP-712 signing for CLOB API authentication.
pub struct ClobAuth {
  /// The wallet signer (private key).
  signer: PrivateKeySigner,
  /// Cached API credentials.
  credentials: Option<ApiCredentials>,
  /// Chain ID for EIP-712 domain separator (137 = Polygon).
  chain_id: u64,
}

impl ClobAuth {
  /// Create a new auth manager from a private key hex string.
  pub fn new(private_key_hex: &str, chain_id: u64) -> Result<Self> {
    let signer: PrivateKeySigner = private_key_hex
      .parse()
      .context("Failed to parse private key")?;

    debug!(
      address = %signer.address(),
      "Auth manager initialized"
    );

    Ok(Self {
      signer,
      credentials: None,
      chain_id,
    })
  }

  /// Get the wallet address as a hex string.
  pub fn address(&self) -> String {
    format!("{:?}", self.signer.address())
  }

  /// Set pre-existing API credentials (from env vars).
  pub fn set_credentials(&mut self, credentials: ApiCredentials) {
    self.credentials = Some(credentials);
  }

  /// Get a reference to stored credentials.
  pub fn credentials(&self) -> Option<&ApiCredentials> {
    self.credentials.as_ref()
  }

  /// Generate a HMAC signature for API requests.
  ///
  /// The CLOB API requires HMAC-SHA256 signatures on request
  /// timestamps and payloads for authenticated endpoints.
  pub fn sign_request(
    &self,
    timestamp: &str,
    method: &str,
    path: &str,
    body: &str,
  ) -> Result<String> {
    let credentials = self
      .credentials
      .as_ref()
      .context("API credentials not set")?;

    let message = format!("{}{}{}{}", timestamp, method.to_uppercase(), path, body);

    let key = hmac_sha256::HMAC::mac(
      message.as_bytes(),
      credentials.api_secret.as_bytes(),
    );

    Ok(base64::Engine::encode(
      &base64::engine::general_purpose::STANDARD,
      key,
    ))
  }

  /// Generate the current timestamp string for API requests.
  pub fn timestamp() -> String {
    let now = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .expect("Time went backwards");
    now.as_secs().to_string()
  }

  /// Generate a nonce for order signing.
  pub fn generate_nonce() -> u64 {
    SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .expect("Time went backwards")
      .as_millis() as u64
  }

  /// Get the underlying signer for direct EIP-712 signing.
  pub fn signer(&self) -> &PrivateKeySigner {
    &self.signer
  }

  /// Get the chain ID.
  pub fn chain_id(&self) -> u64 {
    self.chain_id
  }
}
