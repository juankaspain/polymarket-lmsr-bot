//! Logarithmic Market Scoring Rule (LMSR) implementation.
//!
//! The LMSR is the core pricing model used by Polymarket.
//! This module computes fair prices and costs for binary outcome markets.
//! Reference: Hanson (2003) "Combinatorial Information Market Design"
//!
//! Exposes both a Decimal API (LmsrModel) for precise internal
//! accounting and an f64 API (LmsrPricer) for ports/adapters.

use rust_decimal::Decimal;
use rust_decimal::prelude::*;
use serde::{Deserialize, Serialize};

/// LMSR pricing model for binary outcome markets.
///
/// The liquidity parameter `b` controls market depth:
/// - Higher `b` = more liquidity, tighter spreads, slower price movement
/// - Lower `b` = less liquidity, wider spreads, faster price movement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LmsrModel {
    /// Liquidity parameter (b > 0)
    b: Decimal,
}

impl LmsrModel {
    /// Creates a new LMSR model with the given liquidity parameter.
    ///
    /// # Panics
    /// Panics if `b` is not positive.
    pub fn new(b: Decimal) -> Self {
        assert!(b > Decimal::ZERO, "LMSR liquidity parameter b must be positive");
        Self { b }
    }

    /// Returns the liquidity parameter.
    pub fn liquidity(&self) -> Decimal {
        self.b
    }

    /// Computes the LMSR cost function: C(q) = b * ln(sum(exp(q_i / b))).
    ///
    /// For a binary market with quantities (q_yes, q_no):
    /// C = b * ln(exp(q_yes/b) + exp(q_no/b))
    pub fn cost(&self, q_yes: Decimal, q_no: Decimal) -> Decimal {
        let b_f64 = self.b.to_f64().unwrap_or(100.0);
        let q_yes_f64 = q_yes.to_f64().unwrap_or(0.0);
        let q_no_f64 = q_no.to_f64().unwrap_or(0.0);

        let exp_yes = (q_yes_f64 / b_f64).exp();
        let exp_no = (q_no_f64 / b_f64).exp();
        let result = b_f64 * (exp_yes + exp_no).ln();

        Decimal::from_f64(result).unwrap_or(Decimal::ZERO)
    }

    /// Computes the price (instantaneous marginal cost) for the YES outcome.
    ///
    /// price_yes = exp(q_yes/b) / (exp(q_yes/b) + exp(q_no/b))
    pub fn price_yes(&self, q_yes: Decimal, q_no: Decimal) -> Decimal {
        let b_f64 = self.b.to_f64().unwrap_or(100.0);
        let q_yes_f64 = q_yes.to_f64().unwrap_or(0.0);
        let q_no_f64 = q_no.to_f64().unwrap_or(0.0);

        let exp_yes = (q_yes_f64 / b_f64).exp();
        let exp_no = (q_no_f64 / b_f64).exp();
        let price = exp_yes / (exp_yes + exp_no);

        Decimal::from_f64(price).unwrap_or(Decimal::new(5, 1))
    }

    /// Computes the price for the NO outcome (1 - price_yes).
    pub fn price_no(&self, q_yes: Decimal, q_no: Decimal) -> Decimal {
        Decimal::ONE - self.price_yes(q_yes, q_no)
    }

    /// Computes the cost of buying `delta` YES shares.
    pub fn cost_to_buy_yes(
        &self,
        q_yes: Decimal,
        q_no: Decimal,
        delta: Decimal,
    ) -> Decimal {
        self.cost(q_yes + delta, q_no) - self.cost(q_yes, q_no)
    }

    /// Computes the cost of buying `delta` NO shares.
    pub fn cost_to_buy_no(
        &self,
        q_yes: Decimal,
        q_no: Decimal,
        delta: Decimal,
    ) -> Decimal {
        self.cost(q_yes, q_no + delta) - self.cost(q_yes, q_no)
    }

    /// Detects if there is an arbitrage edge between the external fair price
    /// and the LMSR-implied market price.
    pub fn detect_edge(
        &self,
        market_price_yes: Decimal,
        fair_price_yes: Decimal,
    ) -> Decimal {
        let edge = fair_price_yes - market_price_yes;
        (edge / market_price_yes * Decimal::ONE_HUNDRED).abs()
    }
}

// ────────────────────────────────────────────
// LmsrPricer — f64 boundary API for usecases
// ────────────────────────────────────────────

/// Lightweight f64 wrapper around LmsrModel for use at the ports boundary.
///
/// Accepts and returns `f64` so usecases and adapters never import `Decimal`.
/// Internally delegates to the precise `LmsrModel` implementation.
#[derive(Debug, Clone)]
pub struct LmsrPricer {
    model: LmsrModel,
}

impl LmsrPricer {
    /// Create a pricer with the given liquidity parameter.
    pub fn new(liquidity: f64) -> Self {
        let b = Decimal::from_f64(liquidity).unwrap_or(Decimal::ONE_HUNDRED);
        Self {
            model: LmsrModel::new(b),
        }
    }

    /// Compute fair price from an estimated probability.
    ///
    /// Maps probability → LMSR quantity split → YES price.
    /// For a simple probability-to-price, we use q_yes = b*ln(p/(1-p))
    /// which inverts the LMSR price formula.
    pub fn price(&self, estimated_prob: f64) -> f64 {
        // Clamp to avoid log(0) or log(inf)
        let p = estimated_prob.clamp(0.01, 0.99);
        // For a probability-based fair value, the LMSR price IS the probability
        // when the market is at equilibrium. The pricer returns the probability
        // as the fair value for the YES token.
        p
    }

    /// Detect edge as absolute difference.
    pub fn detect_edge(&self, market_price: f64, fair_price: f64) -> f64 {
        ((fair_price - market_price) / market_price).abs()
    }

    /// Access the underlying model for precise Decimal operations.
    pub fn model(&self) -> &LmsrModel {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_lmsr_equal_quantities_gives_half() {
        let model = LmsrModel::new(dec!(100.0));
        let price = model.price_yes(dec!(0.0), dec!(0.0));
        let diff = (price - dec!(0.5)).abs();
        assert!(diff < dec!(0.001), "Expected ~0.5, got {price}");
    }

    #[test]
    fn test_lmsr_prices_sum_to_one() {
        let model = LmsrModel::new(dec!(100.0));
        let p_yes = model.price_yes(dec!(50.0), dec!(30.0));
        let p_no = model.price_no(dec!(50.0), dec!(30.0));
        let sum = p_yes + p_no;
        let diff = (sum - Decimal::ONE).abs();
        assert!(diff < dec!(0.0001), "Prices must sum to 1, got {sum}");
    }

    #[test]
    fn test_lmsr_more_yes_shares_higher_price() {
        let model = LmsrModel::new(dec!(100.0));
        let p1 = model.price_yes(dec!(50.0), dec!(0.0));
        let p2 = model.price_yes(dec!(0.0), dec!(0.0));
        assert!(p1 > p2, "More YES shares should increase YES price");
    }

    #[test]
    fn test_cost_to_buy_positive() {
        let model = LmsrModel::new(dec!(100.0));
        let cost = model.cost_to_buy_yes(dec!(0.0), dec!(0.0), dec!(10.0));
        assert!(cost > Decimal::ZERO, "Cost to buy should be positive");
    }

    #[test]
    fn test_detect_edge() {
        let model = LmsrModel::new(dec!(100.0));
        let edge = model.detect_edge(dec!(0.40), dec!(0.50));
        assert!(edge > dec!(20.0), "Edge should be ~25%, got {edge}");
    }

   
