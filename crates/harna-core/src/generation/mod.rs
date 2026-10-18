//! Container `structure`: structure is born correct and stays linked.
//!
//! Folders (0.3.39):
//! - `structure/` — scaffold (components from templates), linkage (the mod
//!   tree and tests wired at birth), relocate (`harna move` keeps every
//!   link), adopt (a first manifest drafted from the tree)
//! - `views/`     — generated, committed views: ARCHITECTURE.md /
//!   SURFACE.md (render), the dependency graph, deny.toml
//! - `hooks`      — the enforcement harna installs for itself (git
//!   pre-commit, Claude Code hooks, CI)

pub mod hooks;
pub mod structure;
pub mod views;
