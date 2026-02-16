//! CLOB Authentication — HMAC-SHA256 Request Signing
//!
//! Signs every CLOB API request using HMAC-SHA256 per the Polymarket
//! CLOB specification. Credentials come from environment variables
//! (POLY_API_KEY, POLY_API_SECRET, POLY_PASSPHRASE).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use base64::Engine;

/// Thread-safe nonce generator: timestamp_seed + atomic counter.
///
/// Guarantees unique nonces even for concurrent requests within
/// the same millisecond. Seed is set once at construction from
/// system clock; counter increments atomically per request.
static NONCE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Bundle of non-secret credentials for request headers.
///
/// Contains the API key and passphrase (NOT the secret).
/// Used by `ClobClient` to attach auth headers to requests.
#[derive(Debug, Clone)]
pub struct ClobCredentials {
    /// API key from POLY_API_KEY env var.
    pub api_key: String,
    /// Passphrase from POLY_PASSPHRASE env var.
    pub api_passphrase: String,
}

/// CLOB API authentication handler.
///
/// Manages API key, secret, and passphrase loaded from env vars.
/// Signs requests using HMAC-SHA256 as required by Polymarket CLOB.
pub struct ClobAuth {
    /// API key from POLY_API_KEY env var.
    api_key: String,
    /// API secret from POLY_API_SECRET env var (never sent in headers).
    api_secret: String,
    /// Passphrase from POLY_PASSPHRASE env var.
    passphrase: String,
    /// Timestamp seed set at construction for nonce generation.
    nonce_seed: u64,
}

impl ClobAuth {
    /// Load credentials from environment variables.
    ///
    /// Required env vars: POLY_API_KEY, POLY_API_SECRET, POLY_PASSPHRASE.
    /// These MUST be set in `.env` (never committed to git).
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("POLY_API_KEY")
            .context("POLY_API_KEY not set")?;
        let api_secret = std::env::var("POLY_API_SECRET")
            .context("POLY_API_SECRET not set")?;
        let passphrase = std::env::var("POLY_PASSPHRASE")
            .context("POLY_PASSPHRASE not set")?;

        let nonce_seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(Self {
            api_key,
            api_secret,
            passphrase,
            nonce_seed,
        })
    }

    /// Get the API key for request headers.
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Get the passphrase for request headers.
    pub fn passphrase(&self) -> &str {
        &self.passphrase
    }

    /// Return non-secret credentials bundle for auth headers.
    ///
    /// Used by `ClobClient::execute_with_retry()` to attach
    /// POLY_API_KEY and POLY_PASSPHRASE headers.
    pub fn credentials(&self) -> Option<ClobCredentials> {
        Some(ClobCredentials {
            api_key: self.api_key.clone(),
            api_passphrase: self.passphrase.clone(),
        })
    }

    /// Generate a unique nonce using timestamp_seed + atomic increment.
    ///
    /// This ensures no two requests share a nonce even under
    /// high concurrency (checklist: nonce=timestamp_seed+atomic_fetch_add).
    pub fn generate_nonce(&self) -> u64 {
        let counter = NONCE_COUNTER.fetch_add(1, Ordering::Relaxed);
        self.nonce_seed + counter
    }

    /// Generate the current Unix timestamp in seconds (for signing).
    ///
    /// Associated function (no `&self`) so `ClobClient` can call it
    /// as `ClobAuth::timestamp()` without borrowing the auth instance.
    pub fn timestamp() -> String {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string()
    }

    /// Sign a request using HMAC-SHA256.
    ///
    /// Signature format: HMAC-SHA256(secret, timestamp + method + path + body)
    /// The secret is NEVER sent as a header — only the computed signature.
    pub fn sign(
        &self,
        timestamp: &str,
        method: &str,
        path: &str,
        body: &str,
    ) -> String {
        let message = format!("{}{}{}{}", timestamp, method, path, body);
        let mac = hmac_sha256::HMAC::mac(
            message.as_bytes(),
            self.api_secret.as_bytes(),
        );
        base64::engine::general_purpose::STANDARD.encode(mac)
    }

    /// Sign a request, returning Result for use in fallible contexts.
    ///
    /// Thin wrapper over `sign()` that matches the call-site in
    /// `ClobClient::execute_with_retry()` which expects `Result<String>`.
    pub fn sign_request(
        &self,
        timestamp: &str,
        method: &str,
        path: &str,
        body: &str,
    ) -> Result<String> {
        Ok(self.sign(timestamp, method, path, body))
    }

    /// Build all authentication headers for a CLOB request.
    ///
    /// Returns (key, timestamp, signature, passphrase) tuple.
    /// The API secret is NEVER included — only the HMAC signature.
    pub fn auth_headers(
        &self,
        method: &str,
        path: &str,
        body: &str,
    ) -> (String, String, String, String) {
        let timestamp = Self::timestamp();
        let signature = self.sign(&timestamp, method, path, body);
        (
            self.api_key.clone(),
            timestamp,
            signature,
            self.passphrase.clone(),
        )
    }
}
