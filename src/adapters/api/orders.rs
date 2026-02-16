//! CLOB Order Execution Adapter - Polymarket REST API
//!
//! Implements the `OrderExecution` port for the Polymarket CLOB.
//! All orders are maker-only (GTC + post-only) to guarantee 0% fees + rebates.
//! Uses reqwest with rustls for HTTPS, API key + secret from env vars.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use governor::{Quota, RateLimiter};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, instrument, warn};

use crate::config::ApiConfig;
use crate::domain::trade::{Order, OrderType, TradeSide, TokenId, OrderId};
use crate::ports::execution::{
    OrderCancellation, OrderExecution, OrderPlacement, OrderStatus,
};

/// CLOB API request for placing an order.
#[derive(Debug, Serialize)]
struct PlaceOrderRequest {
    token_id: String,
    price: f64,
    size: f64,
    side: String,
    #[serde(rename = "type")]
    order_type: String,
    /// Always true for maker-first strategy.
    post_only: bool,
    /// GTD expiration in seconds (90s per checklist).
    expiration: Option<u64>,
}

/// CLOB API response from order placement.
#[derive(Debug, Deserialize)]
struct PlaceOrderResponse {
    #[serde(rename = "orderID")]
    order_id: String,
    success: bool,
    #[serde(default)]
    error_msg: Option<String>,
    #[serde(default)]
    timestamp_ms: Option<u64>,
}

/// CLOB API response for order status query.
#[derive(Debug, Deserialize)]
struct OrderStatusResponse {
    status: String,
    #[serde(default)]
    original_size: Option<f64>,
    #[serde(default)]
    remaining_size: Option<f64>,
    #[serde(default)]
    filled_size: Option<f64>,
    #[serde(default)]
    avg_price: Option<f64>,
}

/// CLOB API response for cancel.
#[derive(Debug, Deserialize)]
struct CancelOrderResponse {
    success: bool,
    #[serde(default)]
    error_msg: Option<String>,
}

/// Polymarket CLOB order execution adapter.
///
/// Connects to the Polymarket CLOB REST API for order lifecycle
/// management. Enforces maker-only placement and rate limiting.
pub struct ClobOrderExecutor {
    /// HTTP client with rustls TLS backend.
    client: Client,
    /// CLOB base URL from config.
    base_url: String,
    /// API key from environment.
    api_key: String,
    /// API secret from environment.
    api_secret: String,
    /// Rate limiter: 50 orders/min budget (limit=60 actual).
    rate_limiter: Arc<RateLimiter<
        governor::state::NotKeyed,
        governor::state::InMemoryState,
        governor::clock::DefaultClock,
    >>,
    /// Rolling order count for budget tracking.
    orders_this_minute: AtomicU32,
}

impl ClobOrderExecutor {
    /// Create a new CLOB executor from config and env credentials.
    ///
    /// Reads `POLYMARKET_API_KEY` and `POLYMARKET_API_SECRET` from
    /// environment variables. Panics if not set.
    pub fn new(config: &ApiConfig) -> Result<Self> {
        let api_key = std::env::var("POLYMARKET_API_KEY")
            .context("POLYMARKET_API_KEY not set")?;
        let api_secret = std::env::var("POLYMARKET_API_SECRET")
            .context("POLYMARKET_API_SECRET not set")?;

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_seconds))
            .build()
            .context("Failed to build HTTP client")?;

        // 50 orders per 60 seconds budget (API hard limit is 60)
        let quota = Quota::per_minute(std::num::NonZeroU32::new(50).unwrap());
        let rate_limiter = Arc::new(RateLimiter::direct(quota));

        Ok(Self {
            client,
            base_url: config.clob_url.clone(),
            api_key,
            api_secret,
            rate_limiter,
            orders_this_minute: AtomicU32::new(0),
        })
    }

    /// Build authorization headers for CLOB API.
    fn auth_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "POLY-API-KEY",
            self.api_key.parse().unwrap_or_default(),
        );
        headers.insert(
            "POLY-API-SECRET",
            self.api_secret.parse().unwrap_or_default(),
        );
        headers
    }

    /// Get current epoch millis for timestamps.
    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

#[async_trait]
impl OrderExecution for ClobOrderExecutor {
    #[instrument(skip(self, order), fields(token = %order.token_id, price = order.price, size = order.size))]
    async fn place_order(&self, order: &Order) -> Result<OrderPlacement> {
        // Rate limit enforcement
        self.rate_limiter.until_ready().await;

        let side_str = match order.side {
            TradeSide::Buy => "BUY",
            TradeSide::Sell => "SELL",
        };

        // GTD with 90s expiration per checklist (NEVER GTC)
        let request = PlaceOrderRequest {
            token_id: order.token_id.clone(),
            price: order.price,
            size: order.size,
            side: side_str.to_string(),
            order_type: "GTD".to_string(),
            post_only: true,
            expiration: Some(90),
        };

        let url = format!("{}/order", self.base_url);

        let response = self
            .client
            .post(&url)
            .headers(self.auth_headers())
            .json(&request)
            .send()
            .await
            .context("CLOB place_order request failed")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            bail!("CLOB place_order HTTP {status}: {body}");
        }

        let resp: PlaceOrderResponse = response
            .json()
            .await
            .context("Failed to parse place_order response")?;

