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
//! - `deferred-data` — the DEBT REGISTER's data structs (0.3.23):
//!   deliberate deferrals with a review trigger, distinct from parity
//!   debts (the store persists them)
//! - `terms-data` — the GLOSSARY's data structs (0.3.26): agent
//!   vocabulary -> harna vocabulary, collision-checked pure logic (the
//!   store persists them)
//! - `non-goals-data` — the DECLINE register's data structs (0.3.27):
//!   deliberate product refusals, distinct from rules (the store
//!   persists them)
//! - `lessons-data` — the DEAD-ENDS register's data structs (0.3.27):
//!   tried-and-failed records with verdicts (the store persists them)
//! - `roadmap-data` — the ROADMAP's data structs (0.3.28): declared
//!   milestones with states, coherence-checked; deferred reviews can
//!   key to a milestone (the store persists it)

pub mod decisions;
pub mod deferred_data;
pub mod digest_data;
pub mod handoff;
pub mod lessons_data;
pub mod manifest;
pub mod migrate;
pub mod non_goals_data;
pub mod parity_data;
pub mod roadmap_data;
pub mod rules;
pub mod store;
pub mod terms_data;
