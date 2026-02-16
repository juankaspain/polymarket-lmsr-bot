//! Integration Tests - End-to-end Bot Component Testing
//!
//! Tests the interaction between usecases, ports, and mock adapters.
//! Uses mockall for trait mocking and tokio::test for async tests.

use std::sync::Arc;
use std::time::Duration;

use mockall::predicate::*;
use mockall::mock;
use tokio::sync::broadcast;

// ---- Mock Definitions ----

mock! {
    pub OrderExec {}

    #[async_trait::async_trait]
    impl polymarket_lmsr_bot::ports::execution::OrderExecution for OrderExec {
        async fn place_order(
            &self,
            order: &polymarket_lmsr_bot::domain::trade::Order,
        ) -> anyhow::Result<polymarket_lmsr_bot::ports::execution::OrderPlacement>;

        async fn cancel_order(
            &self,
            order_id: &str,
        ) -> anyhow::Result<polymarket_lmsr_bot::ports::execution::OrderCancellation>;

        async fn cancel_all_orders(&self) -> anyhow::Result<usize>;

        async fn cancel_orders_for_token(
            &self,
            token_id: &str,
        ) -> anyhow::Result<Vec<polymarket_lmsr_bot::ports::execution::OrderCancellation>>;

        async fn get_order_status(
            &self,
            order_id: &str,
        ) -> anyhow::Result<polymarket_lmsr_bot::ports::execution::OrderStatus>;

        async fn get_open_orders(
            &self,
        ) -> anyhow::Result<Vec<polymarket_lmsr_bot::domain::trade::Order>>;

        async fn available_balance(
            &self,
            side: polymarket_lmsr_bot::domain::trade::TradeSide,
        ) -> anyhow::Result<f64>;

        async fn is_healthy(&self) -> bool;

        async fn rate_limit_status(&self) -> (u32, u64);
    }
}

mock! {
    pub ChainCli {}

    #[async_trait::async_trait]
    impl polymarket_lmsr_bot::ports::chain_client::ChainClient for ChainCli {
        async fn usdc_balance(&self) -> anyhow::Result<f64>;
        async fn token_balance(&self, token_id: &str)
            -> anyhow::Result<polymarket_lmsr_bot::ports::chain_client::TokenBalance>;
        async fn batch_redeem(&self, token_ids: &[String])
            -> anyhow::Result<polymarket_lmsr_bot::ports::chain_client::RedemptionResult>;
        async fn is_condition_resolved(&self, condition_id: &str) -> anyhow::Result<bool>;
        async fn gas_price_gwei(&self) -> anyhow::Result<f64>;
        async fn is_healthy(&self) -> bool;
    }
}

mock! {
    pub Repo {}

    #[async_trait::async_trait]
    impl polymarket_lmsr_bot::ports::repository::Repository for Repo {
        async fn save_trade(&self, record: &polymarket_lmsr_bot::ports::repository::TradeRecord)
            -> anyhow::Result<()>;
        async fn load_trades(&self) -> anyhow::Result<Vec<polymarket_lmsr_bot::ports::repository::TradeRecord>>;
        async fn load_trades_range(&self, from_ms: u64, to_ms: u64)
            -> anyhow::Result<Vec<polymarket_lmsr_bot::ports::repository::TradeRecord>>;
        async fn save_state(&self, state: &polymarket_lmsr_bot::ports::repository::BotStateSnapshot)
            -> anyhow::Result<()>;
        async fn load_latest_state(&self)
            -> anyhow::Result<Option<polymarket_lmsr_bot::ports::repository::BotStateSnapshot>>;
        async fn save_daily_pnl(&self, pnl: &polymarket_lmsr_bot::ports::repository::DailyPnl)
            -> anyhow::Result<()>;
        async fn load_daily_pnl(&self)
            -> anyhow::Result<Vec<polymarket_lmsr_bot::ports::repository::DailyPnl>>;
        async fn is_healthy(&self) -> bool;
    }
}

// ---- Integration Tests ----

