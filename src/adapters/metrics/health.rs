//! Health Check Server - Liveness and Readiness Probes
//!
//! Exposes /live and /ready endpoints via axum 0.7 for Docker
//! health checks and monitoring. Readiness depends on feed
//! connectivity and chain client health.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use tokio::sync::broadcast;
use tracing::{info, instrument};

/// Shared health state polled by readiness probes.
#[derive(Debug, Clone)]
pub struct HealthState {
    /// Whether feeds are connected.
    pub feeds_healthy: Arc<std::sync::atomic::AtomicBool>,
    /// Whether chain client is connected.
    pub chain_healthy: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the engine is running (not paused by circuit breaker).
    pub engine_running: Arc<std::sync::atomic::AtomicBool>,
}

impl HealthState {
    /// Create a new health state (all healthy by default).
    pub fn new() -> Self {
        Self {
            feeds_healthy: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            chain_healthy: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            engine_running: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        }
    }

    /// Check if the system is ready to serve traffic.
    pub fn is_ready(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.feeds_healthy.load(Ordering::Relaxed)
            && self.chain_healthy.load(Ordering::Relaxed)
    }
}

/// Axum-based health check HTTP server.
///
/// Serves liveness (/live) and readiness (/ready) endpoints for
/// Docker health checks and orchestrator probes.
pub struct HealthServer {
    /// Health state shared with all components.
    state: Arc<HealthState>,
    /// Bind port (default 8080 from config).
    port: u16,
}

impl HealthServer {
    /// Create a new health server.
    pub fn new(state: Arc<HealthState>, port: u16) -> Self {
        Self { state, port }
    }

    /// Start the health check server in the background.
    #[instrument(skip(self, shutdown_rx))]
    pub async fn run(
        self,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> anyhow::Result<()> {
        let app = Router::new()
            .route("/live", get(Self::liveness))
            .route("/ready", get(Self::readiness))
            .with_state(Arc::clone(&self.state));

        let addr = format!("0.0.0.0:{}", self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;

        info!(address = %addr, "Health server started");

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.recv().await;
            })
            .await?;

        Ok(())
    }

    /// Liveness probe: always returns 200 if the process is running.
    async fn liveness() -> impl IntoResponse {
        (StatusCode::OK, "OK")
    }

    /// Readiness probe: returns 200 only if feeds + chain are healthy.
    async fn readiness(
        State(state): State<Arc<HealthState>>,
    ) -> impl IntoResponse {
        if state.is_ready() {
            (StatusCode::OK, "READY")
        } else {
            (StatusCode::SERVICE_UNAVAILABLE, "NOT READY")
        }
    }
}
