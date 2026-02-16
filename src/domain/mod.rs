//! Domain layer - Core business logic and models.
//!
//! This module contains the pure domain logic for the Polymarket LMSR bot.
//! No external dependencies allowed here (hexagonal architecture inner ring).
//! All types are serializable and testable in isolation.

pub mod bayesian;
pub mod fees;
pub mod kelly;
pub mod lmsr;
pub mod trade;

// Re-export core types for convenience
pub use trade::{Asset, Market, Order, OrderSide, OrderStatus, OrderType, Position, Trade};
pub use lmsr::LmsrModel;
pub use kelly::KellyCriterion;
pub use fees::FeeCalculator;
pub use bayesian::BayesianEstimator;