        self.orders_this_minute.fetch_add(1, Ordering::Relaxed);

        Ok(OrderPlacement {
            order_id: resp.order_id,
            accepted: resp.success,
            rejection_reason: resp.error_msg,
            timestamp_ms: resp.timestamp_ms.unwrap_or_else(Self::now_ms),
        })
    }

    #[instrument(skip(self), fields(order_id = %order_id))]
    async fn cancel_order(&self, order_id: &OrderId) -> Result<OrderCancellation> {
        let url = format!("{}/order/{}", self.base_url, order_id);

        let response = self
            .client
            .delete(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .context("CLOB cancel_order request failed")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!(http_status = %status, "Cancel order failed: {body}");
            return Ok(OrderCancellation {
                order_id: order_id.clone(),
                success: false,
                error: Some(format!("HTTP {status}: {body}")),
            });
        }

        let resp: CancelOrderResponse = response
            .json()
            .await
            .context("Failed to parse cancel_order response")?;

        Ok(OrderCancellation {
            order_id: order_id.clone(),
            success: resp.success,
            error: resp.error_msg,
        })
    }

    #[instrument(skip(self))]
    async fn cancel_all_orders(&self) -> Result<usize> {
        let url = format!("{}/orders/cancel-all", self.base_url);

        let response = self
            .client
            .delete(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .context("CLOB cancel_all request failed")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            bail!("CLOB cancel_all HTTP {status}: {body}");
        }

        #[derive(Deserialize)]
        struct CancelAllResponse {
            cancelled: usize,
        }

        let resp: CancelAllResponse = response.json().await?;
        info!(cancelled = resp.cancelled, "All orders cancelled");
        Ok(resp.cancelled)
    }

    #[instrument(skip(self), fields(token_id = %token_id))]
    async fn cancel_orders_for_token(
        &self,
        token_id: &TokenId,
    ) -> Result<Vec<OrderCancellation>> {
        // Get open orders for this token, then cancel each
        let open = self.get_open_orders().await?;
        let mut results = Vec::new();

        for order in open.iter().filter(|o| o.token_id == *token_id) {
            let result = self.cancel_order(&order.id).await?;
            results.push(result);
        }

        Ok(results)
    }

    #[instrument(skip(self), fields(order_id = %order_id))]
    async fn get_order_status(&self, order_id: &OrderId) -> Result<OrderStatus> {
        let url = format!("{}/order/{}", self.base_url, order_id);

        let response = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .context("CLOB get_order_status request failed")?;

        let resp: OrderStatusResponse = response
            .json()
            .await
            .context("Failed to parse order status")?;

        let status = match resp.status.as_str() {
            "OPEN" | "LIVE" => OrderStatus::Open {
                remaining_size: resp.remaining_size.unwrap_or(0.0),
                original_size: resp.original_size.unwrap_or(0.0),
            },
            "FILLED" | "MATCHED" => OrderStatus::Filled {
                avg_price: resp.avg_price.unwrap_or(0.0),
                filled_size: resp.filled_size.unwrap_or(0.0),
            },
            "PARTIALLY_FILLED" => OrderStatus::PartiallyFilled {
                filled_size: resp.filled_size.unwrap_or(0.0),
                remaining_size: resp.remaining_size.unwrap_or(0.0),
                avg_price: resp.avg_price.unwrap_or(0.0),
            },
            "CANCELLED" | "EXPIRED" => OrderStatus::Cancelled,
            _ => OrderStatus::Unknown,
        };

        Ok(status)
    }

    #[instrument(skip(self))]
    async fn get_open_orders(&self) -> Result<Vec<Order>> {
        let url = format!("{}/orders/open", self.base_url);

        let response = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .context("CLOB get_open_orders request failed")?;

        #[derive(Deserialize)]
        struct OpenOrder {
            #[serde(rename = "orderID")]
            order_id: String,
            token_id: String,
            side: String,
            price: f64,
            size: f64,
            #[serde(default)]
            timestamp_ms: u64,
        }

        let orders: Vec<OpenOrder> = response.json().await?;

        Ok(orders
            .into_iter()
            .map(|o| Order {
                id: o.order_id,
                token_id: o.token_id,
                side: if o.side == "BUY" {
                    TradeSide::Buy
                } else {
                    TradeSide::Sell
                },
                price: o.price,
                size: o.size,
                order_type: OrderType::Gtc,
                post_only: true,
                timestamp_ms: o.timestamp_ms,
            })
            .collect())
    }

    #[instrument(skip(self))]
    async fn available_balance(&self, _side: TradeSide) -> Result<f64> {
        let url = format!("{}/balance", self.base_url);

        let response = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .context("CLOB balance query failed")?;

        #[derive(Deserialize)]
        struct BalanceResponse {
            balance: f64,
        }

        let resp: BalanceResponse = response.json().await?;
        Ok(resp.balance)
    }

    async fn is_healthy(&self) -> bool {
        let url = format!("{}/health", self.base_url);
        self.client
            .get(&url)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    async fn rate_limit_status(&self) -> (u32, u64) {
        let used = self.orders_this_minute.load(Ordering::Relaxed);
        let remaining = 50u32.saturating_sub(used);
        (remaining, Self::now_ms() + 60_000)
    }
}
