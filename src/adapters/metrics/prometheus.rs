//! Prometheus Metrics Registry - Trading Observability
//!
//! Registers and exposes Prometheus metrics on :9090 for Grafana
//! dashboards. Covers trading latency, PnL, order counts, gas costs,
//! and feed health.

use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use prometheus::{
    Encoder, GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, Opts,
    Registry, TextEncoder,
};
use tokio::sync::broadcast;
use tracing::{info, instrument};

/// Centralized Prometheus metrics for the trading bot.
///
/// All metrics follow the naming convention `polymarket_bot_*` and
/// include asset labels for multi-asset filtering.
pub struct MetricsRegistry {
    /// Prometheus registry.
    registry: Registry,
    /// Order placement latency histogram (microseconds).
    pub order_latency_us: HistogramVec,
    /// Total orders placed counter.
    pub orders_placed: IntCounterVec,
    /// Total orders cancelled counter.
    pub orders_cancelled: IntCounterVec,
    /// Total orders rejected counter.
    pub orders_rejected: IntCounterVec,
    /// Current realized PnL gauge.
    pub realized_pnl: GaugeVec,
    /// Current unrealized PnL gauge.
    pub unrealized_pnl: GaugeVec,
    /// USDC balance gauge.
    pub usdc_balance: GaugeVec,
    /// Gas price gauge (gwei).
    pub gas_price_gwei: prometheus::Gauge,
    /// Feed connection status (1 = connected, 0 = disconnected).
    pub feed_connected: GaugeVec,
    /// Edge captured per trade histogram.
    pub edge_captured: HistogramVec,
    /// Circuit breaker status gauge (1 = active).
    pub circuit_breaker_active: prometheus::Gauge,
}

impl MetricsRegistry {
    /// Create and register all Prometheus metrics.
    pub fn new() -> anyhow::Result<Self> {
        let registry = Registry::new();

        let order_latency_us = HistogramVec::new(
            HistogramOpts::new(
                "polymarket_bot_order_latency_us",
                "Order placement latency in microseconds",
            )
            .buckets(vec![
                100.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 50000.0,
            ]),
            &["asset", "side"],
        )?;

        let orders_placed = IntCounterVec::new(
            Opts::new("polymarket_bot_orders_placed_total", "Total orders placed"),
            &["asset", "side"],
        )?;

        let orders_cancelled = IntCounterVec::new(
            Opts::new(
                "polymarket_bot_orders_cancelled_total",
                "Total orders cancelled",
            ),
            &["asset"],
        )?;

        let orders_rejected = IntCounterVec::new(
            Opts::new(
                "polymarket_bot_orders_rejected_total",
                "Total orders rejected by CLOB",
            ),
            &["asset", "reason"],
        )?;

        let realized_pnl = GaugeVec::new(
            Opts::new(
                "polymarket_bot_realized_pnl_usdc",
                "Cumulative realized PnL in USDC",
            ),
            &["asset"],
        )?;

        let unrealized_pnl = GaugeVec::new(
            Opts::new(
                "polymarket_bot_unrealized_pnl_usdc",
                "Current unrealized PnL in USDC",
            ),
            &["asset"],
        )?;

        let usdc_balance = GaugeVec::new(
            Opts::new(
                "polymarket_bot_usdc_balance",
                "Current USDC wallet balance",
            ),
            &["wallet_type"],
        )?;

        let gas_price_gwei = prometheus::Gauge::new(
            "polymarket_bot_gas_price_gwei",
            "Current Polygon gas price in gwei",
        )?;

        let feed_connected = GaugeVec::new(
            Opts::new(
                "polymarket_bot_feed_connected",
                "Feed connection status (1=connected, 0=disconnected)",
            ),
            &["source"],
        )?;

        let edge_captured = HistogramVec::new(
            HistogramOpts::new(
                "polymarket_bot_edge_captured",
                "Edge captured per trade (price vs fair value)",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.02, 0.05, 0.10]),
            &["asset"],
        )?;

        let circuit_breaker_active = prometheus::Gauge::new(
            "polymarket_bot_circuit_breaker_active",
            "Whether circuit breaker is active (1=yes, 0=no)",
        )?;

        // Register all metrics
        registry.register(Box::new(order_latency_us.clone()))?;
        registry.register(Box::new(orders_placed.clone()))?;
        registry.register(Box::new(orders_cancelled.clone()))?;
        registry.register(Box::new(orders_rejected.clone()))?;
        registry.register(Box::new(realized_pnl.clone()))?;
        registry.register(Box::new(unrealized_pnl.clone()))?;
        registry.register(Box::new(usdc_balance.clone()))?;
        registry.register(Box::new(gas_price_gwei.clone()))?;
        registry.register(Box::new(feed_connected.clone()))?;
        registry.register(Box::new(edge_captured.clone()))?;
        registry.register(Box::new(circuit_breaker_active.clone()))?;

        Ok(Self {
            registry,
            order_latency_us,
            orders_placed,
            orders_cancelled,
            orders_rejected,
            realized_pnl,
            unrealized_pnl,
            usdc_balance,
            gas_price_gwei,
            feed_connected,
            edge_captured,
            circuit_breaker_active,
        })
    }

    /// Serve Prometheus metrics on the configured bind address.
    #[instrument(skip(self, shutdown_rx))]
    pub async fn serve(
        self: Arc<Self>,
        bind_address: String,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> anyhow::Result<()> {
        let metrics_self = Arc::clone(&self);

        let app = Router::new().route(
            "/metrics",
            get(move || {
                let registry = metrics_self.registry.clone();
                async move {
                    let encoder = TextEncoder::new();
                    let metric_families = registry.gather();
                    let mut buffer = Vec::new();
                    encoder.encode(&metric_families, &mut buffer).unwrap();
                    String::from_utf8(buffer).unwrap_or_default()
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind(&bind_address).await?;
        info!(address = %bind_address, "Prometheus metrics server started");

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.recv().await;
            })
            .await?;

        Ok(())
    }
}
