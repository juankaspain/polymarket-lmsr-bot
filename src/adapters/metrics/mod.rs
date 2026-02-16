//! Metrics and Monitoring Adapters
//!
//! Provides Prometheus metrics export on :9090 and health check
//! endpoints (/live, /ready) via axum 0.7. Follows the observability
//! checklist with JSON tracing spans.

pub mod health;
pub mod prometheus;

pub use health::HealthServer;
pub use prometheus::MetricsRegistry;
