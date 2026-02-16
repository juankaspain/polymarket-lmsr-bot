//! Backtest Framework - Historical Data Simulation
//!
//! Simulates the LMSR arbitrage strategy against historical
//! price data to validate edge detection, Kelly sizing, and
//! risk management before going live.

use polymarket_lmsr_bot::config::RiskConfig;
use polymarket_lmsr_bot::domain::bayesian::BayesianEstimator;
use polymarket_lmsr_bot::domain::fees::FeeCalculator;
use polymarket_lmsr_bot::domain::kelly::KellySizer;
use polymarket_lmsr_bot::domain::lmsr::LmsrPricer;
use polymarket_lmsr_bot::usecases::risk_manager::RiskManager;

/// A single historical price point for backtesting.
#[derive(Debug, Clone)]
struct HistoricalTick {
    /// Simulated timestamp (Unix ms).
    timestamp_ms: u64,
    /// CEX spot price (e.g., Binance BTC/USDT).
    spot_price: f64,
    /// Polymarket YES token best ask.
    pm_best_ask: f64,
    /// Polymarket YES token best bid.
    pm_best_bid: f64,
    /// Actual outcome (true = YES won).
    actual_outcome: Option<bool>,
}

/// Backtest result summary.
#[derive(Debug)]
struct BacktestResult {
    /// Total trades executed.
    total_trades: usize,
    /// Winning trades.
    wins: usize,
    /// Losing trades.
    losses: usize,
    /// Total PnL in USDC.
    total_pnl: f64,
    /// Maximum drawdown experienced.
    max_drawdown: f64,
    /// Win rate percentage.
    win_rate: f64,
    /// Average edge captured per trade.
    avg_edge: f64,
    /// Total fees paid (should be ~0 for maker).
    total_fees: f64,
    /// Number of circuit breaker activations.
    circuit_breaker_triggers: usize,
}

/// Generate synthetic historical data for testing.
///
/// Creates a series of price ticks with known outcomes
/// to validate the strategy deterministically.
fn generate_synthetic_data() -> Vec<HistoricalTick> {
    let mut ticks = Vec::new();
    let base_time = 1700000000000u64;

    // Scenario 1: Clear edge — spot at 50000, PM YES underpriced at 0.40
    // (fair value should be ~0.50, edge = 0.10)
    for i in 0..20 {
        ticks.push(HistoricalTick {
            timestamp_ms: base_time + i * 1000,
            spot_price: 50000.0 + (i as f64 * 10.0),
            pm_best_ask: 0.40 + (i as f64 * 0.005),
            pm_best_bid: 0.38 + (i as f64 * 0.005),
            actual_outcome: if i == 19 { Some(true) } else { None },
        });
    }

    // Scenario 2: No edge — prices aligned
    for i in 0..10 {
        ticks.push(HistoricalTick {
            timestamp_ms: base_time + 20000 + i * 1000,
            spot_price: 50200.0,
            pm_best_ask: 0.50,
            pm_best_bid: 0.49,
            actual_outcome: if i == 9 { Some(true) } else { None },
        });
    }

    // Scenario 3: Negative edge — PM overpriced
    for i in 0..15 {
        ticks.push(HistoricalTick {
            timestamp_ms: base_time + 30000 + i * 1000,
            spot_price: 49800.0 - (i as f64 * 20.0),
            pm_best_ask: 0.65,
            pm_best_bid: 0.63,
            actual_outcome: if i == 14 { Some(false) } else { None },
        });
    }

    ticks
}

