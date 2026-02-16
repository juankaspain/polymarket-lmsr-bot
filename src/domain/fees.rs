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
        if p
