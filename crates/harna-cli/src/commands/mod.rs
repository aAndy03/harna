//! Command groups: each `*Cmd` engine owns its enum plus its arms.
//!
//! `main.rs` keeps the `Command` dispatch enum and the one-liner arms;
//! every variant with a subcommand lives here, one module per engine,
//! behind `pub fn run(store, root, cmd)`. The 0.3.31 `decompose` plan
//! executed verbatim: `run` was 61% of the file, these enums are the
//! split units.

pub mod contract;
pub mod gates;
pub mod hooks;
pub mod manifest;
pub mod notes;
pub mod packs;
pub mod parity;
pub mod registers;
pub mod rules;
