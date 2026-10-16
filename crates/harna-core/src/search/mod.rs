//! Container `code-map`: the compressed, queryable map of the codebase.
//!
//! Components:
//! - `digest`     — signatures + inventory + internal deps, never file bodies
//! - `diff`       — set-difference between two digests (API-change log)
//! - `dependents` — the reverse-edge query: who breaks if a module changes
//! - `whereis`    — the symbol-level locator: definitions + referencing
//!   modules, straight from the inventory (pub and private)
//! - `metrics`    — fan-in/fan-out and boundary pressure over the import
//!   graph, plus the coupling outliers (the measurement half of 0.3.12)

pub mod dependents;
pub mod digest;
pub mod diff;
pub mod metrics;
pub mod whereis;
