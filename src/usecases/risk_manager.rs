//! Risk Manager - Position Limits and Circuit Breakers
//!
//! Enforces risk controls:
//! - Maximum daily loss (fraction of bankroll)
//! - Maximum position size per market
//! - Maximum total exposure
//! - Circuit breaker on consecutive losses
//! - Cooldown period after circuit breaker trigger

use tracing::{info, warn};

use crate::config::RiskConfig;

/// Risk manager enforcing trading limits and circuit breakers.
pub struct RiskManager {
  /// Maximum daily loss as fraction of bankroll.
  max_daily_loss_fraction: f64,
  /// Maximum position size per market (USDC).
  max_position_size: f64,
  /// Maximum total exposure.
  max_total_exposure: f64,
  /// Minimum bankroll to continue.
  min_bankroll: f64,
  /// Circuit breaker threshold (consecutive losses).
  circuit_breaker_losses: u32,
  /// Cooldown period (seconds).
  cooldown_seconds: u64,
  /// Current daily realized loss.
  daily_loss: f64,
  /// Consecutive loss counter.
  consecutive_losses: u32,
  /// Whether circuit breaker is active.
  circuit_breaker_active: bool,
  /// When circuit breaker was triggered (Unix ms).
  circuit_breaker_time: Option<u64>,
  /// Current total exposure.
  total_exposure: f64,
}

impl RiskManager {
  /// Create a new risk manager from config.
  pub fn new(config: &RiskConfig) -> Self {
    Self {
      max_daily_loss_fraction: config.max_daily_loss_fraction,
      max_position_size: config.max_position_size,
      max_total_exposure: config.max_total_exposure,
      min_bankroll: config.min_bankroll,
      circuit_breaker_losses: config.circuit_breaker_losses,
      cooldown_seconds: config.cooldown_seconds,
      daily_loss: 0.0,
      consecutive_losses: 0,
      circuit_breaker_active: false,
      circuit_breaker_time: None,
      total_exposure: 0.0,
    }
  }

  /// Check if trading is currently allowed.
  pub fn can_trade(&self) -> bool {
    if self.circuit_breaker_active {
      if let Some(trigger_time) = self.circuit_breaker_time {
        let now = std::time::SystemTime::now()
          .duration_since(std::time::UNIX_EPOCH)
          .unwrap_or_default()
          .as_millis() as u64;
        let elapsed_secs = (now - trigger_time) / 1000;
        if elapsed_secs < self.cooldown_seconds {
          return false;
        }
      }
    }
    true
  }

  /// Check if a new position of given size is allowed.
  pub fn can_open_position(&self, size: f64, bankroll: f64) -> bool {
    if !self.can_trade() {
      return false;
    }

    // Check minimum bankroll
    if bankroll < self.min_bankroll {
      warn!(
        bankroll = bankroll,
        min = self.min_bankroll,
        "Bankroll below minimum"
      );
      return false;
    }

    // Check position size limit
    if size > self.max_position_size {
      return false;
    }

    // Check total exposure
    if self.total_exposure + size > self.max_total_exposure {
      return false;
    }

    // Check daily loss limit
    let max_loss = bankroll * self.max_daily_loss_fraction;
    if self.daily_loss >= max_loss {
      warn!(
        daily_loss = self.daily_loss,
        max = max_loss,
        "Daily loss limit reached"
      );
      return false;
    }

    true
  }

  /// Record a trade result.
  pub fn record_trade(&mut self, pnl: f64) {
    if pnl < 0.0 {
      self.daily_loss += pnl.abs();
      self.consecutive_losses += 1;

      if self.consecutive_losses >= self.circuit_breaker_losses {
        self.trigger_circuit_breaker();
      }
    } else {
      self.consecutive_losses = 0;
    }
  }

  /// Update total exposure.
  pub fn update_exposure(&mut self, exposure: f64) {
    self.total_exposure = exposure;
  }

  /// Reset daily counters (called at day boundary).
  pub fn reset_daily(&mut self) {
    info!(
      daily_loss = self.daily_loss,
      "Resetting daily risk counters"
    );
    self.daily_loss = 0.0;
    self.consecutive_losses = 0;
    self.circuit_breaker_active = false;
    self.circuit_breaker_time = None;
  }

  /// Get current daily loss.
  pub fn daily_loss(&self) -> f64 {
    self.daily_loss
  }

  /// Check if circuit breaker is active.
  pub fn is_circuit_breaker_active(&self) -> bool {
    self.circuit_breaker_active
  }

  /// Trigger the circuit breaker.
  fn trigger_circuit_breaker(&mut self) {
    let now = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap_or_default()
      .as_millis() as u64;

    self.circuit_breaker_active = true;
    self.circuit_breaker_time = Some(now);

    warn!(
      consecutive_losses = self.consecutive_losses,
      cooldown_seconds = self.cooldown_seconds,
      "Circuit breaker triggered"
    );
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn test_config() -> RiskConfig {
    RiskConfig {
      max_daily_loss_fraction: 0.02,
      max_position_size: 100.0,
      max_total_exposure: 500.0,
      min_bankroll: 50.0,
      circuit_breaker_losses: 3,
      cooldown_seconds: 300,
    }
  }

  #[test]
  fn test_can_trade_initially() {
    let rm = RiskManager::new(&test_config());
    assert!(rm.can_trade());
  }

  #[test]
  fn test_circuit_breaker_triggers() {
    let mut rm = RiskManager::new(&test_config());
    rm.record_trade(-10.0);
    rm.record_trade(-10.0);
    rm.record_trade(-10.0);
    assert!(rm.is_circuit_breaker_active());
  }

  #[test]
  fn test_winning_trade_resets_counter() {
    let mut rm = RiskManager::new(&test_config());
    rm.record_trade(-10.0);
    rm.record_trade(-10.0);
    rm.record_trade(5.0); // Win resets counter
    rm.record_trade(-10.0);
    assert!(!rm.is_circuit_breaker_active());
  }
}
