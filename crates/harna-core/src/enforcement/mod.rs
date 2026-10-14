//! Container `conformance`: enforcement of the manifest and rules.
//!
//! Components:
//! - `validate` — compares code against `.harna/`: drift, hardcoded values,
//!   layer crossings, sequence coherence; names the fix.
//! - `parity`   — the change-propagation ledger: structural changes open
//!   parity debts that must be acknowledged, recorded as nothing-needed, or
//!   auto-verified before they stop gating CI.

pub mod parity;
pub mod statuses;
pub mod validate;