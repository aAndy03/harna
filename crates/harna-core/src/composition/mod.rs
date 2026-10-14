//! Container `briefing`: what agents and humans see of the state.
//!
//! Components:
//! - `onboarding` — the ~1k-token digest a fresh agent session starts with.
//!   Composes `schema` (manifest + rules) and `search` (digest).
//! - `status`     — the one-shot health line (version, map, rules, parity).
//! - `place`      — the quick placement map: where similar things live,
//!   which rules apply, where value-sets belong.
//! - `plan`       — the full pre-code briefing: placement + decision context
//!   (supersession-aware) + rule bindings + dependency fan-out preview +
//!   test obligations, the PLAN step as explicit as the CHECK step.
//!
//! This container is allowed to reach into others; containers
//! below it must not reach sideways.

pub mod doctor;
pub mod onboarding;
pub mod place;
pub mod plan;
pub mod status;
