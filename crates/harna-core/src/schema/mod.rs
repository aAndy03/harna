//! Container `schema`: the data model and storage layer.
//!
//! Folders, one concern each (0.3.39 — the grouping lives in the tree,
//! not in `*_data` file names):
//! - `model/`     — the architecture as data: manifest, rules, scope
//! - `registers/` — the declared registers: advisory ledger, deferrals,
//!   lessons, non-goals, parity templates, roadmap, glossary
//! - `journal/`   — the decision log and the burst handoff
//! - `map`        — the code map's data structs (the search layer builds it)
//! - `store`      — the `.harna/` handle: persistence, atomic writes,
//!   migration, the sharded map on disk

pub mod journal;
pub mod map;
pub mod model;
pub mod registers;
pub mod store;
