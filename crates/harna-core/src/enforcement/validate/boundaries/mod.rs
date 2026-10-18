//! Checks group `boundaries`: who may import whom.
//!
//! Layer boundaries and declared surfaces, gateways (facility owners), layering depth, role rules, transiters, and the deny.toml sync.

// Children reach the validate vocabulary (`Severity`, `Violation`, …)
// as `super::`, exactly as they did before the folder existed.
#[allow(unused_imports)]
use super::*;

pub mod boundary;
pub mod gateway;
pub mod layering;
pub mod roles;
pub mod transit;
pub mod deny_sync;
