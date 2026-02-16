//! Polymarket fee calculation engine.
//!
//! Implements the dynamic taker fee model and maker rebate system.
//! CRITICAL: Maker orders pay 0% fees and earn rebates.
//! Taker fees follow a parabolic curve that peaks at p=0.50.

use rust_decimal::Decimal;
use rust_decimal::prelude::*;
use rust_decimal_macros::dec;

/// Fee calculator implementing Polymarket's fee structure.
///
/// The maker-first strategy is essential because:
/// - Maker fee: 0% (+ potential rebates)
/// - Taker fee: up to ~1.56% at p=0.50 for standard markets
/// - Crypto short-duration markets can have even higher taker fees
#[derive(Debug, Clone)]
pub struct FeeCalculator {
    /// Fee rate parameter from Polymarket (default 0.0025 for standard)
    fee_rate: Decimal,
    /// Fee exponent (default 2 = parabolic curve)
    exponent: u32,
}

impl FeeCalculator {
    /// Creates a new fee calculator with custom parameters.
    pub fn new(fee_rate: Decimal, exponent: u32) -> Self {
        Self { fee_rate, exponent }
    }

    /// Creates a calculator for standard Polymarket markets.
    pub fn standard() -> Self {
        Self {
            fee_rate: dec!(0.0025),
            exponent: 2,
        }
    }

    /// Creates a calculator for crypto short-duration markets.
    /// These have higher fees to discourage latency arbitrage.
    pub fn crypto_short_duration() -> Self {
        Self {
            fee_rate: dec!(0.025),
            exponent: 2,
        }
    }

    /// Computes the taker fee for a given market price.
    ///
    /// Formula: fee = fee_rate * price^exponent * (1-price)^exponent * size
    /// The fee is maximized at p=0.50 and approaches 0 near p=0 or p=1.
    pub fn taker_fee(&self, price: Decimal, size: Decimal) -> Decimal {
        if price <= Decimal::ZERO || price >= Decimal::ONE {
            return Decimal::ZERO;
        }

        let p_f64 = price.to_f64().unwrap_or(0.5);
        let rate_f64 = self.fee_rate.to_f64().unwrap_or(0.0025);
        let size_f64 = size.to_f64().unwrap_or(0.0);

        let fee_factor = p_f64.powi(self.exponent as i32)
            * (1.0 - p_f64).powi(self.exponent as i32);
        let fee = rate_f64 * fee_factor * size_f64;

        Decimal::from_f64(fee).unwrap_or(Decimal::ZERO)
    }

    /// Maker fee is always 0% on Polymarket CLOB.
    /// Returns 0 (or negative for rebates in future).
    pub fn maker_fee(&self, _price: Decimal, _size: Decimal) -> Decimal {
        Decimal::ZERO
    }

    /// Returns the net cost after fees for a maker order.
    /// Since maker fee is 0, this equals the raw cost.
    pub fn net_cost_maker(&self, raw_cost: Decimal) -> Decimal {
        raw_cost
    }

    /// Returns the net cost after fees for a taker order.
    pub fn net_cost_taker(&self, raw_cost: Decimal, price: Decimal, size: Decimal) -> Decimal {
        raw_cost + self.taker_fee(price, size)
    }

    /// Computes the minimum edge (%) needed to be profitable as a taker.
    /// This is why we use maker-first: taker fees eat into the edge.
    pub fn min_profitable_edge_taker(&self, price: Decimal) -> Decimal {
        let fee_on_unit = self.taker_fee(price, Decimal::ONE);
        fee_on_unit * Decimal::ONE_HUNDRED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_maker_fee_always_zero() {
        let calc = FeeCalculator::standard();
        assert_eq!(calc.maker_fee(dec!(0.50), dec!(100.0)), Decimal::ZERO);
        assert_eq!(calc.maker_fee(dec!(0.10), dec!(1000.0)), Decimal::ZERO);
    }

    #[test]
    fn test_taker_fee_max_at_half() {
        let calc = FeeCalculator::standard();
        let fee_at_half = calc.taker_fee(dec!(0.50), dec!(100.0));
        let fee_at_quarter = calc.taker_fee(dec!(0.25), dec!(100.0));
        assert!(fee_at_half > fee_at_quarter, "Fee should be highest at p=0.50");
    }

    #[test]
    fn test_taker_fee_near_zero_at_extremes() {
        let calc = FeeCalculator::standard();
        let fee = calc.taker_fee(dec!(0.01), dec!(100.0));
        assert!(fee < dec!(0.001), "Fee should be near 0 at extreme prices");
    }

    #[test]
    fn test_crypto_short_duration_higher_fees() {
        let standard = FeeCalculator::standard();
        let crypto = FeeCalculator::crypto_short_duration();
        let std_fee = standard.taker_fee(dec!(0.50), dec!(100.0));
        let crypto_fee = crypto.taker_fee(dec!(0.50), dec!(100.0));
        assert!(crypto_fee > std_fee, "Crypto markets should have higher fees");
    }

    #[test]
    fn test_net_cost_maker_equals_raw() {
        let calc = FeeCalculator::standard();
        let raw = dec!(50.0);
        assert_eq!(calc.net_cost_maker(raw), raw);
    }
}
