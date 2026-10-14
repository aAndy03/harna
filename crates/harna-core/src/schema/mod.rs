//! Container `schema`: the data model and storage layer.
//!
//! Components (specialized, importable):
//! - `manifest` — architecture: layers -> containers -> components
//! - `rules`    — scoped coding rules with rationale + origin
//! - `store`    — the `.harna/` directory handle + TOML persistence
//! - `migrate`  — schema versioning of the data files

pub mod manifest;
pub mod migrate;
pub mod rules;
pub mod store;
