//! Checks group `sizing`: how big things may grow.
//!
//! Rule-bound budgets (container-size, fan-out-cap, born-size), folder budgets and name clusters, hotspots.

// Children reach the validate vocabulary (`Severity`, `Violation`, …)
// as `super::`, exactly as they did before the folder existed.
#[allow(unused_imports)]
use super::*;

pub mod budgets;
pub mod folders;
pub mod hotspot;
