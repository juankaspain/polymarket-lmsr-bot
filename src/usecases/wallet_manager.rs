//! Wallet Manager Use Case - Balance Tracking and USDC Management
//!
//! Tracks the bot's USDC balance, token positions, and provides
//! bankroll management for the risk manager. Queries on-chain
//! balances via the ChainClient port and caches locally.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::domain::trade::TokenId;
use crate::ports::chain_client::ChainClient;

/// Snapshot of the wallet state at a point in time.
#[derive(Debug, Clone)]
pub struct WalletSnapshot {
  /// USDC balance available for trading.
  pub usdc_balance: f64,
  /// Token balances by token ID.
  pub token_balances: HashMap<String, f64>,
  /// Total portfolio value in USDC (balance + positions).
  pub total_value: f64,
  /// When this snapshot was taken.
  pub timestamp: DateTime<Utc>,
}

/// Cached balance entry with staleness tracking.
#[derive(Debug, Clone)]
struct CachedBalance {
  value: f64,
  updated_at: DateTime<Utc>,
}

impl CachedBalance {
  fn is_stale(&self, max_age_secs: i64) -> bool {
    let age = Utc::now() - self.updated_at;
    age.num_seconds() > max_age_secs
  }
}

/// Manages wallet balances, caching, and bankroll queries.
pub struct WalletManager<C: ChainClient> {
  chain: Arc<C>,
  /// Cached USDC balance.
  usdc_cache: RwLock<Option<CachedBalance>>,
  /// Cached token balances.
  token_cache: RwLock<HashMap<String, CachedBalance>>,
  /// Maximum cache age in seconds before refresh.
  cache_ttl_secs: i64,
  /// Initial bankroll recorded at startup.
  initial_bankroll: RwLock<Option<f64>>,
}

impl<C: ChainClient> WalletManager<C> {
  /// Create a new wallet manager.
  pub fn new(chain: Arc<C>) -> Self {
    Self {
      chain,
      usdc_cache: RwLock::new(None),
      token_cache: RwLock::new(HashMap::new()),
      cache_ttl_secs: 30,
      initial_bankroll: RwLock::new(None),
    }
  }

  /// Create with custom cache TTL.
  pub fn with_cache_ttl(chain: Arc<C>, cache_ttl_secs: i64) -> Self {
    Self {
      chain,
      usdc_cache: RwLock::new(None),
      token_cache: RwLock::new(HashMap::new()),
      cache_ttl_secs,
      initial_bankroll: RwLock::new(None),
    }
  }

  /// Get the current USDC balance, using cache if fresh.
  pub async fn usdc_balance(&self) -> Result<f64> {
    // Check cache first
    {
      let cache = self.usdc_cache.read().await;
      if let Some(ref cached) = *cache {
        if !cached.is_stale(self.cache_ttl_secs) {
          return Ok(cached.value);
        }
      }
    }

    // Cache miss or stale; query on-chain
    let balance = self
      .chain
      .usdc_balance()
      .await
      .context("Failed to query USDC balance")?;

    // Update cache
    {
      let mut cache = self.usdc_cache.write().await;
      *cache = Some(CachedBalance {
        value: balance,
        updated_at: Utc::now(),
      });
    }

    Ok(balance)
  }

  /// Get the balance for a specific token.
  pub async fn token_balance(&self, token_id: &str) -> Result<f64> {
    // Check cache
    {
      let cache = self.token_cache.read().await;
      if let Some(cached) = cache.get(token_id) {
        if !cached.is_stale(self.cache_ttl_secs) {
          return Ok(cached.value);
        }
      }
    }

    // Query on-chain
    let tb = self
      .chain
      .token_balance(token_id)
      .await
      .context("Failed to query token balance")?;

    let balance = tb.balance;

    // Update cache
    {
      let mut cache = self.token_cache.write().await;
      cache.insert(
        token_id.to_string(),
        CachedBalance {
          value: balance,
          updated_at: Utc::now(),
        },
      );
    }

    Ok(balance)
  }

  /// Get a full wallet snapshot (refreshes all balances).
  pub async fn snapshot(&self) -> Result<WalletSnapshot> {
    let usdc = self.usdc_balance().await?;

    let token_balances = {
      let cache = self.token_cache.read().await;
      cache
        .iter()
        .map(|(k, v)| (k.clone(), v.value))
        .collect::<HashMap<_, _>>()
    };

    // Estimate total value as USDC + sum of token balances
    // (a real implementation would price tokens at market value)
    let token_total: f64 = token_balances.values().sum();
    let total_value = usdc + token_total;

    Ok(WalletSnapshot {
      usdc_balance: usdc,
      token_balances,
      total_value,
      timestamp: Utc::now(),
    })
  }

  /// Record and return the initial bankroll (called once at startup).
  pub async fn record_initial_bankroll(&self) -> Result<f64> {
    let balance = self.usdc_balance().await?;

    {
      let mut initial = self.initial_bankroll.write().await;
      *initial = Some(balance);
    }

    info!(bankroll = balance, "Initial bankroll recorded");
    Ok(balance)
  }

  /// Get the initial bankroll (returns None if not yet recorded).
  pub async fn initial_bankroll(&self) -> Option<f64> {
    let guard = self.initial_bankroll.read().await;
    *guard
  }

  /// Calculate the current daily PnL relative to initial bankroll.
  pub async fn daily_pnl(&self) -> Result<f64> {
    let current = self.usdc_balance().await?;
    let initial = self.initial_bankroll().await.unwrap_or(current);
    Ok(current - initial)
  }

  /// Force-refresh all cached balances.
  pub async fn refresh(&self) -> Result<()> {
    // Clear USDC cache
    {
      let mut cache = self.usdc_cache.write().await;
      *cache = None;
    }

    // Refresh USDC
    let _ = self.usdc_balance().await?;

    // Refresh known token balances
    let token_ids: Vec<String> = {
      let cache = self.token_cache.read().await;
      cache.keys().cloned().collect()
    };

    for token_id in &token_ids {
      if let Err(e) = self.token_balance(token_id).await {
        warn!(
          token_id = %token_id,
          error = %e,
          "Failed to refresh token balance"
        );
      }
    }

    info!("Wallet balances refreshed");
    Ok(())
  }

  /// Check if the bankroll is above the minimum threshold.
  pub async fn is_above_minimum(&self, min_bankroll: f64) -> Result<bool> {
    let balance = self.usdc_balance().await?;
    Ok(balance >= min_bankroll)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_cached_balance_staleness() {
    let fresh = CachedBalance {
      value: 100.0,
      updated_at: Utc::now(),
    };
    assert!(!fresh.is_stale(30));

    let old = CachedBalance {
      value: 100.0,
      updated_at: Utc::now() - chrono::Duration::seconds(60),
    };
    assert!(old.is_stale(30));
  }

  #[test]
  fn test_wallet_snapshot_total() {
    let mut tokens = HashMap::new();
    tokens.insert("token_a".to_string(), 50.0);
    tokens.insert("token_b".to_string(), 30.0);

    let snapshot = WalletSnapshot {
      usdc_balance: 100.0,
      token_balances: tokens,
      total_value: 180.0,
      timestamp: Utc::now(),
    };

    assert_eq!(snapshot.total_value, 180.0);
    assert_eq!(snapshot.usdc_balance, 100.0);
  }
}
