//! Arbitrage Engine — Core Pricing and Quoting Loop
//!
//! The main market-making use case that:
//! 1. Receives price updates via `MarketFeed` broadcast channels
//! 2. Computes LMSR fair values
//! 3. Detects edge after fees (maker = 0%)
//! 4. Sizes positions via quarter-Kelly
//! 5. Places maker-only orders via `OrderExecution` port
//!
//! Architecture: event-driven via `tokio::select!` over broadcast
//! receivers. NEVER polls on interval, NEVER uses `try_recv()`.

use std::sync::Arc;
use std::task::Poll;
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

/// Internal event type for the engine select loop.
enum FeedEvent {
    /// A price update from any subscribed market.
    Update(PriceUpdate),
    /// Shutdown signal received.
    Shutdown,
    /// Receiver lagged and dropped messages.
    Lagged(u64),
}

/// Arbitrage engine orchestrating the full market-making loop.
pub struct ArbitrageEngine<F: MarketFeed, E: OrderExecution> {
    /// Market data feed (port).
    feed: Arc<F>,
    /// Order execution adapter (port).
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
    /// Risk manager for limits and circuit breakers.
    risk_manager: RiskManager,
    /// Bot configuration.
    config: AppConfig,
    /// Shutdown signal receiver.
    shutdown_rx: broadcast::Receiver<()>,
}

impl<F: MarketFeed, E: OrderExecution> ArbitrageEngine<F, E> {
    /// Create a new arbitrage engine with all domain components wired.
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
    /// Subscribes to all configured markets and processes price updates
    /// as they arrive via `tokio::select!` — pure event-driven, NEVER polling.
    /// Exits cleanly on shutdown signal.
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

        // Create subscriptions for YES tokens via MarketFeed port
        let token_ids: Vec<_> = active_markets
            .iter()
            .map(|m| m.yes_token_id.clone())
            .collect();

        let mut receivers = self.feed.subscribe_many(&token_ids);

        info!(
            subscriptions = receivers.len(),
            "Subscribed to market feeds"
        );

        // Main event loop — tokio::select! with biased shutdown priority
        loop {
            let event = recv_first_event(
                &mut receivers,
                &mut self.shutdown_rx,
            )
            .await;

            match event {
                FeedEvent::Shutdown => {
                    info!("Shutdown signal received, stopping engine");
                    break;
                }
                FeedEvent::Update(price_update) => {
                    if let Err(e) = self.process_update(&price_update).await {
                        warn!(
                            error = %e,
                            token = %price_update.token_id,
                            "Error processing price update"
                        );
                    }
                }
                FeedEvent::Lagged(count) => {
                    warn!(
                        dropped = count,
                        "Receiver lagged, some updates were dropped"
                    );
                }
            }
        }

        Ok(())
    }

    /// Process a single price update.
    ///
    /// Core pipeline: mid-price → Bayesian estimate → LMSR fair value
    /// → edge detection → risk check → Kelly sizing → maker order.
    #[instrument(skip(self, update), fields(token = %update.token_id))]
    async fn process_update(&mut self, update: &PriceUpdate) -> Result<()> {
        let start = Instant::now();

        // 1. Extract mid-price (must be valid probability range)
        let mid = match update.mid_price {
            Some(p) if p > 0.0 && p < 1.0 => p,
            _ => {
                debug!("Skipping update: no valid mid-price");
                return Ok(());
            }
        };

        // 2. Update Bayesian EWMA estimate
        let estimated_prob = self.estimator.update(mid);

        // 3. Compute LMSR fair value
        let fair_value = self.pricer.price(estimated_prob);

        // 4. Calculate edge after fees (maker fee = 0)
        let edge = if let Some(best_ask) = update.best_ask {
            self.fees.net_edge(fair_value, best_ask, true)
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

        // 6. Risk check (circuit breaker, daily loss, exposure)
        if !self.risk_manager.can_trade() {
            warn!("Risk limits reached, trade blocked");
            return Ok(());
        }

        // 7. Kelly sizing against current bankroll
        let bankroll = self
            .execution
            .available_balance(crate::domain::trade::TradeSide::Buy)
            .await?;

        let kelly_size = self
            .sizer
            .optimal_size(estimated_prob, fair_value, bankroll);

        if kelly_size < 1.0 {
            debug!(size = kelly_size, "Kelly size too small, skipping");
            return Ok(());
        }

        // 8. Place maker order
        let latency = start.elapsed();
        info!(
            fair_value = fair_value,
            edge = edge,
            size = kelly_size,
            latency_us = latency.as_micros(),
            "Signal detected — placing maker order"
        );

        self.order_manager
            .place_maker_order(
                &update.token_id,
                fair_value,
                kelly_size,
                edge > 0.0,
            )
            .await?;

        Ok(())
    }
}

/// Receive the first available event from any market feed receiver OR shutdown.
///
/// Uses `tokio::select!` with biased shutdown priority and a `poll_fn` that
/// races all broadcast receivers concurrently. This is the correct event-driven
/// pattern: no `try_recv()`, no `sleep()`, no busy-wait.
///
/// The `poll_fn` approach is idiomatic for a dynamic number of futures and
/// has zero overhead — each receiver's waker is registered with the tokio
/// runtime and only woken when data arrives.
async fn recv_first_event(
    receivers: &mut [broadcast::Receiver<PriceUpdate>],
    shutdown_rx: &mut broadcast::Receiver<()>,
) -> FeedEvent {
    use tokio::sync::broadcast::error::RecvError;

    if receivers.is_empty() {
        // No market subscriptions — just wait for shutdown
        let _ = shutdown_rx.recv().await;
        return FeedEvent::Shutdown;
    }

    // Race shutdown against all market receivers using tokio::select!
    // The inner poll_fn registers wakers for ALL receivers so the runtime
    // wakes us on the first available message from any channel.
    tokio::select! {
        biased;

        // Shutdown always wins (biased = checked first)
        _ = shutdown_rx.recv() => {
            FeedEvent::Shutdown
        }

        // Race all market feed receivers via poll_fn
        event = std::future::poll_fn(|cx| {
            for rx in receivers.iter_mut() {
                // Pin the recv future and poll it once.
                // broadcast::Receiver::recv() is cancel-safe, so
                // polling and dropping is fine.
                let mut recv_fut = std::pin::pin!(rx.recv());
                match recv_fut.as_mut().poll(cx) {
                    Poll::Ready(Ok(update)) => {
                        return Poll::Ready(FeedEvent::Update(update));
                    }
                    Poll::Ready(Err(RecvError::Lagged(n))) => {
                        return Poll::Ready(FeedEvent::Lagged(n));
                    }
                    Poll::Ready(Err(RecvError::Closed)) => {
                        // Channel closed — skip this receiver, try next
                        continue;
                    }
                    Poll::Pending => {
                        // Waker registered — will be notified when data arrives
                        continue;
                    }
                }
            }
            // All receivers are Pending — we'll be woken by any of them
            Poll::Pending
        }) => {
            event
        }
    }
}
