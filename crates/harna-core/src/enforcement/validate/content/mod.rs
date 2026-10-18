//! Checks group `content`: what the source text may contain.
//!
//! Hard-coded URLs, pattern rules (naming as data), copy-paste and single-owner reuse.

// Children reach the validate vocabulary (`Severity`, `Violation`, …)
// as `super::`, exactly as they did before the folder existed.
#[allow(unused_imports)]
use super::*;

pub mod urls;
pub mod pattern;
pub mod reuse;
