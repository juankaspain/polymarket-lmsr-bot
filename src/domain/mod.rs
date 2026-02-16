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
pub use bayesian::BayesianEstimator;
pub use fees::FeeCalculator;
pub use kelly::KellyCriterion;
pub use lmsr::LmsrModel;
pub use trade::{
    Asset, BotMode, Market, Order, OrderSide, OrderStatus, OrderType, Position,
    Trade, TradeSide,
};
