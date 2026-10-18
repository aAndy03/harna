//! Command groups: each `*Cmd` engine owns its enum plus its arms.
//!
//! `main.rs` keeps the `Command` dispatch enum and the one-liner arms;
//! every variant with a subcommand lives here, one module per engine,
//! behind `pub fn run(store, root, cmd)`. The 0.3.31 `decompose` plan
//! executed verbatim: `run` was 61% of the file, these enums are the
//! split units.

pub mod ack;
pub mod deferred;
pub mod hooks;
pub mod lesson;
pub mod non_goal;
pub mod notes;
pub mod parity;
pub mod roadmap;
pub mod rules;
pub mod templates;
pub mod terms;
