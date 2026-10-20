//! Container `conformance`: enforcement of the manifest and rules.
//!
//! Components:
//! - `checks`   — the check registry as data (checks.toml) + its loader:
//!   one canonical source for check ids, descriptions, default severities.
//! - `violation` — the shared violation shape (Severity + render), kept
//!   outside `validate` so registry and checker cannot ring each other.
//! - `validate` — compares code against `.harna/`: drift, hardcoded values,
//!   layer crossings, sequence coherence; names the fix.
//! - `parity`   — the change-propagation ledger: structural changes open
//!   parity debts that must be acknowledged, recorded as nothing-needed, or
//!   auto-verified before they stop gating CI. Its policy (who needs a
//!   human) and its status vocabulary live in `parity/`.
//!
//! `validate/` groups its checks by concern — `boundaries/`, `coherence/`,
//! `sizing/`, `content/` — while the gate itself (run_on, the advisory
//! ledger, rule bindings, git change selection) stays at its root.

pub mod admission;
pub mod checks;
pub mod parity;
pub mod validate;
pub mod violation;
