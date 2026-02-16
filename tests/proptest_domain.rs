//! Property-Based Tests — Domain Layer Invariants
//!
//! Uses `proptest` to verify that domain components maintain
//! mathematical invariants across random inputs.

use proptest::prelude::*;

use polymarket_lmsr_bot::domain::fees::FeeCalculator;
use polymarket_lmsr_bot::domain::kelly::KellySizer;
use polymarket_lmsr_bot::domain::lmsr::LmsrPricer;

// ── LMSR Pricer Properties ──────────────────────────────────

proptest! {
    /// LMSR prices must always be in (0, 1) for valid probabilities.
    #[test]
    fn lmsr_price_always_in_unit_interval(
        b in 1.0f64..1000.0,
        p in 0.01f64..0.99,
    ) {
        let pricer = LmsrPricer::new(b);
        let price = pricer.price(p);
        prop_assert!(price > 0.0, "LMSR price must be > 0, got {price}");
        prop_assert!(price < 1.0, "LMSR price must be < 1, got {price}");
    }

    /// LMSR prices must be monotonically increasing with probability.
    #[test]
    fn lmsr_price_monotonically_increasing(
        b in 10.0f64..500.0,
        p1 in 0.05f64..0.45,
        delta in 0.05f64..0.45,
    ) {
        let p2 = (p1 + delta).min(0.95);
        let pricer = LmsrPricer::new(b);
        let price1 = pricer.price(p1);
        let price2 = pricer.price(p2);
        prop_assert!(
            price2 >= price1,
            "LMSR must be monotonic: p({p1})={price1} > p({p2})={price2}"
        );
    }
}

// ── Fee Calculator Properties ───────────────────────────────

proptest! {
    /// Maker fees should always be zero.
    #[test]
    fn maker_fee_always_zero(p in 0.01f64..0.99) {
        let fees = FeeCalculator::new_maker();
        let fee = fees.maker_fee(p);
        prop_assert!(
            fee.abs() < f64::EPSILON,
            "Maker fee should be 0, got {fee}"
        );
    }

    /// Taker fee must be non-negative and <= 1.56% (max at p=0.50).
    #[test]
    fn taker_fee_bounded(p in 0.01f64..0.99) {
        let fees = FeeCalculator::new_taker();
        let fee = fees.taker_fee(p);
        prop_assert!(fee >= 0.0, "Taker fee must be >= 0, got {fee}");
        // Max fee = 0.25 × 0.5² × 0.5² = 0.015625 ≈ 1.56%
        prop_assert!(
            fee <= 0.016,
            "Taker fee must be <= 1.56%, got {}",
            fee * 100.0
        );
    }

    /// Net edge after maker fees should equal gross edge (0% fee).
    #[test]
    fn maker_net_edge_equals_gross(
        fair in 0.1f64..0.9,
        market in 0.1f64..0.9,
    ) {
        let fees = FeeCalculator::new_maker();
        let net = fees.net_edge(fair, market, true);
        let gross = fair - market;
        prop_assert!(
            (net - gross).abs() < 1e-10,
            "Maker net edge should equal gross: net={net}, gross={gross}"
        );
    }
}

// ── Kelly Sizer Properties ──────────────────────────────────

proptest! {
    /// Kelly size must be non-negative.
    #[test]
    fn kelly_size_non_negative(
        fraction in 0.1f64..1.0,
        prob in 0.05f64..0.95,
        fair in 0.05f64..0.95,
        bankroll in 100.0f64..10000.0,
    ) {
        let sizer = KellySizer::new(fraction);
        let size = sizer.optimal_size(prob, fair, bankroll);
        prop_assert!(
            size >= 0.0,
            "Kelly size must be >= 0, got {size}"
        );
    }

    /// Kelly size must never exceed bankroll.
    #[test]
    fn kelly_size_bounded_by_bankroll(
        fraction in 0.1f64..1.0,
        prob in 0.05f64..0.95,
        fair in 0.05f64..0.95,
        bankroll in 100.0f64..10000.0,
    ) {
        let sizer = KellySizer::new(fraction);
        let size = sizer.optimal_size(prob, fair, bankroll);
        prop_assert!(
            size <= bankroll,
            "Kelly size {size} exceeds bankroll {bankroll}"
        );
    }

    /// Quarter-Kelly should be ≤ full-Kelly for same inputs.
    #[test]
    fn quarter_kelly_less_than_full(
        prob in 0.1f64..0.9,
        fair in 0.1f64..0.9,
        bankroll in 100.0f64..5000.0,
    ) {
        let full = KellySizer::new(1.0);
        let quarter = KellySizer::new(0.25);
        let full_size = full.optimal_size(prob, fair, bankroll);
        let quarter_size = quarter.optimal_size(prob, fair, bankroll);
        prop_assert!(
            quarter_size <= full_size + 1e-10,
            "Quarter-Kelly {quarter_size} > full-Kelly {full_size}"
        );
    }
}
