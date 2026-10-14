//! Container `onboarding`: compounds that combine other containers.
//!
//! Component `onboarding` composes `schema` (manifest + rules) and
//! `search` (digest) into the ~1k-token digest a fresh agent session
//! starts with. This container is allowed to reach into others; containers
//! below it must not reach sideways.

pub mod onboarding;
