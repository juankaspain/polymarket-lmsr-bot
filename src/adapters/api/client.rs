//! CLOB HTTP Client - Rate-limited REST API Client
//!
//! Wraps reqwest with rate limiting, retries, and authentication
//! for all Polymarket CLOB REST API interactions.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use tokio::sync::Semaphore;
use tokio::time::sleep;
use tracing::{debug, error, warn};

use super::auth::ClobAuth;
use super::types::RateLimitInfo;

/// Configuration for the CLOB HTTP client.
#[derive(Debug, Clone)]
pub struct ClobClientConfig {
  /// Base URL for the CLOB API.
  pub base_url: String,
  /// Request timeout.
  pub timeout: Duration,
  /// Maximum concurrent requests.
  pub max_concurrent: usize,
  /// Maximum retries on transient errors.
  pub max_retries: u32,
  /// Base delay between retries (exponential backoff).
  pub retry_base_delay: Duration,
}

impl Default for ClobClientConfig {
  fn default() -> Self {
    Self {
      base_url: "https://clob.polymarket.com".to_string(),
      timeout: Duration::from_secs(30),
      max_concurrent: 10,
      max_retries: 3,
      retry_base_delay: Duration::from_millis(200),
    }
  }
}

/// Rate-limited HTTP client for the Polymarket CLOB API.
pub struct ClobClient {
  /// Underlying HTTP client.
  http: Client,
  /// Authentication manager.
  auth: Arc<ClobAuth>,
  /// Client configuration.
  config: ClobClientConfig,
  /// Concurrency limiter.
  semaphore: Arc<Semaphore>,
  /// Last known rate limit info.
  last_rate_limit: tokio::sync::RwLock<Option<RateLimitInfo>>,
}

impl ClobClient {
  /// Create a new CLOB client.
  pub fn new(auth: Arc<ClobAuth>, config: ClobClientConfig) -> Result<Self> {
    let http = Client::builder()
      .timeout(config.timeout)
      .pool_max_idle_per_host(5)
      .build()
      .context("Failed to build HTTP client")?;

    let semaphore = Arc::new(Semaphore::new(config.max_concurrent));

    Ok(Self {
      http,
      auth,
      config,
      semaphore,
      last_rate_limit: tokio::sync::RwLock::new(None),
    })
  }

  /// Execute a GET request with auth headers and rate limiting.
  pub async fn get(&self, path: &str) -> Result<Response> {
    let url = format!("{}{}", self.config.base_url, path);
    let request = self.http.get(&url);
    self.execute_with_retry(request, "GET", path, "").await
  }

  /// Execute a POST request with auth headers and rate limiting.
  pub async fn post(&self, path: &str, body: &str) -> Result<Response> {
    let url = format!("{}{}", self.config.base_url, path);
    let request = self
      .http
      .post(&url)
      .header("Content-Type", "application/json")
      .body(body.to_string());
    self.execute_with_retry(request, "POST", path, body).await
  }

  /// Execute a DELETE request with auth headers and rate limiting.
  pub async fn delete(&self, path: &str) -> Result<Response> {
    let url = format!("{}{}", self.config.base_url, path);
    let request = self.http.delete(&url);
    self.execute_with_retry(request, "DELETE", path, "").await
  }

  /// Execute request with authentication, rate limiting, and retries.
  async fn execute_with_retry(
    &self,
    request: RequestBuilder,
    method: &str,
    path: &str,
    body: &str,
  ) -> Result<Response> {
    let _permit = self
      .semaphore
      .acquire()
      .await
      .context("Semaphore closed")?;

    let mut last_error = None;

    for attempt in 0..=self.config.max_retries {
      if attempt > 0 {
        let delay = self.config.retry_base_delay * 2u32.pow(attempt - 1);
        debug!(attempt, delay_ms = delay.as_millis(), "Retrying request");
        sleep(delay).await;
      }

      let timestamp = ClobAuth::timestamp();

      let mut req = request
        .try_clone()
        .context("Failed to clone request")?;

      // Add auth headers
      if let Some(creds) = self.auth.credentials() {
        req = req
          .header("POLY_API_KEY", &creds.api_key)
          .header("POLY_PASSPHRASE", &creds.api_passphrase)
          .header("POLY_TIMESTAMP", &timestamp);

        if let Ok(sig) = self.auth.sign_request(&timestamp, method, path, body) {
          req = req.header("POLY_SIGNATURE", sig);
        }
      }

      match req.send().await {
        Ok(response) => {
          // Extract rate limit headers
          self.update_rate_limit(&response).await;

          match response.status() {
            StatusCode::OK | StatusCode::CREATED => return Ok(response),
            StatusCode::TOO_MANY_REQUESTS => {
              warn!("Rate limited by CLOB API, backing off");
              sleep(Duration::from_secs(2)).await;
              last_error = Some(anyhow::anyhow!("Rate limited"));
              continue;
            }
            status if status.is_server_error() => {
              warn!(status = %status, "Server error, retrying");
              last_error = Some(anyhow::anyhow!("Server error: {status}"));
              continue;
            }
            status => {
              let body = response.text().await.unwrap_or_default();
              return Err(anyhow::anyhow!(
                "API error {status}: {body}"
              ));
            }
          }
        }
        Err(e) => {
          warn!(error = %e, attempt, "Request failed");
          last_error = Some(e.into());
          continue;
        }
      }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Max retries exceeded")))
  }

  /// Extract and cache rate limit info from response headers.
  async fn update_rate_limit(&self, response: &Response) {
    let remaining = response
      .headers()
      .get("x-ratelimit-remaining")
      .and_then(|v| v.to_str().ok())
      .and_then(|v| v.parse().ok())
      .unwrap_or(50);

    let reset = response
      .headers()
      .get("x-ratelimit-reset")
      .and_then(|v| v.to_str().ok())
      .and_then(|v| v.parse().ok())
      .unwrap_or(0);

    let limit = response
      .headers()
      .get("x-ratelimit-limit")
      .and_then(|v| v.to_str().ok())
      .and_then(|v| v.parse().ok())
      .unwrap_or(50);

    let info = RateLimitInfo {
      remaining,
      reset_ms: reset,
      limit,
    };

    let mut guard = self.last_rate_limit.write().await;
    *guard = Some(info);
  }

  /// Get the last known rate limit status.
  pub async fn rate_limit_status(&self) -> Option<RateLimitInfo> {
    let guard = self.last_rate_limit.read().await;
    guard.clone()
  }

  /// Get a reference to the auth manager.
  pub fn auth(&self) -> &ClobAuth {
    &self.auth
  }

  /// Check if the API is reachable.
  pub async fn health_check(&self) -> bool {
    self.get("/time").await.is_ok()
  }
}
