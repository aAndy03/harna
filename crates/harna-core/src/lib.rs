//! harna-core: the data model and storage layer of harna.
//!
//! harna turns agent instructions into *data* that lives in the repository,
//! so that context survives compaction, sessions and machines:
//!
//! - `manifest.toml` — architecture: layers -> containers -> components
//! - `rules.toml`    — scoped coding rules with rationale + origin
//! - `decisions/`    — ADR-style markdown decision log
//! - `digest.json`   — compressed code map (signatures, not bodies)

pub mod digest;
pub mod manifest;
pub mod migrate;
pub mod onboarding;
pub mod rules;
pub mod scaffold;
pub mod store;
pub mod validate;

/// Current version of the `.harna/` data format (the protocol between the
/// tool and the repo). Missing field in old files = version 1.
pub const SCHEMA_VERSION: u32 = 1;
