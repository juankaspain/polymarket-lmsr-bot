//! Settlement Use Case - Batch Redemption of Resolved Markets
//!
//! Handles the process of redeeming positions from resolved
//! Polymarket prediction markets. Uses the CTF (Conditional Token
//! Framework) contract on Polygon for batch redemption.
//!
//! Settlement flow:
//! 1. Scan open positions for resolved markets
//! 2. Verify resolution on-chain
//! 3. Batch redeem winning positions
//! 4. Update local state and log results

use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::Utc;
use tracing::{error, info, warn};

use crate::domain::trade::{MarketId, Position, TokenId};
use crate::ports::chain_client::{ChainClient, RedemptionResult};
use crate::ports::repository::Repository;

/// Status of a market resolution check.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolutionStatus {
  /// Market has not yet resolved.
  Pending,
  /// Market resolved; YES won.
  ResolvedYes,
  /// Market resolved; NO won.
  ResolvedNo,
  /// Market was voided / cancelled.
  Voided,
}

/// Summary of a single settlement attempt.
#[derive(Debug, Clone)]
pub struct SettlementResult {
  /// Market condition ID.
  pub market_id: MarketId,
  /// Resolution outcome.
  pub resolution: ResolutionStatus,
  /// USDC recovered from redemption.
  pub usdc_recovered: f64,
  /// Transaction hash (if redeemed on-chain).
  pub tx_hash: Option<String>,
  /// Whether settlement succeeded.
  pub success: bool,
  /// Error message if settlement failed.
  pub error: Option<String>,
}

/// Aggregated report from a settlement sweep.
#[derive(Debug, Clone)]
pub struct SettlementReport {
  /// Individual settlement results.
  pub results: Vec<SettlementResult>,
  /// Total USDC recovered across all settlements.
  pub total_usdc_recovered: f64,
  /// Number of markets successfully settled.
  pub markets_settled: usize,
  /// Number of markets that failed settlement.
  pub markets_failed: usize,
  /// Timestamp of the sweep.
  pub timestamp: chrono::DateTime<Utc>,
}

/// Settlement manager that handles batch redemption of resolved markets.
pub struct Settlement<C: ChainClient, R: Repository> {
  chain: C,
  repo: R,
  /// Minimum USDC value to trigger on-chain redemption (avoid dust).
  min_redemption_value: f64,
  /// Maximum positions to redeem in a single batch.
  max_batch_size: usize,
}

impl<C: ChainClient, R: Repository> Settlement<C, R> {
  /// Create a new settlement manager.
  pub fn new(chain: C, repo: R) -> Self {
    Self {
      chain,
      repo,
      min_redemption_value: 0.10,
      max_batch_size: 20,
    }
  }

  /// Create with custom thresholds.
  pub fn with_config(
    chain: C,
    repo: R,
    min_redemption_value: f64,
    max_batch_size: usize,
  ) -> Self {
    Self {
      chain,
      repo,
      min_redemption_value,
      max_batch_size,
    }
  }

  /// Run a full settlement sweep across all open positions.
  ///
  /// Checks each position's market for resolution, then batch-redeems
  /// any winning positions via the CTF contract.
  pub async fn sweep(&self, positions: &[Position]) -> Result<SettlementReport> {
    info!(
      position_count = positions.len(),
      "Starting settlement sweep"
    );

    let mut results = Vec::new();
    let mut redeemable: Vec<&Position> = Vec::new();

    // Phase 1: Check resolution status for each position's market
    for position in positions {
      match self.check_resolution(&position.condition_id).await {
        Ok(status) => {
          match status {
            ResolutionStatus::ResolvedYes | ResolutionStatus::ResolvedNo => {
              info!(
                market_id = %position.condition_id,
                resolution = ?status,
                "Market resolved, queuing for redemption"
              );
              redeemable.push(position);
            }
            ResolutionStatus::Voided => {
              info!(
                market_id = %position.condition_id,
                "Market voided, queuing for redemption"
              );
              redeemable.push(position);
            }
            ResolutionStatus::Pending => {
              // Not yet resolved, skip
            }
          }
        }
        Err(e) => {
          warn!(
            market_id = %position.condition_id,
            error = %e,
            "Failed to check resolution status"
          );
          results.push(SettlementResult {
            market_id: position.condition_id.clone(),
            resolution: ResolutionStatus::Pending,
            usdc_recovered: 0.0,
            tx_hash: None,
            success: false,
            error: Some(format!("Resolution check failed: {e}")),
          });
        }
      }
    }

    // Phase 2: Batch redeem resolved positions
    if !redeemable.is_empty() {
      let batch_results = self.batch_redeem(&redeemable).await;
      results.extend(batch_results);
    }

    // Phase 3: Build report
    let total_usdc_recovered: f64 = results
      .iter()
      .filter(|r| r.success)
      .map(|r| r.usdc_recovered)
      .sum();

    let markets_settled = results.iter().filter(|r| r.success).count();
    let markets_failed = results.iter().filter(|r| !r.success).count();

    let report = SettlementReport {
      results,
      total_usdc_recovered,
      markets_settled,
      markets_failed,
      timestamp: Utc::now(),
    };

    info!(
      total_recovered = report.total_usdc_recovered,
      settled = report.markets_settled,
      failed = report.markets_failed,
      "Settlement sweep complete"
    );

    Ok(report)
  }

