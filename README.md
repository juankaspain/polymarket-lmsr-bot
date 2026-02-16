# Polymarket LMSR Arbitrage Bot

## Version 0.1.0

High-frequency arbitrage bot for Polymarket prediction markets.

### Features

- Event-driven architecture (<10ms latency)
- Maker-first strategy (0% fees + rebates)
- Multi-asset support (BTC, ETH)
- Circuit breakers & risk management
- Prometheus metrics

### Quick Start

```bash
cd polymarket-lmsr-bot
cp config.toml.example config.toml
cp .env.example .env
cargo build
cargo test
```

### Stack

- Rust 1.93.1
- tokio 1.49
- alloy-rs 0.9
