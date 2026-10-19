//! Checks group `content`: what the source text may contain.
//!
//! Hard-coded URLs, panics in production code, banned APIs, reasonless suppressions, sync groups, pattern rules (naming as data), copy-paste and single-owner reuse.

// Children reach the validate vocabulary (`Severity`, `Violation`, …)
// as `super::`, exactly as they did before the folder existed.
#[allow(
    unused_imports,
    reason = "children reach the validate vocabulary as super::*; not every group uses every name"
)]
use super::*;

pub mod banned;
pub mod panics;
pub mod pattern;
pub mod reuse;
pub mod suppressions;
pub mod sync;
pub mod urls;
