//! Bayesian probability estimator for fair price computation.
//!
//! Combines multiple price feeds to estimate the true probability
//! that a prediction market outcome will resolve YES.
//! Uses exponential weighted moving average (EWMA) for feed fusion.
//!
//! Exposes both a multi-source Decimal API and a simplified f64 API.

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
    /// Simplified smoothed probability for f64 API.
    smoothed_prob_f64: Option<f64>,
    /// Alpha as f64 for the simplified path.
    alpha_f64: f64,
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
        let alpha_f64 = alpha.to_f64().unwrap_or(0.7);
        Self {
            prices: HashMap::new(),
            alpha,
            smoothed_price: None,
            smoothed_prob_f64: None,
            alpha_f64,
        }
    }

    /// Updates the estimate with a new price tick from a named source (Decimal API).
    pub fn update_source(&mut self, source: &str, price: Decimal) {
        self.prices.insert(source.to_string(), price);
        self.recalculate();
    }

    /// Simplified update with a single f64 probability observation.
    ///
    /// Used by `ArbitrageEngine` which receives mid-prices as `f64`.
    /// Returns the current smoothed probability estimate.
    pub fn update(&mut self, observation: f64) -> f64 {
        let smoothed = match self.smoothed_prob_f64 {
            Some(prev) => prev * (1.0 - self.alpha_f64) + observation * self.alpha_f64,
            None => observation,
        };
        self.smoothed_prob_f64 = Some(smoothed);
        smoothed
    }

    /// Returns the current fused price estimate (Decimal API).
    pub fn current_price(&self) -> Option<Decimal> {
        self.smoothed_price
    }

    /// Returns the current f64 probability estimate.
    pub fn current_prob_f64(&self) -> Option<f64> {
        self.smoothed_prob_f64
    }

    /// Estimates the fair probability for a binary market (Decimal API).
    ///
    /// For "Will asset be above strike at time T?":
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

        let distance_bps = ((current - strike_price) / strike_price)
            * dec!(10000.0);

        let x = distance_bps.to_f64()? / volatility_bps.to_f64()?;
        let prob = 1.0 / (1.0 + (-x).exp());

        let result = Decimal::from_f64(prob)?;
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

        let sum: Decimal = self.prices.values().copied().sum();
        let count = Decimal::from(self.prices.len() as u64);
        let avg = sum / count;

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
        est.update_source("binance", dec!(50000.0));
        assert_eq!(est.current_price(), Some(dec!(50000.0)));
        assert_eq!(est.source_count(), 1);
    }

    #[test]
    fn test_multi_source_fusion() {
        let mut est = BayesianEstimator::new(dec!(1.0));
        est.update_source("binance", dec!(50000.0));
        est.update_source("coinbase", dec!(50100.0));
        let price = est.current_price().unwrap();
        assert!(price > dec!(50000.0) && price < dec!(50100.0));
    }

    #[test]
    fn test_probability_above_strike() {
        let mut est = BayesianEstimator::new(dec!(1.0));
        est.update_source("binance", dec!(50500.0));
        let prob = est.estimate_probability(dec!(50000.0), dec!(50.0)).unwrap();
        assert!(prob > dec!(0.5), "Price above strike should give >50% prob");
    }

    #[test]
    fn test_probability_below_strike() {
        let mut est = BayesianEstimator::new(dec!(1.0));
        est.update_source("binance", dec!(49500.0));
        let prob = est.estimate_probability(dec!(50000.0), dec!(50.0)).unwrap();
        assert!(prob < dec!(0.5), "Price below strike should give <50% prob");
    }

    #[test]
    fn test_probability_clamped() {
        let mut est = BayesianEstimator::new(dec!(1.0));
        est.update_source("binance", dec!(55000.0));
        let prob = est.estimate_probability(dec!(50000.0), dec!(50.0)).unwrap();
        assert!(prob <= dec!(0.99), "Probability should be clamped to 0.99");
    }

    #[test]
    fn test_ewma_smoothing() {
        let mut est = BayesianEstimator::new(dec!(0.5));
        est.update_source("binance", dec!(100.0));
        est.update_source("binance", dec!(200.0));
        let price = est.current_price().unwrap();
        assert_eq!(price, dec!(150.0));
    }

    // f64 API tests
    #[test]
    fn test_f64_update_first_observation() {
        let mut est = BayesianEstimator::default();
        let result = est.update(0.55);
        assert!((result - 0.55).abs() < 0.001);
    }

    #[test]
    fn test_f64_update_smoothing() {
        let mut est = BayesianEstimator::new(dec!(0.5));
        est.update(0.40);
        let result = est.update(0.60);
        // EWMA: 0.40 * 0.5 + 0.60 * 0.5 = 0.50
        assert!((result - 0.50).abs() < 0.001);
    }
}