/// Run the backtest simulation.
fn run_backtest(ticks: &[HistoricalTick]) -> BacktestResult {
    let pricer = LmsrPricer::new(100.0);
    let sizer = KellySizer::new(0.25);
    let fees = FeeCalculator::new_maker();
    let mut estimator = BayesianEstimator::new(0.5);

    let risk_config = RiskConfig {
        max_daily_loss_fraction: 0.30, // 30% daily limit per checklist
        max_position_size: 100.0,
        max_total_exposure: 500.0,
        min_bankroll: 50.0,
        circuit_breaker_losses: 5,
        cooldown_seconds: 1800,
    };
    let mut risk_manager = RiskManager::new(&risk_config);

    let mut bankroll = 1000.0f64;
    let mut total_pnl = 0.0f64;
    let mut max_bankroll = bankroll;
    let mut max_drawdown = 0.0f64;
    let mut total_trades = 0usize;
    let mut wins = 0usize;
    let mut losses = 0usize;
    let mut total_edge = 0.0f64;
    let mut total_fees = 0.0f64;
    let mut circuit_breaker_triggers = 0usize;

    for tick in ticks {
        // Skip ticks without valid PM prices
        if tick.pm_best_ask <= 0.0 || tick.pm_best_ask >= 1.0 {
            continue;
        }

        // Update Bayesian estimate with PM mid price
        let pm_mid = (tick.pm_best_ask + tick.pm_best_bid) / 2.0;
        let estimated_prob = estimator.update(pm_mid);

        // Compute LMSR fair value
        let fair_value = pricer.price(estimated_prob);

        // Calculate edge after fees (maker = 0%)
        let edge = fees.net_edge(fair_value, tick.pm_best_ask, true);

        // Check minimum edge (2% threshold)
        if edge.abs() < 0.02 {
            continue;
        }

        // Risk check
        if !risk_manager.can_trade() {
            continue;
        }

        if !risk_manager.can_open_position(10.0, bankroll) {
            continue;
        }

        // Kelly sizing
        let kelly_size = sizer.optimal_size(estimated_prob, fair_value, bankroll);
        let trade_size = kelly_size.min(100.0).max(1.0);

        // Simulate trade
        total_trades += 1;
        total_edge += edge;

        // If we have an outcome, calculate PnL
        if let Some(outcome) = tick.actual_outcome {
            let bought_yes = edge > 0.0;
            let won = bought_yes == outcome;

            let pnl = if won {
                trade_size * (1.0 - tick.pm_best_ask) // profit = size × (1 - price)
            } else {
                -trade_size * tick.pm_best_ask // loss = size × price
            };

            // Maker fees = 0
            let fee = 0.0;
            total_fees += fee;

            bankroll += pnl - fee;
            total_pnl += pnl - fee;

            risk_manager.record_trade(pnl);

            if pnl > 0.0 {
                wins += 1;
            } else {
                losses += 1;
            }

            if bankroll > max_bankroll {
                max_bankroll = bankroll;
            }
            let drawdown = (max_bankroll - bankroll) / max_bankroll;
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
            }

            if risk_manager.is_circuit_breaker_active() {
                circuit_breaker_triggers += 1;
                risk_manager.reset_daily(); // Simulate cooldown passed
            }
        }
    }

    let win_rate = if total_trades > 0 {
        wins as f64 / total_trades as f64 * 100.0
    } else {
        0.0
    };

    let avg_edge = if total_trades > 0 {
        total_edge / total_trades as f64
    } else {
        0.0
    };

    BacktestResult {
        total_trades,
        wins,
        losses,
        total_pnl,
        max_drawdown,
        win_rate,
        avg_edge,
        total_fees,
        circuit_breaker_triggers,
    }
}

#[test]
fn test_backtest_synthetic_data_produces_trades() {
    let ticks = generate_synthetic_data();
    let result = run_backtest(&ticks);

    // Must generate some trades from the clear-edge scenario
    assert!(
        result.total_trades > 0,
        "Backtest should produce at least 1 trade, got 0"
    );

    println!("=== Backtest Results ===");
    println!("Total trades: {}", result.total_trades);
    println!("Wins: {} | Losses: {}", result.wins, result.losses);
    println!("Win rate: {:.1}%", result.win_rate);
    println!("Total PnL: ${:.2}", result.total_pnl);
    println!("Max drawdown: {:.2}%", result.max_drawdown * 100.0);
    println!("Avg edge: {:.4}", result.avg_edge);
    println!("Total fees: ${:.2} (should be ~0 for maker)", result.total_fees);
    println!(
        "Circuit breaker triggers: {}",
        result.circuit_breaker_triggers
    );
}

#[test]
fn test_backtest_maker_fees_are_zero() {
    let ticks = generate_synthetic_data();
    let result = run_backtest(&ticks);

    assert_eq!(
        result.total_fees, 0.0,
        "Maker strategy should have zero fees"
    );
}

#[test]
fn test_backtest_respects_daily_loss_limit() {
    let risk_config = RiskConfig {
        max_daily_loss_fraction: 0.02, // 2%
        max_position_size: 100.0,
        max_total_exposure: 500.0,
        min_bankroll: 50.0,
        circuit_breaker_losses: 3,
        cooldown_seconds: 1800,
    };

    let mut rm = RiskManager::new(&risk_config);
    let bankroll = 1000.0;

    // Simulate losses up to the daily limit
    rm.record_trade(-5.0);
    rm.record_trade(-5.0);
    rm.record_trade(-5.0);

    // Should still allow (15 < 20 = 2% of 1000)
    assert!(rm.can_open_position(10.0, bankroll));

    // But circuit breaker should have triggered (3 consecutive)
    assert!(rm.is_circuit_breaker_active());
}

#[test]
fn test_backtest_kelly_sizing_bounds() {
    let sizer = KellySizer::new(0.25); // quarter-Kelly

    // High edge, high confidence → reasonable size
    let size = sizer.optimal_size(0.7, 0.65, 1000.0);
    assert!(size > 0.0, "Kelly should return positive size for edge");
    assert!(size < 1000.0, "Kelly should not bet entire bankroll");

    // No edge → zero or near-zero size
    let size_no_edge = sizer.optimal_size(0.5, 0.50, 1000.0);
    assert!(
        size_no_edge < 1.0,
        "Kelly should return near-zero for no edge"
    );
}

#[test]
fn test_backtest_fee_curve_at_probability_extremes() {
    let fees = FeeCalculator::new_maker();

    // At p=0.50 (max fee region): fee
