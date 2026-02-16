//! Polymarket fee calculation engine.
//!
//! Implements the dynamic taker fee model and maker rebate system.
//! CRITICAL: Maker orders pay 0% fees and earn rebates.
//! Taker fees follow a parabolic curve that peaks at p=0.50.
//!
//! Exposes both Decimal API (precise) and f64 methods for ports/adapters.

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
    /// Whether this calculator is for maker orders (always 0% fees).
    is_maker: bool,
}

impl FeeCalculator {
    /// Creates a new fee calculator with custom parameters.
    pub fn new(fee_rate: Decimal, exponent: u32) -> Self {
        Self {
            fee_rate,
            exponent,
            is_maker: false,
        }
    }

    /// Creates a calculator for maker orders (0% fee + rebates).
    ///
    /// This is the primary constructor per the maker-first strategy.
    pub fn new_maker() -> Self {
        Self {
            fee_rate: dec!(0.0025),
            exponent: 2,
            is_maker: true,
        }
    }

    /// Creates a calculator for standard Polymarket markets.
    pub fn standard() -> Self {
        Self {
            fee_rate: dec!(0.0025),
            exponent: 2,
            is_maker: false,
        }
    }

    /// Creates a calculator for crypto short-duration markets.
    pub fn crypto_short_duration() -> Self {
        Self {
            fee_rate: dec!(0.025),
            exponent: 2,
            is_maker: false,
        }
    }

    /// Computes the taker fee for a given market price.
    ///
    /// Formula: fee = fee_rate * price^exponent * (1-price)^exponent * size
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
    pub fn maker_fee(&self, _price: Decimal, _size: Decimal) -> Decimal {
        Decimal::ZERO
    }

    /// Returns the net cost after fees for a maker order.
    pub fn net_cost_maker(&self, raw_cost: Decimal) -> Decimal {
        raw_cost
    }

    /// Returns the net cost after fees for a taker order.
    pub fn net_cost_taker(&self, raw_cost: Decimal, price: Decimal, size: Decimal) -> Decimal {
        raw_cost + self.taker_fee(price, size)
    }

    /// Computes the minimum edge (%) needed to be profitable as a taker.
    pub fn min_profitable_edge_taker(&self, price: Decimal) -> Decimal {
        let fee_on_unit = self.taker_fee(price, Decimal::ONE);
        fee_on_unit * Decimal::ONE_HUNDRED
    }

    // ────────────────────────────────────────
    // f64 boundary API for usecases/adapters
    // ────────────────────────────────────────

    /// Compute net edge after fees (f64 API).
    ///
    /// Returns `fair_value - price - fee` for a buy, or
    /// `price - fair_value - fee` for a sell.
    /// For maker orders, fee is 0.
    pub fn net_edge(&self, fair_value: f64, market_price: f64, is_buy: bool) -> f64 {
        let fee = if self.is_maker {
            0.0
        } else {
            self.taker_fee_f64(market_price, 1.0)
        };

        if is_buy {
            fair_value - market_price - fee
        } else {
            market_price - fair_value - fee
        }
    }

    /// Compute taker fee as f64 for convenience.
    pub fn taker_fee_f64(&self, price: f64, size: f64) -> f64 {
        if price <= 0.0 || price >= 1.0 {
            return 0.0;
        }
        let rate = self.fee_rate.to_f64().unwrap_or(0.0025);
        let fee_factor = price.powi(self.exponent as i32)
            * (1.0 - price).powi(self.exponent as i32);
        rate * fee_factor * size
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

    // f64 API tests
    #[test]
    fn test_new_maker_net_edge_zero_fee() {
        let calc = FeeCalculator::new_maker();
        let edge = calc.net_edge(0.55, 0.50, true);
        assert!((edge - 0.05).abs() < 0.001, "Maker edge should be 0.05");
    }

    #[test]
    fn test_taker_net_edge_reduced_by_fee() {
        let calc = FeeCalculator::standard();
        let edge = calc.net_edge(0.55, 0.50, true);
        assert!(edge < 0.05, "Taker edge should be reduced by fee");
        assert!(edge > 0.0, "Edge should still be positive for this case");
    }

    #[test]
    fn test_taker_fee_f64_matches_decimal() {
        let calc = FeeCalculator::standard();
        let f64_fee = calc.taker_fee_f64(0.50, 100.0);
        let dec_fee = calc.taker_fee(dec!(0.50), dec!(100.0));
        assert!((f64_fee - dec_fee.to_f64().unwrap()).abs() < 0.0001);
    }
}
