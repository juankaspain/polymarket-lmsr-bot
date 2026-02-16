//! Bayesian probability estimator for fair price computation.
//!
//! Combines multiple price feeds to estimate the true probability
//! that a prediction market outcome will resolve YES.
//! Uses exponential weighted moving average (EWMA) for feed fusion.

use rust_decimal::Decimal;
use rust_decimal::prelude::*;
use rust_decimal_macros::dec;
use std::collections::HashMap;

use super::trade::Asset;

/// Bayesian estimator that fuses multiple price feeds into a fair probability.
///
/// For 5-minute BTC/ETH markets like "Will BTC be above $X at time T?",
/// the fair probability is derived from the current spot price relative
/// to the strike price, adjusted by recent volatility.
#[derive(Debug, Clone)]
pub struct BayesianEstimator {
    /// Latest prices from each feed source
    prices: HashMap<String, Decimal>,
    /// EWMA smoothing factor (0 < alpha <= 1)
    alpha: Decimal,
    /// Current smoothed estimate
    smoothed_price: Option<Decimal>,
}

impl BayesianEstimator {
    /// Creates a new estimator with the given EWMA alpha.
    ///
    /// Alpha controls responsiveness:
    /// - Higher alpha (0.8-1.0) = more responsive to recent ticks
    /// - Lower alpha (0.1-0.3) = smoother, less noise
    pub fn new(alpha: Decimal) -> Self {
        assert!(
            alpha > Decimal::ZERO && alpha <= Decimal::ONE,
            "Alpha must be in (0, 1]"
        );
        Self {
            prices: HashMap::new(),
            alpha,
            smoothed_price: None,
        }
    }

    /// Updates the estimate with a new price tick from a feed source.
    pub fn update(&mut self, source: &str, price: Decimal) {
        self.prices.insert(source.to_string(), price);
        self.recalculate();
    }

    /// Returns the current fused price estimate.
    pub fn current_price(&self) -> Option<Decimal> {
        self.smoothed_price
    }

    /// Estimates the fair probability for a binary market.
    ///
    /// For "Will asset be above strike at time T?":
    /// - If current_price >> strike: probability approaches 1.0
    /// - If current_price << strike: probability approaches 0.0
    /// - If current_price ~= strike: probability ~0.50
    ///
    /// Uses a simplified logistic model scaled by the asset's typical
    /// 5-minute volatility.
    pub fn estimate_probability(
        &self,
        strike_price: Decimal,
        volatility_bps: Decimal,
    ) -> Option<Decimal> {
        let current = self.smoothed_price?;

        if strike_price <= Decimal::ZERO || volatility_bps <= Decimal::ZERO {
            return None;
        }

        // Distance from strike in basis points
        let distance_bps = ((current - strike_price) / strike_price)
            * dec!(10000.0);

        // Logistic function: 1 / (1 + exp(-distance / volatility))
        let x = distance_bps.to_f64()? / volatility_bps.to_f64()?;
        let prob = 1.0 / (1.0 + (-x).exp());

        let result = Decimal::from_f64(prob)?;
        // Clamp to valid probability range
        Some(result.max(dec!(0.01)).min(dec!(0.99)))
    }

    /// Returns the number of active feed sources.
    pub fn source_count(&self) -> usize {
        self.prices.len()
    }

    /// Recalculates the fused estimate from all sources using EWMA.
    fn recalculate(&mut self) {
        if self.prices.is_empty() {
            return;
        }

        // Simple average of all current feed prices as the raw estimate
        let sum: Decimal = self.prices.values().copied().sum();
        let count = Decimal::from(self.prices.len() as u64);
        let avg = sum / count;

        // Apply EWMA smoothing
        self.smoothed_price = Some(match self.smoothed_price {
            Some(prev) => prev * (Decimal::ONE - self.alpha) + avg * self.alpha,
            None => avg,
        });
    }
}

impl Default for BayesianEstimator {
    fn default() -> Self {
        Self::new(dec!(0.7))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_source_update() {
        let mut est = BayesianEstimator::default();
        est.update("binance", dec!(50000.0));
        assert_eq!(est.current_price(), Some(dec!(50000.0)));
        assert_eq!(est.source_count(), 1);
    }

    #[test]
    fn test_multi_source_fusion() {
        let mut est = BayesianEstimator::new(dec!(1.0)); // no smoothing
        est.update("binance", dec!(50000.0));
        est.update("coinbase", dec!(50100.0));
        let price = est.current_price().unwrap();
        assert!(price > dec!(50000.0) && price < dec!(50100.0));
    }

    #[test]
    fn test_probability_above_strike() {
        let mut est = BayesianEstimator::new(dec!(1.0));
        est.update("binance", dec!(50500.0));
        let prob = est.estimate_probability(dec!(50000.0), dec!(50.0)).unwrap();
        assert!(prob > dec!(0.5), "Price above strike should give >50% prob");
    }

    #[test]
    fn test_probability_below_strike() {
        let mut est = BayesianEstimator::new(dec!(1.0));
        est.update("binance", dec!(49500.0));
        let prob = est.estimate_probability(dec!(50000.0), dec!(50.0)).unwrap();
        assert!(prob < dec!(0.5), "Price below strike should give <50% prob");
    }

    #[test]
    fn test_probability_clamped() {
        let mut est = BayesianEstimator::new(dec!(1.0));
        est.update("binance", dec!(55000.0));
        let prob = est.estimate_probability(dec!(50000.0), dec!(50.0)).unwrap();
        assert!(prob <= dec!(0.99), "Probability should be clamped to 0.99");
    }

    #[test]
    fn test_ewma_smoothing() {
        let mut est = BayesianEstimator::new(dec!(0.5));
        est.update("binance", dec!(100.0));
        est.update("binance", dec!(200.0));
        let price = est.current_price().unwrap();
        // With alpha=0.5: smoothed = 100*0.5 + 200*0.5 = 150
        assert_eq!(price, dec!(150.0));
    }
}
