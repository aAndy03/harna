//! Container `code-map`: the compressed, queryable map of the codebase.
//!
//! Component `digest` stores signatures only — never file bodies — so the
//! map stays small enough to inline into a prompt or diff in a PR.

pub mod digest;
