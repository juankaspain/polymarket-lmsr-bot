//! CLOB Order Executor — Adapter for Order Placement
//!
//! Implements the `OrderExecution` port using the shared `ClobClient`
//! for authenticated requests. All orders use maker-first strategy
//! (GTC + post-only) for 0% fees + rebates.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use tracing::{debug, info, instrument, warn};

use super::client::ClobClient;
use super::orderbook::OrderBookAdapter;
use crate::domain::trade::{Order, OrderId, TokenId, TradeSide};
use crate::ports::execution::{
    OrderCancellation, OrderExecution, OrderPlacement, OrderStatus,
};

/// Maximum slippage tolerance before skipping trade (checklist: 2%).
const MAX_SLIPPAGE_PCT: f64 = 2.0;

/// CLOB order executor backed by the shared authenticated client.
///
/// Uses `ClobClient` for all HTTP requests (inherits HMAC auth,
/// retry logic, and rate limiting). Never creates its own reqwest client.
pub struct ClobOrderExecutor {
    /// Shared CLOB client with auth + retry.
    client: Arc<ClobClient>,
    /// Order book adapter for pre-trade slippage checks.
    orderbook: OrderBookAdapter,
    /// Orders placed this minute for rate tracking.
    orders_this_minute: AtomicU32,
    /// Last minute reset timestamp.
    minute_reset: std::sync::Mutex<Instant>,
}

impl ClobOrderExecutor {
    /// Create a new order executor.
    pub fn new(client: Arc<ClobClient>) -> Self {
        Self {
            orderbook: OrderBookAdapter::new(Arc::clone(&client)),
            client,
            orders_this_minute: AtomicU32::new(0),
            minute_reset: std::sync::Mutex::new(Instant::now()),
        }
    }

    /// Reset the per-minute order counter if a minute has elapsed.
    fn maybe_reset_minute_counter(&self) {
        let mut reset = self.minute_reset.lock().unwrap();
        if reset.elapsed().as_secs() >= 60 {
            self.orders_this_minute.store(0, Ordering::Relaxed);
            *reset = Instant::now();
        }
    }

    /// Check orderbook depth and slippage before trade (checklist requirement).
    ///
    /// Returns Ok(avg_fill_price) if slippage is acceptable, Err if >2%.
    async fn check_slippage(
        &self,
        token_id: &str,
        side: TradeSide,
        size: f64,
    ) -> Result<f64> {
        let book = self
            .orderbook
            .get_order_book(token_id)
            .await
            .context("Failed to fetch orderbook for slippage check")?;

        let levels = match side {
            TradeSide::Buy => &book.asks,
            TradeSide::Sell => &book.bids,
        };

        if levels.is_empty() {
            bail!("No orderbook depth for {token_id} on {:?} side", side);
        }

        // Compute weighted average fill price
        let mut remaining = size;
        let mut total_cost = 0.0;

        for level in levels {
            let fill = remaining.min(level.size);
            total_cost += fill * level.price;
            remaining -= fill;
            if remaining <= 0.0 {
                break;
            }
        }

        if remaining > 0.0 {
            bail!(
                "Insufficient orderbook depth: {:.2} unfilled of {:.2}",
                remaining,
                size
            );
        }

        let avg_fill = total_cost / size;
        let best_price = levels[0].price;
        let slippage_pct = ((avg_fill - best_price) / best_price).abs() * 100.0;

        if slippage_pct > MAX_SLIPPAGE_PCT {
            bail!(
                "Slippage {:.2}% exceeds {:.1}% threshold",
                slippage_pct,
                MAX_SLIPPAGE_PCT,
            );
        }

        Ok(avg_fill)
    }
}

