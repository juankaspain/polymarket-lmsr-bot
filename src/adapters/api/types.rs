//! CLOB API Request/Response Types
//!
//! Defines the serialization types for communicating with the
//! Polymarket CLOB REST API. All types derive Serialize/Deserialize
//! for JSON transport.

use serde::{Deserialize, Serialize};

/// Order request payload for the CLOB API.
#[derive(Debug, Clone, Serialize)]
pub struct CreateOrderRequest {
  /// Token ID to trade.
  pub token_id: String,
  /// Price in USDC (0.01 - 0.99).
  pub price: f64,
  /// Size in contracts.
  pub size: f64,
  /// "BUY" or "SELL".
  pub side: String,
  /// Fee rate basis points (0 for maker).
  pub fee_rate_bps: u32,
  /// Nonce for replay protection.
  pub nonce: u64,
  /// Expiration timestamp (0 = GTC).
  pub expiration: u64,
  /// EIP-712 signature.
  pub signature: String,
  /// Signer address.
  pub maker: String,
  /// Order type: "GTC" or "GTD".
  pub order_type: String,
}

/// Response from order creation.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateOrderResponse {
  /// Whether the order was accepted.
  pub success: bool,
  /// Assigned order ID.
  #[serde(rename = "orderID")]
  pub order_id: Option<String>,
  /// Error message if rejected.
  #[serde(rename = "errorMsg")]
  pub error_msg: Option<String>,
  /// Server timestamp.
  pub timestamp: Option<u64>,
}

/// Cancel order request.
#[derive(Debug, Clone, Serialize)]
pub struct CancelOrderRequest {
  /// Order ID to cancel.
  #[serde(rename = "orderID")]
  pub order_id: String,
}

/// Cancel response.
#[derive(Debug, Clone, Deserialize)]
pub struct CancelOrderResponse {
  /// Whether cancellation succeeded.
  pub success: bool,
  /// Error message if failed.
  #[serde(rename = "errorMsg")]
  pub error_msg: Option<String>,
}

/// Cancel all orders response.
#[derive(Debug, Clone, Deserialize)]
pub struct CancelAllResponse {
  /// Number of orders cancelled.
  pub cancelled: usize,
}

/// Order book level from the API.
#[derive(Debug, Clone, Deserialize)]
pub struct OrderBookLevel {
  /// Price at this level.
  pub price: String,
  /// Total size at this level.
  pub size: String,
}

/// Order book response from the API.
#[derive(Debug, Clone, Deserialize)]
pub struct OrderBookResponse {
  /// Bid levels (price descending).
  pub bids: Vec<OrderBookLevel>,
  /// Ask levels (price ascending).
  pub asks: Vec<OrderBookLevel>,
  /// Market hash / token ID.
  pub hash: Option<String>,
  /// Timestamp of snapshot.
  pub timestamp: Option<String>,
}

/// Open order info from the API.
#[derive(Debug, Clone, Deserialize)]
pub struct OpenOrderInfo {
  /// CLOB order ID.
  pub id: String,
  /// Token ID.
  pub asset_id: String,
  /// "BUY" or "SELL".
  pub side: String,
  /// Original order price.
  pub price: String,
  /// Original size.
  pub original_size: String,
  /// Remaining unfilled size.
  pub size_matched: String,
  /// Order status.
  pub status: String,
  /// Creation timestamp.
  pub created_at: Option<String>,
}

/// API error response.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiError {
  /// Error message.
  pub error: Option<String>,
  /// Error code.
  pub code: Option<u32>,
}

/// Authentication token response.
#[derive(Debug, Clone, Deserialize)]
pub struct AuthTokenResponse {
  /// API key / token.
  pub token: String,
  /// Expiration timestamp.
  pub expires_at: Option<u64>,
}

/// Rate limit info from response headers.
#[derive(Debug, Clone)]
pub struct RateLimitInfo {
  /// Remaining requests in current window.
  pub remaining: u32,
  /// Window reset time (Unix ms).
  pub reset_ms: u64,
  /// Maximum requests per window.
  pub limit: u32,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_create_order_request_serialization() {
    let req = CreateOrderRequest {
      token_id: "token_123".to_string(),
      price: 0.55,
      size: 10.0,
      side: "BUY".to_string(),
      fee_rate_bps: 0,
      nonce: 12345,
      expiration: 0,
      signature: "0xsig".to_string(),
      maker: "0xaddr".to_string(),
      order_type: "GTC".to_string(),
    };

    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("token_123"));
    assert!(json.contains("BUY"));
  }

  #[test]
  fn test_create_order_response_deserialization() {
    let json = r#"{"success": true, "orderID": "order_abc", "timestamp": 1234567890}"#;
    let resp: CreateOrderResponse = serde_json::from_str(json).unwrap();
    assert!(resp.success);
    assert_eq!(resp.order_id.unwrap(), "order_abc");
  }
}
