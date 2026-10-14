//! Container `briefing`: what agents and humans see of the state.
//!
//! Components:
//! - `onboarding` — the ~1k-token digest a fresh agent session starts with.
//!   Composes `schema` (manifest + rules) and `search` (digest).
//! - `status`     — the one-shot health line (version, map, rules, parity).
//!
//! This container is allowed to reach into others; containers
//! below it must not reach sideways.

pub mod onboarding;
pub mod status;
