//! Group `registers`: the declared registers, one data model each —
//! advisory ledger, deferrals, lessons (dead ends), non-goals (refusals),
//! parity templates, roadmap, glossary. The store persists them
//! (`store/register_io.rs`); behaviour lives with their consumers.

pub mod acknowledged;
pub mod deferred;
pub mod lessons;
pub mod non_goals;
pub mod parity_templates;
pub mod roadmap;
pub mod terms;
