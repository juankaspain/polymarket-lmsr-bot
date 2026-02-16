# Changelog

## [0.5.0] - 2026-02-16

### Added
- **Polymarket WebSocket Feed** (`adapters/feeds/polymarket_ws.rs`): Primary market data source implementing `MarketFeed` port with broadcast channels, debounce (skip delta < 0.5%), and auto-reconnect
- **Feed Bridge** (`adapters/feeds/bridge.rs`): Converts `BinanceTick` → `PriceUpdate` for cross-validation; warns on >2% price divergence
- **Repository Implementation** (`adapters/persistence/repository_impl.rs`): Concrete `Repository` trait implementation wrapping `StateStore` + `TradeLogger`
- **Contract Validator** (`adapters/chain/validator.rs`): On-chain contract validation at startup (code exists check); fails hard if CTF Exchange is invalid
- **Config Hot-Reload** (`config/hot_reload.rs`): Watches config.toml every 60s, broadcasts changes via `watch` channel for A/B testing
- **Wallet Config** (`config/mod.rs`): `WalletConfig` with hot/cold allocation (20%/80%), MATIC minimum, alert thresholds
- **Settlement Config** (`config/mod.rs`): `SettlementConfig` with batch redeem timing, EIP-1559 gas parameters
- **Property-Based Tests** (`tests/proptest_domain.rs`): Proptest for LMSR, Kelly, Fees invariants
- **cargo-deny config** (`deny.toml`): License, advisory, ban, and source policy enforcement

### Changed
- **main.rs**: Full wire-up — PolygonProvider → ContractValidator → PolymarketFeed → ArbitrageEngine → RepositoryImpl; state recovery on startup; state save on shutdown
- **CI/CD**: cargo-deny now uses `deny.toml` (strict mode); version pinned to Rust 1.82
- **config.toml.example**: Added `[wallet]` and `[settlement]` sections
- **Cargo.toml**: Version 0.5.0

### Fixed
- ArbitrageEngine now runs with real `MarketFeed` adapter (PolymarketFeed) instead of heartbeat loop

## [0.4.0] - 2026-02-16

### Added
- Full hexagonal architecture: 39 source files across domain, ports, adapters, usecases
- Sprint 2 audit: all files compile with 0 errors, 0 warnings

## [0.1.0] - TBD

Initial release
