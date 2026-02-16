//! Chain Adapters - Polygon Blockchain Interaction Layer
//!
//! Provides on-chain access via alloy-rs 0.9 for:
//! - RPC provider management with failover
//! - CTF contract interactions (balance, redeem)
//! - ERC-20 approval management (USDCe → CTF, CTF → exchanges)
//! - Gas price monitoring with EIP-1559 support

pub mod approvals;
pub mod contracts;
pub mod gas;
pub mod provider;

pub use approvals::ApprovalManager;
pub use contracts::CtfContracts;
pub use gas::GasOracle;
pub use provider::PolygonProvider;
