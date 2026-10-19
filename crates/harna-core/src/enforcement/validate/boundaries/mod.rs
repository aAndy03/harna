//! Checks group `boundaries`: who may import whom.
//!
//! Layer boundaries and declared surfaces, gateways (facility owners), ownership (capabilities, markers, escape hatches), layering depth, role rules, transiters, and the deny.toml sync.

// Children reach the validate vocabulary (`Severity`, `Violation`, …)
// as `super::`, exactly as they did before the folder existed.
#[allow(
    unused_imports,
    reason = "children reach the validate vocabulary as super::*; not every group uses every name"
)]
use super::*;

pub mod boundary;
pub mod deny_sync;
pub mod gateway;
pub mod layering;
pub mod ownership;
pub mod roles;
pub mod transit;
