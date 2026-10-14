//! Container `code-map`: the compressed, queryable map of the codebase.
//!
//! Components:
//! - `digest` — signatures + internal deps only, never file bodies
//! - `diff`   — set-difference between two digests (API-change log)

pub mod digest;
pub mod diff;
