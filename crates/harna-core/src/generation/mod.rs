//! Container `structure`: generating structure from templates.
//!
//! Components:
//! - `scaffold` — creates primitive/compound components and registers them
//!   in the manifest in the same step — structure is born correct.
//! - `deny` — renders the manifest's layer topology into a cargo-deny
//!   `deny.toml`, so wrong cross-crate imports fail the build.

pub mod deny;
pub mod scaffold;
