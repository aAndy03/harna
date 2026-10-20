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
            return format!(
                "no structural change between `{}` and `{}`",
                self.from, self.to
            );
        }
        let mut out = format!("structural diff `{}` -> `{}`:\n", self.from, self.to);
        if !self.modules_removed.is_empty() {
            out.push_str(&format!(
                "removed modules: {}\n",
                self.modules_removed.join(", ")
            ));
        }
        if !self.modules_added.is_empty() {
            out.push_str(&format!(
                "added modules: {}\n",
                self.modules_added.join(", ")
            ));
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
            out.push_str(
                "  (body-only edits: governed by validate --changed, not the parity ledger)\n",
            );
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
    let to_map: HashMap<&str, &Module> = to.modules.iter().map(|m| (m.path.as_str(), m)).collect();

    let mut d = DigestDiff {
        from: from.generated_at.clone(),
        to: to.generated_at.clone(),
        ..Default::default()
    };

    for path in to_map.keys() {
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
            d.signatures_added
                .push(((*path).to_string(), (*s).to_string()));
        }
        for s in old_sigs.difference(&new_sigs) {
            d.signatures_removed
                .push(((*path).to_string(), (*s).to_string()));
        }
        let old_deps: HashSet<&str> = m.deps.iter().map(|s| s.as_str()).collect();
        let new_deps: HashSet<&str> = new.deps.iter().map(|s| s.as_str()).collect();
        for dep in new_deps.difference(&old_deps) {
            d.deps_added.push(((*path).to_string(), (*dep).to_string()));
        }
        for dep in old_deps.difference(&new_deps) {
            d.deps_removed
                .push(((*path).to_string(), (*dep).to_string()));
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
        let old_priv = priv_items(m);
        let new_priv = priv_items(new);
        for item in new_priv.difference(&old_priv) {
            d.items_changed
                .push(((*path).to_string(), format!("{item} (added)")));
        }
        for item in old_priv.difference(&new_priv) {
            d.items_changed
                .push(((*path).to_string(), format!("{item} (removed)")));
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

/// Load a committed digest from git: the sharded layout
/// (`.harna/digest/index.json` + shards, 0.3.15) when the ref has one,
/// the legacy `.harna/digest.json` monolith otherwise. Diffing needs a
/// stable *baseline*: the digest must be committed for this to work
/// (harna's own convention).
/// What a parity event's numbers mean when there is no baseline yet
/// (0.3.49, ledger field test: `parity open` before the first commit read
/// "0 open, all origin" with no hint why — everything is new against the
/// empty tree).
pub fn baseline_note(root: &Path, spec: &str) -> Option<String> {
    (spec == "HEAD" && !head_exists(root)).then(|| {
        "no commit yet: the baseline is the empty tree, so every module is new (origin) — commit once, then parity tracks each change against it".to_string()
    })
}

pub fn digest_at_ref(root: &Path, spec: &str) -> anyhow::Result<Digest> {
    check_ref(spec)?;
    if spec == "HEAD" && !head_exists(root) {
        // Unborn HEAD (git init, no commits yet): the empty tree is the
        // honest baseline — the first structural change diffs against
        // nothing, which is exactly what it is (0.3.32 dogfood: the old
        // error left the very first change with no baseline story).
        return Ok(Digest::default());
    }
    if has_at_ref(root, spec, ".harna/digest/index.json") {
        return digest_sharded_at_ref(root, spec);
    }
    let arg = format!("{spec}:.harna/digest.json");
    let output = Command::new("git")
        .args(["show", &arg])
        .current_dir(root)
        .output()
        .map_err(|e| {
            anyhow::anyhow!("running git failed: {e} — diffing requires a git repository")
        })?;
    if !output.status.success() {
        anyhow::bail!(
            "no committed digest at ref `{spec}` — commit the digest first (it is a harna convention)"
        );
    }
    serde_json::from_slice(&output.stdout).context("parsing committed digest.json")
}

/// A ref from the CLI, MCP or a stored parity event reaches `git` as a
/// positional argument: one starting with `-` would be parsed as an
/// option (`--output=<file>` makes `git show` write anywhere). Git refs
/// never start with `-`, and never contain whitespace or NUL.
pub fn check_ref(spec: &str) -> anyhow::Result<()> {
    if spec.is_empty()
        || spec.starts_with('-')
        || spec.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        anyhow::bail!(
            "`{spec}` is not a git ref (refs never start with `-` or contain whitespace)"
        );
    }
    Ok(())
}

/// Does `<ref>:<path>` exist? (`git cat-file -e` probes without output.)
fn has_at_ref(root: &Path, spec: &str, path: &str) -> bool {
    Command::new("git")
        .args(["cat-file", "-e", &format!("{spec}:{path}")])
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Does the repo have any commit at all? An unborn HEAD is not a broken
/// ref — it is a repo before its first commit, and diffing against it
/// means diffing against the empty tree.
pub fn head_exists(root: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn show_at_ref(root: &Path, spec: &str, path: &str) -> anyhow::Result<Vec<u8>> {
    let output = Command::new("git")
        .args(["show", &format!("{spec}:{path}")])
        .current_dir(root)
        .output()
        .map_err(|e| {
            anyhow::anyhow!("running git failed: {e} — diffing requires a git repository")
        })?;
    if !output.status.success() {
        anyhow::bail!("committed file `{path}` is unreadable at ref `{spec}`");
    }
    Ok(output.stdout)
}

fn digest_sharded_at_ref(root: &Path, spec: &str) -> anyhow::Result<Digest> {
    use crate::schema::map::{DigestIndex, DigestShard};

    let raw = show_at_ref(root, spec, ".harna/digest/index.json")?;
    let index: DigestIndex =
        serde_json::from_slice(&raw).context("parsing committed digest index")?;
    let mut modules = Vec::new();
    for shard in &index.shards {
        let raw = show_at_ref(root, spec, &format!(".harna/digest/{}", shard.file))?;
        let parsed: DigestShard =
            serde_json::from_slice(&raw).context("parsing committed digest shard")?;
        modules.extend(parsed.modules);
    }
    Ok(Digest {
        orphans: Vec::new(),
        generated_at: index.generated_at,
        extractor_version: index.extractor_version,
        modules,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(path: &str, sigs: &[&str], deps: &[&str]) -> Module {
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

    fn git(root: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git should be available for tests");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn option_shaped_refs_never_reach_git() {
        let root = std::env::temp_dir().join(format!("harna-diff-ref-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("written-by-git.txt");
        let spec = format!("--output={}", target.display());
        let err = digest_at_ref(&root, &spec).unwrap_err().to_string();
        assert!(err.contains("not a git ref"), "{err}");
        assert!(!target.exists(), "git must never have run with the option");
        for bad in ["", "-p", "HEAD foo", "a\tb"] {
            assert!(check_ref(bad).is_err(), "{bad:?}");
        }
        for good in [
            "HEAD",
            "HEAD~2",
            "v0.3.32",
            "origin/main",
            "499d6a2",
            "main@{1}",
        ] {
            assert!(check_ref(good).is_ok(), "{good}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unborn_head_diffs_against_the_empty_tree() {
        let root = std::env::temp_dir().join(format!(
            "harna-diff-unborn-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);

        let back = digest_at_ref(&root, "HEAD").unwrap();
        assert!(
            back.modules.is_empty(),
            "no commits yet: the baseline is nothing, not an error"
        );

        // A genuinely bad ref still errors — the empty tree is for unborn
        // HEAD only, never a mask for typos.
        assert!(
            digest_at_ref(&root, "HEAD~99").is_err(),
            "bad refs must still fail loudly"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn digest_at_ref_reads_legacy_then_sharded() -> anyhow::Result<()> {
        use crate::schema::model::manifest::{Container, Layer};
        use crate::schema::store::Store;

        let root = std::env::temp_dir().join(format!(
            "harna-diff-ref-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".harna"))?;
        git(&root, &["init", "-q"]);
        let store = Store::open(&root)?;
        let digest = Digest {
            orphans: Vec::new(),
            generated_at: "then".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![
                module("crates/t/src/one/a.rs", &["pub fn f()"], &[]),
                module("crates/t/src/stray.rs", &["pub fn g()"], &[]),
            ],
        };
        // Commit 1: the pre-0.3.15 monolith.
        std::fs::write(store.digest_path(), serde_json::to_string_pretty(&digest)?)?;
        git(&root, &["add", "."]);
        git(
            &root,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "-m",
                "legacy",
            ],
        );
        let back = digest_at_ref(&root, "HEAD")?;
        assert_eq!(back.modules.len(), 2, "legacy layout reads at its ref");

        // Commit 2: sharded (the writer migrates and removes the monolith).
        let manifest = crate::schema::model::manifest::Manifest {
            test_kinds: Vec::new(),
            schema_version: crate::SCHEMA_BASELINE,
            layers: vec![Layer {
                owners: Vec::new(),
                isolated: false,
                name: "l".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                role: None,
                containers: vec![Container {
                    name: "one".into(),
                    path: "crates/t/src/one".into(),
                    ..Default::default()
                }],
            }],
        };
        store.save_digest(&manifest, &digest)?;
        git(&root, &["add", "-A", "."]);
        git(
            &root,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "-m",
                "sharded",
            ],
        );
        let back = digest_at_ref(&root, "HEAD")?;
        assert_eq!(
            back.modules
                .iter()
                .map(|m| m.path.clone())
                .collect::<Vec<_>>(),
            vec![
                "crates/t/src/one/a.rs".to_string(),
                "crates/t/src/stray.rs".to_string()
            ],
            "sharded layout assembles at its ref, order kept"
        );
        // The old ref still reads the old layout: history is bilingual.
        let back = digest_at_ref(&root, "HEAD~1")?;
        assert_eq!(back.modules.len(), 2);
        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn diff_detects_every_class_of_change() {
        let before = Digest {
            orphans: Vec::new(),
            generated_at: "before".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![
                module("src/a.rs", &["pub fn one()", "pub fn two()"], &["crate::x"]),
                module("src/removed.rs", &["pub fn gone()"], &[]),
            ],
        };
        let after = Digest {
            orphans: Vec::new(),
            generated_at: "after".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![
                module(
                    "src/a.rs",
                    &["pub fn two()", "pub fn three()"],
                    &["crate::y"],
                ),
                module("src/new.rs", &["pub fn added()"], &[]),
            ],
        };
        let d = diff(&before, &after);
        assert_eq!(d.modules_added, vec!["src/new.rs"]);
        assert_eq!(d.modules_removed, vec!["src/removed.rs"]);
        assert!(d
            .signatures_added
            .contains(&("src/a.rs".into(), "pub fn three()".into())));
        assert!(d
            .signatures_removed
            .contains(&("src/a.rs".into(), "pub fn one()".into())));
        assert!(d
            .deps_added
            .contains(&("src/a.rs".into(), "crate::y".into())));
        assert!(d
            .deps_removed
            .contains(&("src/a.rs".into(), "crate::x".into())));
        assert!(!d.is_empty());
    }

    #[test]
    fn identical_digests_produce_empty_diff() {
        let a = Digest {
            orphans: Vec::new(),
            generated_at: "t1".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![module("src/a.rs", &["pub fn one()"], &[])],
        };
        let b = Digest {
            orphans: Vec::new(),
            generated_at: "t2".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: a.modules.clone(),
        };
        let d = diff(&a, &b);
        assert!(d.is_empty());
        assert!(d.render().contains("no structural change"));
    }

    #[test]
    fn signature_order_does_not_count_as_change() {
        let a = Digest {
            orphans: Vec::new(),
            generated_at: "t1".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![module("src/m.rs", &["pub fn a()", "pub fn b()"], &[])],
        };
        let b = Digest {
            orphans: Vec::new(),
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
            orphans: Vec::new(),
            generated_at: "t1".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![Module {
                reexports: Vec::new(),
                crate_dir: None,
                src_root: None,
                path: "src/m.rs".into(),
                lang: "rust".into(),
                signatures: vec!["pub fn f()".into()],
                deps: vec![],
                body_hash: "h1".into(),
                inventory: vec![],
                mods: vec![],
            }],
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
        let d2 = diff(
            &a,
            &Digest {
                orphans: Vec::new(),
                generated_at: "t3".into(),
                extractor_version: crate::search::digest::EXTRACTOR_VERSION,
                modules: a.modules.clone(),
            },
        );
        assert!(d2.is_empty());
    }
}