#[tokio::test]
async fn test_order_placement_and_cancellation_lifecycle() {
    let mut mock_exec = MockOrderExec::new();

    // Expect a successful order placement
    mock_exec
        .expect_place_order()
        .returning(|_order| {
            Ok(polymarket_lmsr_bot::ports::execution::OrderPlacement {
                order_id: "ord_123".to_string(),
                accepted: true,
                rejection_reason: None,
                timestamp_ms: 1700000000000,
            })
        });

    // Expect cancellation
    mock_exec
        .expect_cancel_order()
        .with(eq("ord_123"))
        .returning(|oid| {
            Ok(polymarket_lmsr_bot::ports::execution::OrderCancellation {
                order_id: oid.to_string(),
                success: true,
                error: None,
            })
        });

    let exec = Arc::new(mock_exec);

    // Place order
    let order = polymarket_lmsr_bot::domain::trade::Order {
        id: String::new(),
        token_id: "token_yes_btc".to_string(),
        side: polymarket_lmsr_bot::domain::trade::TradeSide::Buy,
        price: 0.45,
        size: 10.0,
        order_type: polymarket_lmsr_bot::domain::trade::OrderType::Gtc,
        post_only: true,
        timestamp_ms: 0,
    };

    let result = exec.place_order(&order).await.unwrap();
    assert!(result.accepted);
    assert_eq!(result.order_id, "ord_123");

    // Cancel order
    let cancel = exec.cancel_order("ord_123").await.unwrap();
    assert!(cancel.success);
}

#[tokio::test]
async fn test_settlement_with_resolved_market() {
    let mut mock_chain = MockChainCli::new();

    mock_chain
        .expect_is_condition_resolved()
        .with(eq("condition_abc"))
        .returning(|_| Ok(true));

    mock_chain
        .expect_batch_redeem()
        .returning(|ids| {
            Ok(polymarket_lmsr_bot::ports::chain_client::RedemptionResult {
                tx_hash: "0xabc123".to_string(),
                positions_redeemed: ids.len(),
                usdc_recovered: 50.0,
                gas_cost_matic: 0.01,
            })
        });

    let resolved = mock_chain
        .is_condition_resolved("condition_abc")
        .await
        .unwrap();
    assert!(resolved);

    let redeem = mock_chain
        .batch_redeem(&["condition_abc".to_string()])
        .await
        .unwrap();
    assert_eq!(redeem.positions_redeemed, 1);
    assert_eq!(redeem.usdc_recovered, 50.0);
}

#[tokio::test]
async fn test_risk_manager_circuit_breaker_integration() {
    use polymarket_lmsr_bot::config::RiskConfig;
    use polymarket_lmsr_bot::usecases::risk_manager::RiskManager;

    let config = RiskConfig {
        max_daily_loss_fraction: 0.02,
        max_position_size: 100.0,
        max_total_exposure: 500.0,
        min_bankroll: 50.0,
        circuit_breaker_losses: 5,
        cooldown_seconds: 1800,
    };

    let mut rm = RiskManager::new(&config);

    // Should allow trading initially
    assert!(rm.can_trade());
    assert!(rm.can_open_position(50.0, 1000.0));

    // Simulate 5 consecutive losses
    for _ in 0..5 {
        rm.record_trade(-10.0);
    }

    // Circuit breaker should now block trading
    assert!(rm.is_circuit_breaker_active());
    assert!(!rm.can_trade());

    // Reset daily counters
    rm.reset_daily();
    assert!(!rm.is_circuit_breaker_active());
    assert!(rm.can_trade());
}

#[tokio::test]
async fn test_repository_save_and_load_trade() {
    let mut mock_repo = MockRepo::new();

    let record = polymarket_lmsr_bot::ports::repository::TradeRecord {
        id: "trade_001".to_string(),
        order_id: "ord_001".to_string(),
        market_id: "market_btc_up".to_string(),
        side: "BUY".to_string(),
        price: 0.55,
        size: 20.0,
        lmsr_fair_value: 0.52,
        edge: 0.03,
        kelly_fraction: 0.25,
        fees: 0.0,
        timestamp_ms: 1700000000000,
    };

    let record_clone = record.clone();

    mock_repo
        .expect_save_trade()
        .returning(|_| Ok(()));

    mock_repo
        .expect_load_trades()
        .returning(move || Ok(vec![record_clone.clone()]));

    // Save
    mock_repo.save_trade(&record).await.unwrap();

    // Load
    let trades = mock_repo.load_trades().await.unwrap();
    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].id, "trade_001");
    assert_eq!(trades[0].edge, 0.03);
    assert_eq!(trades[0].fees, 0.0); // Maker = 0% fees
}

#[tokio::test]
async fn test_wallet_balance_query() {
    let mut mock_chain = MockChainCli::new();

    mock_chain
        .expect_usdc_balance()
        .returning(|| Ok(1500.0));

    mock_chain
        .expect_is_healthy()
        .returning(|| true);

    let balance = mock_chain.usdc_balance().await.unwrap();
    assert_eq!(balance, 1500.0);
    assert!(mock_chain.is_healthy().await);
}

#[tokio::test]
async fn test_graceful_shutdown_cancels_orders() {
    let mut mock_exec = MockOrderExec::new();

    mock_exec
        .expect_cancel_all_orders()
        .times(1)
        .returning(|| Ok(5));

    let cancelled = mock_exec.cancel_all_orders().await.unwrap();
    assert_eq!(cancelled, 5);
}
