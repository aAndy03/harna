//! Container `structure`: generating structure from templates.
//!
//! Components:
//! - `scaffold` — creates primitive/compound components and registers them
//!   in the manifest in the same step — structure is born correct.
//! - `deny` — renders the manifest's layer topology into a cargo-deny
//!   `deny.toml`, so wrong cross-crate imports fail the build.
//! - `render` — emits human views (ARCHITECTURE.md) from the manifest,
//!   so the prose is generated from the data, never stale.
//! - `graph` — exports the dependency graph (DOT, Mermaid): map modules
//!   as nodes, resolved internal imports as edges, the manifest's
//!   layer/container topology as clusters in manifest order.
//! - `adopt` — drafts a manifest proposal from the existing tree (a
//!   layer per crate, sizes in the sums; printed, pruned, then adopted).

pub mod adopt;
pub mod deny;
pub mod graph;
pub mod hooks;
pub mod linkage;
pub mod render;
pub mod scaffold;
