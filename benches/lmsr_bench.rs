//! LMSR Pricing Benchmarks — Hot-Path Performance Validation
//!
//! Benchmarks the core domain functions that run on every price update.
//! Target: < 10ms feed-to-order (checklist requirement).
//!
//! Run with: cargo bench --bench lmsr_bench

use criterion::{black_box, criterion_group, criterion_main, Criterion};

use polymarket_lmsr_bot::domain::lmsr::LmsrModel;
use polymarket_lmsr_bot::domain::kelly::KellyCriterion;
use polymarket_lmsr_bot::domain::fees::FeeCalculator;
use polymarket_lmsr_bot::domain::bayesian::BayesianEstimator;

/// Benchmark LMSR price computation for a binary market.
fn bench_lmsr_price(c: &mut Criterion) {
    let model = LmsrModel::new(100.0);

    c.bench_function("lmsr_price_binary", |b| {
        b.iter(|| {
            let _price = model.price(black_box(60.0), black_box(40.0));
        });
    });
}

/// Benchmark LMSR cost function (buy 10 shares).
fn bench_lmsr_cost(c: &mut Criterion) {
    let model = LmsrModel::new(100.0);

    c.bench_function("lmsr_cost_10_shares", |b| {
        b.iter(|| {
            let _cost = model.cost(
                black_box(60.0),
                black_box(40.0),
                black_box(10.0),
                black_box(true),
            );
        });
    });
}

/// Benchmark Kelly criterion position sizing.
fn bench_kelly_size(c: &mut Criterion) {
    let kelly = KellyCriterion::new(0.25, 0.20);

    c.bench_function("kelly_quarter_size", |b| {
        b.iter(|| {
            let _size = kelly.optimal_size(
                black_box(0.55),
                black_box(0.50),
                black_box(1000.0),
            );
        });
    });
}

/// Benchmark fee calculation at various probability points.
fn bench_fee_calc(c: &mut Criterion) {
    let fee_calc = FeeCalculator::new_taker();

    c.bench_function("fee_calc_taker", |b| {
        b.iter(|| {
            let _fee = fee_calc.calculate_fee(black_box(0.50));
        });
    });
}

/// Benchmark Bayesian EWMA probability update.
fn bench_bayesian_update(c: &mut Criterion) {
    let mut estimator = BayesianEstimator::new(rust_decimal::Decimal::new(7, 1));

    c.bench_function("bayesian_ewma_update", |b| {
        b.iter(|| {
            let _est = estimator.update(black_box(0.55));
        });
    });
}

criterion_group!(
    benches,
    bench_lmsr_price,
    bench_lmsr_cost,
    bench_kelly_size,
    bench_fee_calc,
    bench_bayesian_update,
);
criterion_main!(benches);
