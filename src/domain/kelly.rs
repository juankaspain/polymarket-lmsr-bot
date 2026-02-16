//! Kelly Criterion position sizing.
//!
//! Implements fractional Kelly for optimal bankroll management.
//! We use quarter-Kelly (0.25x) by default for safety, which reduces
//! variance significantly while retaining ~75% of the growth rate.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Kelly Criterion calculator for optimal position sizing.
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
    ///
    /// Returns the fraction of bankroll to bet (0.0 if negative edge).
    pub fn optimal_fraction(
        &self,
        estimated_prob: Decimal,
        market_price: Decimal,
    ) -> Decimal {
        if market_price <= Decimal::ZERO || market_price >= Decimal::ONE {
            return Decimal::ZERO;
        }

        // Payout ratio: if we buy at market_price, we get 1.0 on win
        let b = (Decimal::ONE - market_price) / market_price;
        let q = Decimal::ONE - estimated_prob;

        // Full Kelly fraction
        let full_kelly = (estimated_prob * b - q) / b;

        if full_kelly <= Decimal::ZERO {
            return Decimal::ZERO;
        }

        // Apply fractional Kelly and cap at max position
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kelly_positive_edge() {
        let kelly = KellyCriterion::default();
        // Fair price 0.60, market price 0.45 -> positive edge
        let f = kelly.optimal_fraction(dec!(0.60), dec!(0.45));
        assert!(f > Decimal::ZERO, "Should have positive position for edge");
    }

    #[test]
    fn test_kelly_no_edge() {
        let kelly = KellyCriterion::default();
        // Fair price equals market price -> no edge
        let f = kelly.optimal_fraction(dec!(0.50), dec!(0.50));
        assert_eq!(f, Decimal::ZERO, "No edge should give zero position");
    }

    #[test]
    fn test_kelly_negative_edge() {
        let kelly = KellyCriterion::default();
        // Fair price 0.40, market price 0.50 -> negative edge
        let f = kelly.optimal_fraction(dec!(0.40), dec!(0.50));
        assert_eq!(f, Decimal::ZERO, "Negative edge should give zero position");
    }

    #[test]
    fn test_kelly_capped_at_max() {
        let kelly = KellyCriterion::new(dec!(1.0), dec!(0.0625));
        // Full Kelly with huge edge should still be capped
        let f = kelly.optimal_fraction(dec!(0.90), dec!(0.10));
        assert!(f <= dec!(0.0625), "Position should be capped at max fraction");
    }

    #[test]
    fn test_position_size_usdc() {
        let kelly = KellyCriterion::default();
        let size = kelly.position_size_usdc(dec!(100.0), dec!(0.60), dec!(0.45));
        assert!(size > Decimal::ZERO);
        assert!(size <= dec!(6.25), "Should not exceed 6.25% of bankroll");
    }
}
