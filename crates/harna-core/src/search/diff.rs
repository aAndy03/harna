//! Component `diff`: compare two digests.
//!
//! The digest diff is the machine-readable API-change log between two states
//! of the codebase. It is the substrate parity needs: a change event computes
//! its fan-out from *what actually moved* (public surface + internal import
//! graph), not from assumption.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

use anyhow::Context as _;
use serde::Serialize;

use crate::search::digest::{Digest, Module};

#[derive(Debug, Default, Clone, Serialize)]
pub struct DigestDiff {
    pub from: String,
    pub to: String,
    pub modules_added: Vec<String>,
    pub modules_removed: Vec<String>,
    /// (module path, changed signature line)
    pub signatures_added: Vec<(String, String)>,
    pub signatures_removed: Vec<(String, String)>,
    /// (module path, changed dep id) — internal import graph moves
    pub deps_added: Vec<(String, String)>,
    pub deps_removed: Vec<(String, String)>,
    /// Modules whose bodies changed while their shipped surface stayed
    /// identical — body-only edits the ledger deliberately does not open
    /// targets for (they are governed by `validate --changed`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contents_changed: Vec<String>,
    /// Private-inventory moves: (module, description) for non-pub items that
    /// appeared or disappeared. Informational — private symbols cannot be
    /// imported across modules, so these never open fan-out targets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items_changed: Vec<(String, String)>,
}

impl DigestDiff {
    pub fn is_empty(&self) -> bool {
        self.modules_added.is_empty()
            && self.modules_removed.is_empty()
            && self.signatures_added.is_empty()
            && self.signatures_removed.is_empty()
            && self.deps_added.is_empty()
            && self.deps_removed.is_empty()
            && self.contents_changed.is_empty()
            && self.items_changed.is_empty()
    }

    /// Token-efficient rendering for agent consumption.
    pub fn render(&self) -> String {
        if self.is_empty() {
            return format!("no structural change between `{}` and `{}`", self.from, self.to);
        }
        let mut out = format!("structural diff `{}` -> `{}`:\n", self.from, self.to);
        if !self.modules_removed.is_empty() {
            out.push_str(&format!("removed modules: {}\n", self.modules_removed.join(", ")));
        }
        if !self.modules_added.is_empty() {
            out.push_str(&format!("added modules: {}\n", self.modules_added.join(", ")));
        }
        out.push_str(&sig_section("signature removed", &self.signatures_removed));
        out.push_str(&sig_section("signature added", &self.signatures_added));
        out.push_str(&sig_section("dep removed", &self.deps_removed));
        out.push_str(&sig_section("dep added", &self.deps_added));
        if !self.contents_changed.is_empty() {
            out.push_str(&format!(
                "contents changed (surface stable): {}\n",
                self.contents_changed.join(", ")
            ));
            out.push_str("  (body-only edits: governed by validate --changed, not the parity ledger)\n");
        }
        if !self.items_changed.is_empty() {
            out.push_str("private inventory moved:\n");
            for (module, what) in &self.items_changed {
                out.push_str(&format!("  {module} :: {what}\n"));
            }
            out.push_str("  (private items cannot be imported across modules; informational)\n");
        }
        out
    }
}

fn sig_section(label: &str, items: &[(String, String)]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (module, what) in items {
        out.push_str(&format!("{label}: {module} :: {what}\n"));
    }
    out
}

/// Compare two digests by module identity (repo-relative path) and by set
/// difference of signatures and dependency ids.
pub fn diff(from: &Digest, to: &Digest) -> DigestDiff {
    let from_map: HashMap<&str, &Module> =
        from.modules.iter().map(|m| (m.path.as_str(), m)).collect();
    let to_map: HashMap<&str, &Module> =
        to.modules.iter().map(|m| (m.path.as_str(), m)).collect();

    let mut d = DigestDiff {
        from: from.generated_at.clone(),
        to: to.generated_at.clone(),
        ..Default::default()
    };

    for (path, _m) in &to_map {
        if !from_map.contains_key(*path) {
            d.modules_added.push((*path).to_string());
        }
    }
    for (path, m) in &from_map {
        let Some(new) = to_map.get(*path) else {
            d.modules_removed.push((*path).to_string());
            continue;
        };
        // Set differences: signature order does not matter, only the surface.
        let old_sigs: HashSet<&str> = m.signatures.iter().map(|s| s.as_str()).collect();
        let new_sigs: HashSet<&str> = new.signatures.iter().map(|s| s.as_str()).collect();
        for s in new_sigs.difference(&old_sigs) {
            d.signatures_added.push(((*path).to_string(), (*s).to_string()));
        }
        for s in old_sigs.difference(&new_sigs) {
            d.signatures_removed.push(((*path).to_string(), (*s).to_string()));
        }
        let old_deps: HashSet<&str> = m.deps.iter().map(|s| s.as_str()).collect();
        let new_deps: HashSet<&str> = new.deps.iter().map(|s| s.as_str()).collect();
        for dep in new_deps.difference(&old_deps) {
            d.deps_added.push(((*path).to_string(), (*dep).to_string()));
        }
        for dep in old_deps.difference(&new_deps) {
            d.deps_removed.push(((*path).to_string(), (*dep).to_string()));
        }
        // Body-only change: same shipped surface, different content hash.
        let surface_equal = new_sigs == old_sigs && new_deps == old_deps;
        if surface_equal
            && !m.body_hash.is_empty()
            && !new.body_hash.is_empty()
            && m.body_hash != new.body_hash
        {
            d.contents_changed.push((*path).to_string());
        }
        // Private inventory: only non-pub items; keys (kind, name) without
        // the pub entries (their moves already surface via signatures).
        let priv_items = |m: &&Module| -> std::collections::HashSet<String> {
            m.inventory
                .iter()
                .filter(|i| i.vis == "priv")
                .map(|i| format!("{} {}", i.kind, i.name))
                .collect()
        };
        let old_priv = priv_items(&m);
        let new_priv = priv_items(&new);
        for item in new_priv.difference(&old_priv) {
            d.items_changed.push(((*path).to_string(), format!("{item} (added)")));
        }
        for item in old_priv.difference(&new_priv) {
            d.items_changed.push(((*path).to_string(), format!("{item} (removed)")));
        }
    }
    d.modules_added.sort();
    d.modules_removed.sort();
    d.signatures_added.sort();
    d.signatures_removed.sort();
    d.deps_added.sort();
    d.deps_removed.sort();
    d.contents_changed.sort();
    d.items_changed.sort();
    d
}