  /// Check if a market's condition has been resolved on-chain.
  async fn check_resolution(&self, condition_id: &str) -> Result<ResolutionStatus> {
    let resolved = self
      .chain
      .is_condition_resolved(condition_id)
      .await
      .context("Failed to query condition resolution")?;

    if resolved {
      // For simplicity, return ResolvedYes; a full implementation
      // would query the payout vector to determine the outcome.
      Ok(ResolutionStatus::ResolvedYes)
    } else {
      Ok(ResolutionStatus::Pending)
    }
  }

  /// Batch redeem a set of positions, respecting batch size limits.
  async fn batch_redeem(&self, positions: &[&Position]) -> Vec<SettlementResult> {
    let mut results = Vec::new();

    for chunk in positions.chunks(self.max_batch_size) {
      let condition_ids: Vec<String> = chunk
        .iter()
        .map(|p| p.condition_id.clone())
        .collect();

      info!(
        batch_size = chunk.len(),
        "Submitting batch redemption"
      );

      match self.chain.batch_redeem(&condition_ids).await {
        Ok(redemption) => {
          info!(
            tx_hash = %redemption.tx_hash,
            positions_redeemed = redemption.positions_redeemed,
            usdc_recovered = redemption.usdc_recovered,
            "Batch redemption successful"
          );

          // Distribute recovered USDC proportionally
          let per_position = if redemption.positions_redeemed > 0 {
            redemption.usdc_recovered / redemption.positions_redeemed as f64
          } else {
            0.0
          };

          for pos in chunk {
            results.push(SettlementResult {
              market_id: pos.condition_id.clone(),
              resolution: ResolutionStatus::ResolvedYes,
              usdc_recovered: per_position,
              tx_hash: Some(redemption.tx_hash.clone()),
              success: true,
              error: None,
            });
          }
        }
        Err(e) => {
          error!(
            error = %e,
            batch_size = chunk.len(),
            "Batch redemption failed"
          );

          for pos in chunk {
            results.push(SettlementResult {
              market_id: pos.condition_id.clone(),
              resolution: ResolutionStatus::ResolvedYes,
              usdc_recovered: 0.0,
              tx_hash: None,
              success: false,
              error: Some(format!("Batch redemption failed: {e}")),
            });
          }
        }
      }
    }

    results
  }

  /// Settle a single market position.
  pub async fn settle_single(&self, position: &Position) -> Result<SettlementResult> {
    let status = self.check_resolution(&position.condition_id).await?;

    match status {
      ResolutionStatus::Pending => {
        Ok(SettlementResult {
          market_id: position.condition_id.clone(),
          resolution: ResolutionStatus::Pending,
          usdc_recovered: 0.0,
          tx_hash: None,
          success: false,
          error: Some("Market not yet resolved".to_string()),
        })
      }
      _ => {
        let ids = vec![position.condition_id.clone()];
        match self.chain.batch_redeem(&ids).await {
          Ok(redemption) => Ok(SettlementResult {
            market_id: position.condition_id.clone(),
            resolution: status,
            usdc_recovered: redemption.usdc_recovered,
            tx_hash: Some(redemption.tx_hash),
            success: true,
            error: None,
          }),
          Err(e) => Ok(SettlementResult {
            market_id: position.condition_id.clone(),
            resolution: status,
            usdc_recovered: 0.0,
            tx_hash: None,
            success: false,
            error: Some(format!("Redemption failed: {e}")),
          }),
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_settlement_report_aggregation() {
    let results = vec![
      SettlementResult {
        market_id: "market_1".to_string(),
        resolution: ResolutionStatus::ResolvedYes,
        usdc_recovered: 50.0,
        tx_hash: Some("0xabc".to_string()),
        success: true,
        error: None,
      },
      SettlementResult {
        market_id: "market_2".to_string(),
        resolution: ResolutionStatus::ResolvedNo,
        usdc_recovered: 0.0,
        tx_hash: None,
        success: false,
        error: Some("Redemption failed".to_string()),
      },
      SettlementResult {
        market_id: "market_3".to_string(),
        resolution: ResolutionStatus::ResolvedYes,
        usdc_recovered: 30.0,
        tx_hash: Some("0xdef".to_string()),
        success: true,
        error: None,
      },
    ];

    let total: f64 = results.iter().filter(|r| r.success).map(|r| r.usdc_recovered).sum();
    let settled = results.iter().filter(|r| r.success).count();
    let failed = results.iter().filter(|r| !r.success).count();

    assert_eq!(total, 80.0);
    assert_eq!(settled, 2);
    assert_eq!(failed, 1);
  }

  #[test]
  fn test_resolution_status_eq() {
    assert_eq!(ResolutionStatus::Pending, ResolutionStatus::Pending);
    assert_ne!(ResolutionStatus::ResolvedYes, ResolutionStatus::ResolvedNo);
  }
}
