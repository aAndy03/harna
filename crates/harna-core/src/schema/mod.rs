//! Container `schema`: the data model and storage layer.
//!
//! Components (specialized, importable):
//! - `manifest`  — architecture: layers -> containers -> components
//! - `rules`     — scoped coding rules with rationale + origin
//! - `decisions` — the decision log: supersedes chains, chain rendering
//! - `store`     — the `.harna/` directory handle + TOML persistence
//! - `migrate`   — schema versioning of the data files

pub mod decisions;
pub mod manifest;
pub mod migrate;
pub mod rules;
pub mod store;
