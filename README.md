# Polymarket LMSR Arbitrage Bot

## Version 0.5.0

High-frequency arbitrage bot for Polymarket prediction markets using LMSR pricing, Bayesian estimation, and Kelly sizing.

### Architecture

**Hexagonal (Ports & Adapters)**

```
domain/          Pure business logic (LMSR, Kelly, Bayesian, Fees)
ports/           Trait definitions (MarketFeed, OrderExecution, ChainClient, Repository)
usecases/        Orchestration (ArbitrageEngine, OrderManager, RiskManager, Settlement, WalletManager)
adapters/api/    Polymarket CLOB REST client + auth
adapters/feeds/  Polymarket WS + Binance WS + Coinbase WS + Feed Bridge
adapters/chain/  Polygon RPC via alloy-rs 0.9 + contract validation
adapters/metrics/ Prometheus + health probes
adapters/persistence/ JSONL trades + atomic state snapshots
config/          TOML config + hot-reload (60s)
```

### Features

- **Event-driven** — `tokio::select!` over broadcast channels (<10ms feed-to-order)
- **Maker-first** — 0% fees + rebates; taker only when edge_net > threshold
- **Multi-asset** — Parallel BTC + ETH market support
- **Risk management** — Circuit breakers (per-trade ≤5%, hourly ≤10%, daily ≤30%)
- **On-chain validation** — Contracts verified at startup (code exists check)
- **Config hot-reload** — config.toml changes detected every 60s
- **Crash recovery** — Atomic state snapshots + JSONL trade logs
- **Observability** — Structured JSON tracing + Prometheus metrics on :9090
- **CI/CD** — GitHub Actions: fmt → clippy → test → audit → Docker → deploy

### Quick Start

```bash
git clone https://github.com/juankaspain/polymarket-lmsr-bot
cd polymarket-lmsr-bot
cp config.toml.example config.toml
cp .env.example .env
# Edit config.toml with your market IDs
# Edit .env with your API keys
cargo build
cargo test
```

### Docker

```bash
docker build -t polymarket-lmsr-bot .
docker run --env-file .env -v ./config.toml:/app/config.toml:ro -v ./data:/app/data -p 9090:9090 polymarket-lmsr-bot
```

### Testing

```bash
cargo test --lib              # Unit tests (domain + adapters)
cargo test --test '*'          # Integration + backtest + proptest
cargo deny check               # License + advisory audit
cargo audit                    # Vulnerability scan
```

### Stack

| Component | Version |
|---|---|
| Rust | 1.82 (edition 2024) |
| Tokio | 1.49 |
| alloy-rs | 0.9 |
| tokio-tungstenite | 0.24 |
| axum | 0.7 |
| serde | 1.0.219 (pinned) |
| prometheus | 0.13 |
| proptest | 1.5 |
| mockall | 0.13 |

### License

MIT
