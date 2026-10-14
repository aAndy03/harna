//! harna-core: the data model and storage layer of harna.
//!
//! harna turns agent instructions into *data* that lives in the repository,
//! so that context survives compaction, sessions and machines.
//!
//! Organization mirrors harna's own principles — layers, containers,
//! components:
//!
//! - `schema`/      container of primitives: manifest, rules, store, migrate
//! - `search`/      container: digest (compressed code map)
//! - `enforcement/` container: validate (conformance checking)
//! - `generation/`  container: scaffold (structure from templates)
//! - `composition/` container: onboarding (compound combining schema + search)
//!
//! Current data schema version of the `.harna/` format lives here; missing
//! field in old files = version 1.

pub mod composition;
pub mod enforcement;
pub mod generation;
pub mod schema;
pub mod search;

/// Current version of the `.harna/` data format (the protocol between the
/// tool and the repo). Missing field in old files = version 1.
pub const SCHEMA_VERSION: u32 = 1;
