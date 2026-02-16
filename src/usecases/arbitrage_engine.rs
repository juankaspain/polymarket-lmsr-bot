//! Arbitrage Engine - Core Pricing and Quoting Loop
//!
//! The main market-making use case that:
//! 1. Receives price updates via MarketFeed
//! 2. Computes LMSR fair values
//! 3. Detects edge after fees
//! 4. Sizes positions via quarter-Kelly
//! 5. Places maker-only GTC orders
//!
//! Event-driven architecture: reacts to order book changes,
//! not on a fixed polling interval.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use tokio::sync::broadcast;
use tracing::{debug, info, instrument, warn};

use crate::config::AppConfig;
use crate::domain::bayesian::BayesianEstimator;
use crate::domain::fees::FeeCalculator;
use crate::domain::kelly::KellySizer;
use crate::domain::lmsr::LmsrPricer;
use crate::ports::execution::OrderExecution;
use crate::ports::market_feed::{MarketFeed, PriceUpdate};

use super::order_manager::OrderManager;
use super::risk_manager::RiskManager;

/// Arbitrage engine orchestrating the full market-making loop.
pub struct ArbitrageEngine<F: MarketFeed, E: OrderExecution> {
  /// Market data feed.
  feed: Arc<F>,
  /// Order execution adapter.
  execution: Arc<E>,
  /// LMSR pricing model.
  pricer: LmsrPricer,
  /// Kelly position sizer.
  sizer: KellySizer,
  /// Fee calculator (maker = 0%).
  fees: FeeCalculator,
  /// Bayesian probability estimator.
  estimator: BayesianEstimator,
  /// Order manager for lifecycle.
  order_manager: OrderManager<E>,
  /// Risk manager for limits.
  risk_manager: RiskManager,
  /// Bot configuration.
  config: AppConfig,
  /// Shutdown signal receiver.
  shutdown_rx: broadcast::Receiver<()>,
}

impl<F: MarketFeed, E: OrderExecution> ArbitrageEngine<F, E> {
  /// Create a new arbitrage engine.
  pub fn new(
    feed: Arc<F>,
    execution: Arc<E>,
    config: AppConfig,
    shutdown_rx: broadcast::Receiver<()>,
  ) -> Self {
    let pricer = LmsrPricer::new(config.lmsr.liquidity_parameter);
    let sizer = KellySizer::new(config.lmsr.kelly_fraction);
    let fees = FeeCalculator::new_maker();
    let estimator = BayesianEstimator::new(config.lmsr.prior_weight);
    let order_manager = OrderManager::new(Arc::clone(&execution), &config);
    let risk_manager = RiskManager::new(&config.risk);

    Self {
      feed,
      execution,
      pricer,
      sizer,
      fees,
      estimator,
      order_manager,
      risk_manager,
      config,
      shutdown_rx,
    }
  }

  /// Run the main event loop.
  ///
  /// Subscribes to all configured markets and processes
  /// price updates as they arrive. Exits on shutdown signal.
  #[instrument(skip(self), name = "arbitrage_loop")]
  pub async fn run(&mut self) -> Result<()> {
    info!(
      markets = self.config.markets.len(),
      "Starting arbitrage engine"
    );

    // Subscribe to all active market tokens
    let active_markets: Vec<_> = self
      .config
      .markets
      .iter()
      .filter(|m| m.active)
      .collect();

    if active_markets.is_empty() {
      warn!("No active markets configured, engine idle");
      return Ok(());
    }

    // Create subscriptions for YES tokens
    let token_ids: Vec<_> = active_markets
      .iter()
      .map(|m| m.yes_token_id.clone())
      .collect();

    let mut receivers = self.feed.subscribe_many(&token_ids);

    info!(
      subscriptions = receivers.len(),
      "Subscribed to market feeds"
    );

    // Main event loop
    loop {
      tokio::select! {
        // Check for shutdown signal
        _ = self.shutdown_rx.recv() => {
          info!("Shutdown signal received, stopping engine");
          break;
        }
        // Process price updates from any market
        update = Self::next_update(&mut receivers) => {
          if let Some(price_update) = update {
            if let Err(e) = self.process_update(&price_update).await {
              warn!(
                error = %e,
                token = %price_update.token_id,
                "Error processing price update"
              );
            }
          }
        }
      }
    }

    Ok(())
  }

  /// Process a single price update.
  ///
  /// Core logic: LMSR fair value -> edge detection -> Kelly sizing -> order.
  #[instrument(skip(self, update), fields(token = %update.token_id))]
  async fn process_update(&mut self, update: &PriceUpdate) -> Result<()> {
    let start = Instant::now();

    // 1. Extract mid-price
    let mid = match update.mid_price {
      Some(p) if p > 0.0 && p < 1.0 => p,
      _ => {
        debug!("Skipping update: no valid mid-price");
        return Ok(());
      }
    };

    // 2. Update Bayesian estimate
    let estimated_prob = self.estimator.update(mid);

    // 3. Compute LMSR fair value
    let fair_value = self.pricer.price(estimated_prob);

    // 4. Calculate edge after fees
    let edge = if let Some(best_ask) = update.best_ask {
      let buy_edge = self.fees.net_edge(fair_value, best_ask, true);
      buy_edge
    } else {
      0.0
    };

    // 5. Check minimum edge threshold
    if edge.abs() < self.config.lmsr.min_edge {
      debug!(
        edge = edge,
        min = self.config.lmsr.min_edge,
        "Edge below threshold, skipping"
      );
      return Ok(());
    }

    // 6. Risk check
    if !self.risk_manager.can_trade() {
      warn!("Risk limits reached, trade blocked");
      return Ok(());
    }

    // 7. Kelly sizing
    let bankroll = self
      .execution
      .available_balance(crate::domain::trade::TradeSide::Buy)
      .await?;

    let kelly_size = self
      .sizer
      .optimal_size(estimated_prob, fair_value, bankroll);

    if kelly_size < 1.0 {
      debug!(size = kelly_size, "Kelly size too small");
      return Ok(());
    }

    // 8. Place maker order
    let latency = start.elapsed();
    info!(
      fair_value = fair_value,
      edge = edge,
      size = kelly_size,
      latency_us = latency.as_micros(),
      "Placing maker order"
    );

    self
      .order_manager
      .place_maker_order(
        &update.token_id,
        fair_value,
        kelly_size,
        edge > 0.0,
      )
      .await?;

    Ok(())
  }

  /// Get the next price update from any receiver.
  async fn next_update(
    receivers: &mut [broadcast::Receiver<PriceUpdate>],
  ) -> Option<PriceUpdate> {
    if receivers.is_empty() {
      tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
      return None;
    }

    // Poll all receivers, return first available
    for rx in receivers.iter_mut() {
      match rx.try_recv() {
        Ok(update) => return Some(update),
        Err(_) => continue,
      }
    }

    // No updates available, brief sleep to avoid busy-wait
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    None
  }
}
