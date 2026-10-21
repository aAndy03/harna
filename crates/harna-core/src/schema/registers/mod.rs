//! Group `registers`: the declared registers, one data model each —
//! advisory ledger, ratchet baselines, deferrals, lessons (dead ends), non-goals (refusals),
//! parity templates, roadmap, glossary, qualities (long-term intent). The store persists them
//! (`store/register_io.rs`); behaviour lives with their consumers.

pub mod approvals;
pub mod assumptions;
pub mod commands;
pub mod ledgers;
pub mod lessons;
pub mod non_goals;
pub mod parity_templates;
pub mod qualities;
pub mod roadmap;
pub mod terms;
