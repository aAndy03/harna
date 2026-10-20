//! Container `briefing`: what agents and humans see of the state.
//!
//! Folders (0.3.39):
//! - `briefing/` — onboarding (+ the built-in concept map), place and plan
//!   before code, status, review/release notes
//! - `gates/`    — preflight (release gate), validate-intent (claims vs
//!   state), doctor (installation health), score (the grade and trend)
//! - `hooks/`    — hook setup and the hook runtime (session-start,
//!   post-edit, stop)
//! - `decompose` — the split planner; `scan` — one map rebuild for every
//!   surface
//!
//! This container is allowed to reach into others; containers below it
//! must not reach sideways.

pub mod briefing;
pub mod decompose;
pub mod gates;
pub mod hooks;
pub mod owners;
pub mod scan;
