//! Container `schema`: the data model and storage layer.
//!
//! Components (specialized, importable):
//! - `manifest`    — architecture: layers -> containers -> components
//! - `rules`       — scoped coding rules with rationale + origin
//! - `decisions`   — the decision log: supersedes chains, chain rendering
//! - `handoff`     — the burst journal: recorded before compaction, read at
//!   session start
//! - `store`       — the `.harna/` directory handle + TOML persistence
//! - `migrate`     — schema versioning of the data files
//! - `digest-data` — the digest's DATA structs (0.3.9): owned here so the
//!   memory layer stops reaching into search (the store persists them)
//! - `parity-data` — the parity TEMPLATES' data structs (0.3.22): declared
//!   fan-out shapes for recurring change types (the store persists them)

pub mod decisions;
pub mod digest_data;
pub mod handoff;
pub mod manifest;
pub mod migrate;
pub mod parity_data;
pub mod rules;
pub mod store;