#[async_trait]
impl OrderExecution for ClobOrderExecutor {
    #[instrument(skip(self, order), fields(token = %order.token_id, price = order.price, size = order.size))]
    async fn place_order(&self, order: &Order) -> Result<OrderPlacement> {
        // Reset minute counter if needed
        self.maybe_reset_minute_counter();

        // Rate limit check (budget: 50 ord/min, hard limit 60)
        let current = self.orders_this_minute.load(Ordering::Relaxed);
        if current >= 50 {
            warn!(current, "Order rate limit reached (50/min)");
            return Ok(OrderPlacement {
                order_id: String::new(),
                accepted: false,
                rejection_reason: Some("Rate limit: 50 orders/min".to_string()),
                timestamp_ms: 0,
            });
        }

        // Pre-trade slippage check (checklist: check_orderbook_depth BEFORE trade)
        if let Err(e) = self.check_slippage(&order.token_id, order.side, order.size).await {
            warn!(error = %e, "Slippage check failed, skipping order");
            return Ok(OrderPlacement {
                order_id: String::new(),
                accepted: false,
                rejection_reason: Some(format!("Slippage: {e}")),
                timestamp_ms: 0,
            });
        }

        // Build order payload — all orders are GTC + post-only (maker)
        let side_str = match order.side {
            TradeSide::Buy => "BUY",
            TradeSide::Sell => "SELL",
        };

        let payload = serde_json::json!({
            "tokenID": order.token_id,
            "price": format!("{:.2}", order.price),
            "size": format!("{:.2}", order.size),
            "side": side_str,
            "type": "GTC",
            "postOnly": true,
        });

        let body = serde_json::to_string(&payload)?;

        // Use shared ClobClient which handles HMAC auth automatically
        let response = self
            .client
            .post("/order", &body)
            .await
            .context("Failed to place order via CLOB")?;

        // Parse response
        let order_id = response["orderID"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let accepted = response["success"].as_bool().unwrap_or(false);
        let rejection_reason = if !accepted {
            response["errorMsg"].as_str().map(String::from)
        } else {
            None
        };

        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        if accepted {
            self.orders_this_minute.fetch_add(1, Ordering::Relaxed);
            info!(order_id = %order_id, "Order placed successfully");
        } else {
            warn!(reason = ?rejection_reason, "Order rejected by CLOB");
        }

        Ok(OrderPlacement {
            order_id,
            accepted,
            rejection_reason,
            timestamp_ms,
        })
    }

    #[instrument(skip(self))]
    async fn cancel_order(&self, order_id: &OrderId) -> Result<OrderCancellation> {
        let payload = serde_json::json!({ "orderID": order_id });
        let body = serde_json::to_string(&payload)?;

        let response = self
            .client
            .delete("/order", &body)
            .await
            .context("Failed to cancel order")?;

        let success = response["success"].as_bool().unwrap_or(false);

        Ok(OrderCancellation {
            order_id: order_id.clone(),
            success,
            error: if !success {
                response["errorMsg"].as_str().map(String::from)
            } else {
                None
            },
        })
    }

    #[instrument(skip(self))]
    async fn cancel_all_orders(&self) -> Result<usize> {
        let response = self
            .client
            .delete("/order/all", "")
            .await
            .context("Failed to cancel all orders")?;

        let count = response["cancelled"]
            .as_u64()
            .unwrap_or(0) as usize;

        info!(cancelled = count, "Cancelled all open orders");
        Ok(count)
    }

    async fn cancel_orders_for_token(
        &self,
        token_id: &TokenId,
    ) -> Result<Vec<OrderCancellation>> {
        let payload = serde_json::json!({ "tokenID": token_id });
        let body = serde_json::to_string(&payload)?;

        let response = self
            .client
            .delete("/order/token", &body)
            .await
            .context("Failed to cancel orders for token")?;

        let cancelled = response["cancelled"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|id| OrderCancellation {
                        order_id: id.to_string(),
                        success: true,
                        error: None,
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(cancelled)
    }

    async fn get_order_status(&self, order_id: &OrderId) -> Result<OrderStatus> {
        let path = format!("/order/{}", order_id);
        let response = self
            .client
            .get(&path)
            .await
            .context("Failed to get order status")?;

        let status_str = response["status"].as_str().unwrap_or("UNKNOWN");

        let status = match status_str {
            "LIVE" | "OPEN" => {
                let remaining = response["remaining_size"]
                    .as_f64()
                    .unwrap_or(0.0);
                let original = response["original_size"]
                    .as_f64()
                    .unwrap_or(0.0);
                OrderStatus::Open {
                    remaining_size: remaining,
                    original_size: original,
                }
            }
            "FILLED" => {
                let avg = response["avg_price"].as_f64().unwrap_or(0.0);
                let filled = response["filled_size"].as_f64().unwrap_or(0.0);
                OrderStatus::Filled {
                    avg_price: avg,
                    filled_size: filled,
                }
            }
            "CANCELLED" => OrderStatus::Cancelled,
            _ => OrderStatus::Unknown,
        };

        Ok(status)
    }

    async fn get_open_orders(&self) -> Result<Vec<Order>> {
        let response = self
            .client
            .get("/orders/open")
            .await
            .context("Failed to get open orders")?;

        let orders = response
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok(orders)
    }

    async fn available_balance(&self, _side: TradeSide) -> Result<f64> {
        let response = self
            .client
            .get("/balance")
            .await
            .context("Failed to get balance")?;

        let balance = response["available"]
            .as_f64()
            .unwrap_or(0.0);

        Ok(balance)
    }

    async fn is_healthy(&self) -> bool {
        self.client.get("/time").await.is_ok()
    }

    async fn rate_limit_status(&self) -> (u32, u64) {
        self.maybe_reset_minute_counter();
        let used = self.orders_this_minute.load(Ordering::Relaxed);
        let remaining = 50u32.saturating_sub(used);
        let reset = self
            .minute_reset
            .lock()
            .map(|r| {
                let elapsed = r.elapsed().as_millis() as u64;
                60_000u64.saturating_sub(elapsed)
            })
            .unwrap_or(0);
        (remaining, reset)
    }
}
