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

/// Newest version of the `.harna/` data format this harna understands (the
/// protocol between the tool and the repo). Missing field in old files = 1.
/// v2 (0.4.9) = the 0.4 fields an older harna would silently drop on save:
/// isolated layers, slots, test kinds, meaning-scoped rules.
pub const SCHEMA_VERSION: u32 = 2;

/// The version data is stamped with when it needs nothing newer: a file is
/// stamped with the lowest version that holds it (`required_schema`), so a
/// repo not using the 0.4 fields stays readable by an older harna, and one
/// using them is refused by it rather than stripped.
pub const SCHEMA_BASELINE: u32 = 1;
