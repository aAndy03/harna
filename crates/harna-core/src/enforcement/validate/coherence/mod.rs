//! Checks group `coherence`: the declared data stays true.
//!
//! Manifest paths and sequences, the layer DAG on Cargo deps, structure vs the map, rename linkage, decision chains, roadmap coherence, registered tests.

// Children reach the validate vocabulary (`Severity`, `Violation`, …)
// as `super::`, exactly as they did before the folder existed.
#[allow(
    unused_imports,
    reason = "children reach the validate vocabulary as super::*; not every group uses every name"
)]
use super::*;

pub mod cargo_layering;
pub mod decisions;
pub mod manifest;
pub mod rename;
pub mod roadmap;
pub mod structure;
pub mod test_coverage;
