//! Container `briefing`: what agents and humans see of the state.
//!
//! Components:
//! - `onboarding` — the ~1k-token digest a fresh agent session starts with.
//!   Composes `schema` (manifest + rules) and `search` (digest).
//! - `status`     — the one-shot health line (version, map, rules, parity).
//! - `place`      — guidance before code: where similar things live, which
//!   rules apply, where value-sets belong (the PLAN step of the flow).
//!
//! This container is allowed to reach into others; containers
//! below it must not reach sideways.

pub mod onboarding;
pub mod place;
pub mod status;
