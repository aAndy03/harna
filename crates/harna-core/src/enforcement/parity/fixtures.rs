//! Shared fixtures for the parity tests (the ledger's and the auto pass's).

use super::*;
use crate::search::digest::Module;

/// Workspace crate names for the fixture digests (crate `x` is a member
/// so cross-crate imports like `x::core::store::load` resolve).
pub(super) fn workspace() -> Vec<String> {
    vec!["x".to_string()]
}

pub(super) fn module(path: &str, sigs: &[&str], deps: &[&str]) -> Module {
    Module {
        reexports: Vec::new(),
        crate_dir: None,
        src_root: None,
        path: path.into(),
        lang: "rust".into(),
        signatures: sigs.iter().map(|s| s.to_string()).collect(),
        deps: deps.iter().map(|s| s.to_string()).collect(),
        body_hash: "fixture".into(),
        inventory: vec![],
        mods: vec![],
    }
}

pub(super) fn digests() -> (Digest, Digest) {
    let baseline = Digest {
        orphans: Vec::new(),
        generated_at: "before".into(),
        extractor_version: crate::search::digest::EXTRACTOR_VERSION,
        modules: vec![
            module("crates/x/src/core/store.rs", &["pub fn load()"], &[]),
            // API picks up x's store via the workspace crate
            module("crates/y/src/api.rs", &[], &["x::core::store::load"]),
            module("crates/x/src/core/logger.rs", &["pub fn log()"], &[]),
        ],
    };
    let fresh = Digest {
        orphans: Vec::new(),
        generated_at: "after".into(),
        extractor_version: crate::search::digest::EXTRACTOR_VERSION,
        modules: vec![
            module("crates/x/src/core/store.rs", &["pub fn load_v2()"], &[]),
            module("crates/y/src/api.rs", &[], &["x::core::store::load"]),
            module("crates/x/src/core/logger.rs", &["pub fn log()"], &[]),
            module("crates/x/src/core/added.rs", &["pub fn new_thing()"], &[]),
        ],
    };
    (baseline, fresh)
}
