//! Container `conformance`: enforcement of the manifest and rules.
//!
//! Component `validate` compares code against `.harna/` — drift, hardcoded
//! values, layer crossings — and names the fix, so agents can repair.

pub mod validate;
