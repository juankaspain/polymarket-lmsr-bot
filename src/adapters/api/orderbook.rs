//! Order Book Adapter - CLOB Order Book Queries
//!
//! Fetches order book snapshots from the Polymarket CLOB REST API
//! and converts them into domain types used by the pricing engine.
//! Supports both single-token and batch order book retrieval.

use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::{debug, warn};

use super::client::ClobClient;
use super::types::OrderBookResponse;

/// Order book adapter that wraps the CLOB HTTP client.
pub struct OrderBookAdapter {
    client: Arc<ClobClient>,
}

impl OrderBookAdapter {
    /// Create a new order book adapter.
    pub fn new(client: Arc<ClobClient>) -> Self {
        Self { client }
    }

    /// Fetch the order book snapshot for a single token.
    ///
    /// Calls GET /book?token_id={token_id} on the CLOB API
    /// and parses the response into bid/ask levels.
    pub async fn get_order_book(&self, token_id: &str) -> Result<OrderBookResponse> {
        let path = format!("/book?token_id={}", token_id);
        let response = self
            .client
            .get(&path)
            .await
            .context("Failed to fetch order book")?;

        let book: OrderBookResponse = response
            .json()
            .await
            .context("Failed to parse order book response")?;

        debug!(
            token_id,
            bids = book.bids.len(),
            asks = book.asks.len(),
            "Order book fetched"
        );

        Ok(book)
    }

    /// Fetch the mid-price for a token from the order book.
    ///
    /// Returns None if either the bid or ask side is empty.
    pub async fn get_mid_price(&self, token_id: &str) -> Result<Option<f64>> {
        let book = self.get_order_book(token_id).await?;

        let best_bid = book
            .bids
            .first()
            .and_then(|l| l.price.parse::<f64>().ok());
        let best_ask = book
            .asks
            .first()
            .and_then(|l| l.price.parse::<f64>().ok());

        match (best_bid, best_ask) {
            (Some(bid), Some(ask)) => Ok(Some((bid + ask) / 2.0)),
            _ => {
                warn!(token_id, "Incomplete order book, cannot compute mid-price");
                Ok(None)
            }
        }
    }

    /// Fetch the best bid and ask for a token.
    ///
    /// Returns (best_bid, best_ask) as Option pairs.
    pub async fn get_top_of_book(
        &self,
        token_id: &str,
    ) -> Result<(Option<f64>, Option<f64>)> {
        let book = self.get_order_book(token_id).await?;

        let best_bid = book
            .bids
            .first()
            .and_then(|l| l.price.parse::<f64>().ok());
        let best_ask = book
            .asks
            .first()
            .and_then(|l| l.price.parse::<f64>().ok());

        Ok((best_bid, best_ask))
    }

    /// Fetch order books for multiple tokens.
    ///
    /// Executes requests concurrently for efficiency.
    pub async fn get_order_books(
        &self,
        token_ids: &[&str],
    ) -> Vec<Result<OrderBookResponse>> {
        let mut handles = Vec::with_capacity(token_ids.len());

        for token_id in token_ids {
            let client = Arc::clone(&self.client);
            let tid = token_id.to_string();
            handles.push(tokio::spawn(async move {
                let path = format!("/book?token_id={}", tid);
                let response = client
                    .get(&path)
                    .await
                    .context("Failed to fetch order book")?;
                let book: OrderBookResponse = response
                    .json()
                    .await
                    .context("Failed to parse order book response")?;
                Ok(book)
            }));
        }

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => results.push(Err(anyhow::anyhow!("Task join error: {e}"))),
            }
        }
        results
    }

    /// Parse order book levels into sorted (price, size) tuples.
    ///
    /// Bids are sorted descending by price, asks ascending.
    pub fn parse_levels(book: &OrderBookResponse) -> (Vec<(f64, f64)>, Vec<(f64, f64)>) {
        let mut bids: Vec<(f64, f64)> = book
            .bids
            .iter()
            .filter_map(|l| {
                let price = l.price.parse::<f64>().ok()?;
                let size = l.size.parse::<f64>().ok()?;
                Some((price, size))
            })
            .collect();

        let mut asks: Vec<(f64, f64)> = book
            .asks
            .iter()
            .filter_map(|l| {
                let price = l.price.parse::<f64>().ok()?;
                let size = l.size.parse::<f64>().ok()?;
                Some((price, size))
            })
            .collect();

        // Bids descending, asks ascending
        bids.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        asks.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        (bids, asks)
    }

    /// Calculate the spread in basis points from raw order book.
    pub fn spread_bps(book: &OrderBookResponse) -> Option<f64> {
        let best_bid = book.bids.first()?.price.parse::<f64>().ok()?;
        let best_ask = book.asks.first()?.price.parse::<f64>().ok()?;
        let mid = (best_bid + best_ask) / 2.0;
        if mid > 0.0 {
            Some((best_ask - best_bid) / mid * 10_000.0)
        } else {
            None
        }
    }
}