/// Load a committed digest from git (`git show <ref>:.harna/digest.json`).
/// Diffing needs a stable *baseline*: the digest must be committed for this
/// to work (harna's own convention).
pub fn digest_at_ref(root: &Path, spec: &str) -> anyhow::Result<Digest> {
    let arg = format!("{spec}:.harna/digest.json");
    let output = Command::new("git")
        .args(["show", &arg])
        .current_dir(root)
        .output()
        .map_err(|e| anyhow::anyhow!("running git failed: {e} — diffing requires a git repository"))?;
    if !output.status.success() {
        anyhow::bail!(
            "no .harna/digest.json at ref `{spec}` — commit the digest first (it is a harna convention)"
        );
    }
    serde_json::from_slice(&output.stdout).context("parsing committed digest.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(path: &str, sigs: &[&str], deps: &[&str]) -> Module {
        Module {
            path: path.into(),
            lang: "rust".into(),
            signatures: sigs.iter().map(|s| s.to_string()).collect(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            body_hash: "fixture".into(),
            inventory: vec![],
            mods: vec![],
        }
    }

    #[test]
    fn diff_detects_every_class_of_change() {
        let before = Digest {
            generated_at: "before".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![
                module("src/a.rs", &["pub fn one()", "pub fn two()"], &["crate::x"]),
                module("src/removed.rs", &["pub fn gone()"], &[]),
            ],
        };
        let after = Digest {
            generated_at: "after".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![
                module("src/a.rs", &["pub fn two()", "pub fn three()"], &["crate::y"]),
                module("src/new.rs", &["pub fn added()"], &[]),
            ],
        };
        let d = diff(&before, &after);
        assert_eq!(d.modules_added, vec!["src/new.rs"]);
        assert_eq!(d.modules_removed, vec!["src/removed.rs"]);
        assert!(d.signatures_added.contains(&("src/a.rs".into(), "pub fn three()".into())));
        assert!(d.signatures_removed.contains(&("src/a.rs".into(), "pub fn one()".into())));
        assert!(d.deps_added.contains(&("src/a.rs".into(), "crate::y".into())));
        assert!(d.deps_removed.contains(&("src/a.rs".into(), "crate::x".into())));
        assert!(!d.is_empty());
    }

    #[test]
    fn identical_digests_produce_empty_diff() {
        let a = Digest {
            generated_at: "t1".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![module("src/a.rs", &["pub fn one()"], &[])],
        };
        let b = Digest { generated_at: "t2".into(), extractor_version: crate::search::digest::EXTRACTOR_VERSION, modules: a.modules.clone() };
        let d = diff(&a, &b);
        assert!(d.is_empty());
        assert!(d.render().contains("no structural change"));
    }

    #[test]
    fn signature_order_does_not_count_as_change() {
        let a = Digest {
            generated_at: "t1".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![module("src/m.rs", &["pub fn a()", "pub fn b()"], &[])],
        };
        let b = Digest {
            generated_at: "t2".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![module("src/m.rs", &["pub fn b()", "pub fn a()"], &[])],
        };
        let d = diff(&a, &b);
        assert!(d.is_empty());
    }

    #[test]
    fn body_only_changes_are_visible_but_separate() {
        let mut a = Digest {
            generated_at: "t1".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![Module {
                path: "src/m.rs".into(),
                lang: "rust".into(),
                signatures: vec!["pub fn f()".into()],
                deps: vec![],
                body_hash: "h1".into(),
                inventory: vec![],
                mods: vec![],
            },],
        };
        let mut b = a.clone();
        b.generated_at = "t2".into();
        a.modules[0].path = "src/m.rs".into();
        b.modules[0].body_hash = "h2".into(); // same surface, new body

        let d = diff(&a, &b);
        assert_eq!(d.contents_changed, vec!["src/m.rs"]);
        assert!(d.signatures_added.is_empty() && d.signatures_removed.is_empty());
        assert!(!d.is_empty(), "contents-only edits must be observable");

        // Identical content is not a change at all.
        let d2 = diff(&a, &Digest { generated_at: "t3".into(), extractor_version: crate::search::digest::EXTRACTOR_VERSION, modules: a.modules.clone() });
        assert!(d2.is_empty());
    }
}
