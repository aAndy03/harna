//! Checks group `coherence`: the declared data stays true.
//!
//! Manifest paths and sequences, structure vs the map, rename linkage, decision chains, roadmap coherence, registered tests.

// Children reach the validate vocabulary (`Severity`, `Violation`, …)
// as `super::`, exactly as they did before the folder existed.
#[allow(unused_imports)]
use super::*;

pub mod decisions;
pub mod manifest;
pub mod rename;
pub mod roadmap;
pub mod structure;
pub mod test_coverage;
