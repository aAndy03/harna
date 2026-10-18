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
//! - `score`      — the composite health grade: coverage, debts, freshness,
//!   boundary pressure, coupling outliers (0.3.12; 0.3.13 persists these).
//!
//! - `notes`      — collaboration drafts rendered from data (0.3.22): review
//!   checklists (fan-out + scoped rules + registered tests) and release-note
//!   drafts (decisions + parity between refs), stdout only.
//!
//! - `preflight`  — the release gate as one command (0.3.29): validation
//!   errors, doctor failures, open parity debts and map freshness compose
//!   into PASS/FAIL; the test suite is named NOT-RUN (harna never executes
//!   project code), never pretended passed.
//!
//! - `intent`      — truthfulness reconciliation (0.3.30): the mechanically
//!   verifiable claims (package version, changelog head, roadmap NOW and
//!   DONEs, parity silence) cross-checked against the state; DRIFTs name
//!   both sides, skips say what they cannot see, exit 2 on drift.
//!
//! This container is allowed to reach into others; containers
//! below it must not reach sideways.

pub mod decompose;
pub mod doctor;
pub mod hook_events;
pub mod hook_setup;
pub mod intent;
pub mod notes;
pub mod onboarding;
pub mod place;
pub mod plan;
pub mod preflight;
pub mod score;
pub mod status;
