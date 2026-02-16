//! Polymarket CLOB API Adapter
//!
//! Implements the HTTP client for interacting with the Polymarket
//! Central Limit Order Book (CLOB) REST API. Handles authentication,
//! order placement, cancellation, and order book queries.
//!
//! Sub-modules:
//! - `auth`: EIP-712 signature-based authentication
//! - `client`: HTTP client with rate limiting and retries
//! - `orderbook`: Order book snapshot retrieval
//! - `orders`: Order placement and management
//! - `types`: API request/response type definitions

pub mod auth;
pub mod client;
pub mod orderbook;
pub mod orders;
pub mod types;
