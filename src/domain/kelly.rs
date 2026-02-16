//! Kelly Criterion position sizing.
//!
//! Implements fractional Kelly for optimal bankroll management.
//! We use quarter-Kelly (0.25x) by default for safety, which reduces
//! variance significantly while retaining ~75% of the growth rate.
//!
//! Exposes both `KellyCriterion` (Decimal API) and `KellySizer` (f64 API).

use rust_decimal::Decimal;
use rust_decimal::prelude::*;
use rust_decimal_macros::dec;

/// Kelly Criterion calculator for optimal position sizing (Decimal API).
///
/// Full Kelly maximizes long-term growth rate but has high variance.
/// We use fractional Kelly (default 0.25) for production safety.
#[derive(Debug, Clone)]
pub struct KellyCriterion {
    /// Kelly fraction multiplier (0.25 = quarter-Kelly)
    fraction: Decimal,
    /// Maximum position as fraction of bankroll
    max_position_fraction: Decimal,
}

impl KellyCriterion {
    /// Creates a new Kelly calculator with the given fraction and max position.
    pub fn new(fraction: Decimal, max_position_fraction: Decimal) -> Self {
        Self {
            fraction,
            max_position_fraction,
        }
    }

    /// Computes the optimal position size using fractional Kelly.
    ///
    /// Kelly formula for binary outcomes:
    ///   f* = (p * b - q) / b
    /// where:
    ///   p = probability of winning (our estimated fair price)
    ///   q = 1 - p
    ///   b = odds offered (payout ratio)
    pub fn optimal_fraction(
        &self,
        estimated_prob: Decimal,
        market_price: Decimal,
    ) -> Decimal {
        if market_price <= Decimal::ZERO || market_price >= Decimal::ONE {
            return Decimal::ZERO;
        }

        let b = (Decimal::ONE - market_price) / market_price;
        let q = Decimal::ONE - estimated_prob;

        let full_kelly = (estimated_prob * b - q) / b;

        if full_kelly <= Decimal::ZERO {
            return Decimal::ZERO;
        }

        let sized = full_kelly * self.fraction;
        sized.min(self.max_position_fraction)
    }

    /// Computes the position size in USDC given bankroll.
    pub fn position_size_usdc(
        &self,
        bankroll: Decimal,
        estimated_prob: Decimal,
        market_price: Decimal,
    ) -> Decimal {
        let fraction = self.optimal_fraction(estimated_prob, market_price);
        (bankroll * fraction).round_dp(2)
    }
}

impl Default for KellyCriterion {
    /// Default: quarter-Kelly with 6.25% max position.
    fn default() -> Self {
        Self {
            fraction: dec!(0.25),
            max_position_fraction: dec!(0.0625),
        }
    }
}

// ────────────────────────────────────────────
// KellySizer — f64 boundary API for usecases
// ────────────────────────────────────────────

/// Lightweight f64 wrapper around KellyCriterion for use at the ports boundary.
///
/// Accepts and returns `f64` so usecases/adapters never import `Decimal`.
#[derive(Debug, Clone)]
pub struct KellySizer {
    inner: KellyCriterion,
}

impl KellySizer {
    /// Create a sizer with the given Kelly fraction (e.g., 0.25 for quarter-Kelly).
    pub fn new(fraction: f64) -> Self {
        let frac = Decimal::from_f64(fraction).unwrap_or(dec!(0.25));
        Self {
            inner: KellyCriterion::new(frac, dec!(0.0625)),
        }
    }

    /// Compute optimal position size in USDC.
    ///
    /// Returns the dollar amount to risk on this trade.
    pub fn optimal_size(
        &self,
        estimated_prob: f64,
        market_price: f64,
        bankroll: f64,
    ) -> f64 {
        let prob = Decimal::from_f64(estimated_prob).unwrap_or(dec!(0.5));
        let price = Decimal::from_f64(market_price).unwrap_or(dec!(0.5));
        let bank = Decimal::from_f64(bankroll).unwrap_or(Decimal::ZERO);

        self.inner
            .position_size_usdc(bank, prob, price)
            .to_f64()
            .unwrap_or(0.0)
    }

    /// Compute optimal fraction (0.0 – 1.0).
    pub fn optimal_fraction(&self, estimated_prob: f64, market_price: f64) -> f64 {
        let prob = Decimal::from_f64(estimated_prob).unwrap_or(dec!(0.5));
        let price = Decimal::from_f64(market_price).unwrap_or(dec!(0.5));

        self.inner
            .optimal_fraction(prob, price)
            .to_f64()
            .unwrap_or(0.0)
    }

    /// Access the underlying precise calculator.
    pub fn inner(&self) -> &KellyCriterion {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kelly_positive_edge() {
        let k
