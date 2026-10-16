//! Storage: everything harna knows lives in `<repo>/.harna/`, versioned in git.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::schema::digest_data::{Digest, DigestIndex, DigestShard, Module};
use crate::schema::manifest::{module_homes, Manifest};
use crate::schema::rules::Rules;

/// Handle to the `.harna/` directory of a repository.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

const MANIFEST: &str = "manifest.toml";
const RULES: &str = "rules.toml";
const DIGEST: &str = "digest.json";
const DECISIONS: &str = "decisions";
/// Score snapshots (0.3.13): one TOML per recording, timestamp-slugged.
const SCORES: &str = "scores";

fn manifest_starter() -> String {
    format!(
        "# harna manifest: layers -> containers -> components\n\
# data schema version; upgrade with `harna migrate`\nschema_version = {}\n",
        crate::SCHEMA_VERSION
    )
}

fn rules_starter() -> String {
    format!(
        "# harna rules: each rule has id, text, rationale, scope, origin\n\
# optional: env = \"dev\"|\"prod\"|... restricts a rule to one environment\n\
# optional: check = \"<check-id>\" binds documentation to enforcement\n\
schema_version = {}\n",
        crate::SCHEMA_VERSION
    )
}

/// The stock agent instructions `harna init` writes at the repo root —
/// agents know the usage contract the moment the project is initialized.
pub const AGENTS_TEMPLATE: &str = "\
# Agent instructions — harna

This project uses harna: agent instructions are data in `.harna/`
(manifest, rules, decisions, digest, parity), versioned with the code.

Flow: plan -> place -> code -> check -> align -> commit.

- Start every session by calling `harna_onboarding` (CLI: `harna digest`).
- PLAN before code: call `harna_place` with the idea; check
  `harna_dependents <module>` for anything it would touch.
- Prefer scaffolding: `harna_scaffold` (primitive | compound | data) —
  value-set literals live in data files via `--kind data`, never inline.
- After edits: `harna_validate` with `changed_only: true`. Fix violations.
- Structural changes: `harna_parity_open` -> `harna_parity_auto` ->
  resolve targets. Open debts gate validation.
- Persist conventions the moment they are agreed: `harna_add_rule`
  (optionally bind `check`), `harna_log_decision` for non-obvious choices.
- Before compaction or running out of context: record the current burst
  with `harna_handoff` (CLI `harna handoff`) — `harna_onboarding` carries it
  to the next session.
- Commit `.harna/` together with the code change it describes (atomic PRs).
- Do not hand-edit files under `.harna/`; use the tools.
";

impl Store {
    /// Open the store rooted at `root` (the repository root).
    pub fn open(root: &Path) -> anyhow::Result<Self> {
        Ok(Self { root: root.join(".harna") })
    }

    pub fn exists(&self) -> bool {
        self.root.is_dir()
    }

    /// Create `.harna/` with starter files (idempotent).
    pub fn init(&self) -> anyhow::Result<()> {
        fs::create_dir_all(self.decisions_dir())?;
        self.write_starter(self.manifest_path(), "manifest.toml", manifest_starter())?;
        self.write_starter(self.rules_path(), "rules.toml", rules_starter())?;
        Ok(())
    }

    /// Repository root (everything above `.harna/`).
    pub fn repo_root(&self) -> PathBuf {
        self.root
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.root.clone())
    }

    /// Write the stock AGENTS.md at the repo root if there is none (or if it
    /// exists but never mentions harna — appending the section then).
    /// Returns true when the file (or section) was created.
    pub fn ensure_agents_md(&self) -> anyhow::Result<bool> {
        let path = self.repo_root().join("AGENTS.md");
        let existing = fs::read_to_string(&path).ok();
        match existing {
            Some(body) => {
                if body.to_lowercase().contains("harna") {
                    Ok(false)
                } else {
                    let mut updated = body;
                    if !updated.ends_with('\n') {
                        updated.push('\n');
                    }
                    updated.push_str("\n---\n\n");
                    updated.push_str(AGENTS_TEMPLATE);
                    fs::write(&path, updated).with_context(|| format!("writing {}", path.display()))?;
                    Ok(true)
                }
            }
            None => {
                fs::write(&path, AGENTS_TEMPLATE)
                    .with_context(|| format!("writing {}", path.display()))?;
                Ok(true)
            }
        }
    }

    fn write_starter(&self, path: PathBuf, name: &str, body: String) -> anyhow::Result<()> {
        if !path.exists() {
            fs::write(path, body).with_context(|| format!("writing {name}"))?;
        }
        Ok(())
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root.join(MANIFEST)
    }

    pub fn rules_path(&self) -> PathBuf {
        self.root.join(RULES)
    }

    pub fn digest_path(&self) -> PathBuf {
        self.root.join(DIGEST)
    }

    pub fn decisions_dir(&self) -> PathBuf {
        self.root.join(DECISIONS)
    }

    /// Score snapshots live here (0.3.13): git-tracked like every `.harna/`
    /// ledger, so trends survive across sessions and machines.
    pub fn scores_dir(&self) -> PathBuf {
        self.root.join(SCORES)
    }

    /// Persist a score snapshot (0.3.13): `stem[_N].toml` under scores/.
    /// Recordings append, never replace — a stem collision takes the next
    /// `_N` suffix. The underscore keeps chronological filename order (`-`
    /// would sort before `.toml`'s dot and put the newer recording first).
    /// Persistence lives here, in the storage gateway, so composition
    /// briefings never touch the disk directly (gateway-ownership holds
    /// the `fs:write` monopoly to schema/generation/conformance).
    pub fn save_score_snapshot(&self, stem: &str, content: &str) -> anyhow::Result<PathBuf> {
        let dir = self.scores_dir();
        fs::create_dir_all(&dir)?;
        let mut n = 0u32;
        loop {
            let candidate = dir.join(format!(
                "{stem}{}.toml",
                if n == 0 { String::new() } else { format!("_{n}") }
            ));
            if !candidate.exists() {
                fs::write(&candidate, content)?;
                return Ok(candidate);
            }
            n += 1;
        }
    }

    /// Sorted `.toml` stems under scores/; a missing dir is no recordings,
    /// not an error.
    pub fn score_snapshot_names(&self) -> Vec<String> {
        let dir = self.scores_dir();
        let Ok(entries) = fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
            .filter_map(|p| {
                p.file_stem().map(|s| s.to_string_lossy().into_owned())
            })
            .collect();
        names.sort();
        names
    }

    /// Read one snapshot file back (parse errors fail loudly at the call
    /// site — silent rot would poison every trend drawn from it).
    pub fn read_score_snapshot(&self, name: &str) -> anyhow::Result<String> {
        let path = self.scores_dir().join(format!("{name}.toml"));
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
    }

    pub fn load_manifest(&self) -> anyhow::Result<Manifest> {
        let m: Manifest = read_toml(&self.manifest_path())?;
        check_schema_version(m.schema_version)?;
        Ok(m)
    }

    pub fn save_manifest(&self, m: &Manifest) -> anyhow::Result<()> {
        write_toml(&self.manifest_path(), m)
    }

    pub fn load_rules(&self) -> anyhow::Result<Rules> {
        let r: Rules = read_toml(&self.rules_path())?;
        check_schema_version(r.schema_version)?;
        Ok(r)
    }

    pub fn save_rules(&self, r: &Rules) -> anyhow::Result<()> {
        write_toml(&self.rules_path(), r)
    }

    /// Sharded digest directory (0.3.15): `index.json` plus one shard
    /// per container under `shards/`.
    pub fn digest_dir(&self) -> PathBuf {
        self.root.join("digest")
    }

    /// Load the code map: sharded layout first, legacy `digest.json`
    /// second (repos scanned before 0.3.15 keep working until their next
    /// scan migrates them forward).
    pub fn load_digest(&self) -> anyhow::Result<Digest> {
        let index_path = self.digest_dir().join("index.json");
        if index_path.is_file() {
            return self.load_sharded(&index_path);
        }
        let raw = fs::read_to_string(self.digest_path())
            .with_context(|| format!("reading {}", self.digest_path().display()))?;
        serde_json::from_str(&raw).context("parsing digest.json")
    }

    fn load_sharded(&self, index_path: &Path) -> anyhow::Result<Digest> {
        let raw = fs::read_to_string(index_path)
            .with_context(|| format!("reading {}", index_path.display()))?;
        let index: DigestIndex = serde_json::from_str(&raw)
            .with_context(|| format!("parsing {}", index_path.display()))?;
        let mut modules = Vec::new();
        for shard in &index.shards {
            let path = self.digest_dir().join(&shard.file);
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let parsed: DigestShard = serde_json::from_str(&raw)
                .with_context(|| format!("parsing {}", path.display()))?;
            modules.extend(parsed.modules);
        }
        Ok(Digest {
            generated_at: index.generated_at,
            extractor_version: index.extractor_version,
            modules,
        })
    }

    /// Save the code map sharded per container (0.3.15): `index.json`
    /// (stamps + compact per-module records) plus one shard file per
    /// container holding its modules in full, digest order preserved
    /// inside each shard and across the index — a sharded round-trip is
    /// module-for-module identical to the monolith. Modules outside every
    /// container ride the `_unhomed` shard. The legacy `digest.json` is
    /// removed on save: one truth, never two.
    pub fn save_digest(&self, manifest: &Manifest, d: &Digest) -> anyhow::Result<()> {
        use crate::schema::digest_data::{IndexModule, IndexShard};

        let homes = module_homes(manifest, d);
        // Container path -> (layer, name) for shard naming.
        let mut names: std::collections::BTreeMap<&str, (&str, &str)> =
            std::collections::BTreeMap::new();
        for layer in &manifest.layers {
            for c in &layer.containers {
                names.entry(c.path.as_str()).or_insert((layer.name.as_str(), c.name.as_str()));
            }
        }
        // First-seen container order; digest order inside each shard.
        let mut order: Vec<String> = Vec::new();
        let mut grouped: std::collections::BTreeMap<String, Vec<&Module>> =
            std::collections::BTreeMap::new();
        for m in &d.modules {
            let key = homes
                .get(m.path.as_str())
                .map(|h| h.container_path.to_string())
                .unwrap_or_else(|| "_unhomed".to_string());
            if !grouped.contains_key(&key) {
                order.push(key.clone());
            }
            grouped.entry(key).or_default().push(m);
        }
        let dir = self.digest_dir();
        let shards_dir = dir.join("shards");
        fs::create_dir_all(&shards_dir)?;
        let mut taken: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut index = DigestIndex {
            generated_at: d.generated_at.clone(),
            extractor_version: d.extractor_version,
            shards: Vec::new(),
        };
        for key in &order {
            let (layer, container) = if key == "_unhomed" {
                ("", "_unhomed")
            } else {
                names.get(key.as_str()).copied().unwrap_or(("", "_unhomed"))
            };
            let stem = shard_slug(&format!("{layer}-{container}"));
            let mut n = 0u32;
            let file = loop {
                let candidate = format!(
                    "shards/{stem}{}.json",
                    if n == 0 { String::new() } else { format!("_{n}") }
                );
                if !taken.contains(&candidate) {
                    taken.insert(candidate.clone());
                    break candidate;
                }
                n += 1;
            };
            let modules = &grouped[key.as_str()];
            let shard = DigestShard {
                modules: modules.iter().map(|m| (*m).clone()).collect(),
            };
            fs::write(dir.join(&file), serde_json::to_string_pretty(&shard)?)?;
            index.shards.push(IndexShard {
                layer: layer.into(),
                container: container.into(),
                file,
                modules: modules
                    .iter()
                    .map(|m| IndexModule { path: m.path.clone(), body_hash: m.body_hash.clone() })
                    .collect(),
            });
        }
        fs::write(dir.join("index.json"), serde_json::to_string_pretty(&index)?)?;
        let legacy = self.digest_path();
        if legacy.is_file() {
            fs::remove_file(&legacy)?;
        }
        Ok(())
    }

    /// Write a decision record and return its relative path. The optional
    /// `supersedes` points at the decision file this one replaces — the
    /// decision log renders the chain.
    pub fn save_decision(
        &self,
        title: &str,
        body: &str,
        supersedes: Option<&str>,
    ) -> anyhow::Result<PathBuf> {
        let slug: String = title
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect();
        let slug = slug.trim_matches('-').to_string();
        let file = self.decisions_dir().join(format!(
            "{}-{}.md",
            chrono::Utc::now().format("%Y-%m-%d"),
            if slug.is_empty() { "decision".into() } else { slug }
        ));
        let content = crate::schema::decisions::markdown(title, body, supersedes);
        fs::create_dir_all(&self.decisions_dir())?;
        fs::write(&file, content)?;
        Ok(file)
    }
}

fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

/// Filesystem-safe shard stem: lowercase alphanumerics survive, the rest
/// becomes `-` (same sanitizer shape as decision slugs).
fn shard_slug(raw: &str) -> String {
    let slug: String = raw
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "shard".into()
    } else {
        slug
    }
}

fn write_toml<T: serde::Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let raw = toml::to_string_pretty(value)?;
    fs::write(path, raw)?;
    Ok(())
}

/// A repo written by a *newer* harna must never be silently misread.
fn check_schema_version(found: u32) -> anyhow::Result<()> {
    if found > crate::SCHEMA_VERSION {
        anyhow::bail!(
            "this repository's .harna data uses schema v{found}, but this harna understands v{} — \
             update harna (`cargo install harna-cli` or download the new release) before touching this repo",
            crate::SCHEMA_VERSION
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent_and_loads_defaults() {
        let tmp = std::env::temp_dir().join(format!("harna-test-{}", std::process::id()));
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        store.init().unwrap(); // second call must not fail

        let m = store.load_manifest().unwrap();
        assert!(m.is_empty());
        let r = store.load_rules().unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn save_decision_records_the_supersedes_pointer() -> anyhow::Result<()> {
        let tmp =
            std::env::temp_dir().join(format!("harna-decision-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(tmp.join(".harna"));
        let store = Store::open(&tmp).unwrap();
        let path = store.save_decision("New plan", "replaces the old one", Some("old.md"))?;
        let written = std::fs::read_to_string(path)?;
        assert!(written.starts_with("# New plan"));
        assert!(written.contains("supersedes: old.md"));

        // Persists visibly: the decision-log parser reads it back through
        // the same store.
        let decisions = crate::schema::decisions::list(&store.decisions_dir())?;
        assert!(decisions
            .iter()
            .any(|d| d.supersedes.as_deref() == Some("old.md")));
        std::fs::remove_dir_all(tmp.join(".harna"))?;
        Ok(())
    }

    fn shard_manifest() -> Manifest {
        use crate::schema::manifest::{Container, Layer};

        let container = |name: &str, path: &str| Container {
            name: name.into(),
            path: path.into(),
            ..Default::default()
        };
        Manifest {
            schema_version: crate::SCHEMA_VERSION,
            layers: vec![
                Layer {
                    name: "l1".into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec![],
                    containers: vec![
                        container("one", "crates/t/src/one"),
                        container("two", "crates/t/src/two"),
                    ],
                },
                Layer {
                    name: "l2".into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec![],
                    containers: vec![container("uno", "crates/t/src/one/nested")],
                },
            ],
        }
    }

    fn shard_module(path: &str, hash: &str) -> Module {
        Module {
            path: path.into(),
            lang: "rust".into(),
            signatures: vec!["pub fn f()".into()],
            deps: vec![],
            body_hash: hash.into(),
            inventory: vec![],
            mods: vec![],
        }
    }

    fn shard_digest() -> Digest {
        Digest {
            generated_at: "then".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![
                shard_module("crates/t/src/one/a.rs", "ha"),
                // Nested container wins by longest prefix.
                shard_module("crates/t/src/one/nested/deep.rs", "hd"),
                shard_module("crates/t/src/two/b.rs", "hb"),
                shard_module("crates/t/src/stray.rs", "hs"),
            ],
        }
    }

    #[test]
    fn digest_shards_per_container_and_round_trips_identical() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!(
            "harna-shards-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp)?;
        let m = shard_manifest();
        let d = shard_digest();
        store.save_digest(&m, &d)?;

        // One shard per home (nested longest-prefix wins) plus _unhomed.
        let dir = store.digest_dir();
        assert!(dir.join("index.json").is_file());
        assert!(dir.join("shards/l1-one.json").is_file());
        assert!(dir.join("shards/l2-uno.json").is_file());
        assert!(dir.join("shards/l1-two.json").is_file());
        assert!(dir.join("shards/unhomed.json").is_file(), "slug trims to `unhomed`");

        // The index carries stamps plus compact per-module records.
        let raw = std::fs::read_to_string(dir.join("index.json"))?;
        let index: DigestIndex = serde_json::from_str(&raw)?;
        assert_eq!(index.generated_at, "then");
        assert_eq!(index.shards.len(), 4);
        let one = index.shards.iter().find(|s| s.container == "one").unwrap();
        assert_eq!(one.layer, "l1");
        assert_eq!(one.modules.len(), 1);
        assert_eq!(one.modules[0].body_hash, "ha", "reuse hash rides the index");

        // Module-for-module identical, order preserved.
        let back = store.load_digest()?;
        assert_eq!(back.generated_at, "then");
        assert_eq!(
            back.modules.iter().map(|x| x.path.clone()).collect::<Vec<_>>(),
            d.modules.iter().map(|x| x.path.clone()).collect::<Vec<_>>()
        );
        assert_eq!(back.modules[0].body_hash, "ha");
        let _ = std::fs::remove_dir_all(&tmp);
        Ok(())
    }

    #[test]
    fn legacy_monolith_loads_until_the_next_scan_migrates() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!(
            "harna-shards-legacy-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp)?;
        store.init()?;
        // A pre-0.3.15 repo: monolith present, no shards.
        let d = shard_digest();
        std::fs::write(store.digest_path(), serde_json::to_string_pretty(&d)?)?;
        let back = store.load_digest()?;
        assert_eq!(back.modules.len(), 4, "legacy fallback reads the monolith");
        // The next scan migrates forward and removes the monolith.
        store.save_digest(&shard_manifest(), &d)?;
        assert!(!store.digest_path().exists(), "one truth, never two");
        assert_eq!(store.load_digest()?.modules.len(), 4);
        let _ = std::fs::remove_dir_all(&tmp);
        Ok(())
    }

    #[test]
    fn empty_digest_writes_an_empty_index() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!(
            "harna-shards-empty-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp)?;
        let d = Digest {
            generated_at: "then".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![],
        };
        store.save_digest(&shard_manifest(), &d)?;
        let back = store.load_digest()?;
        assert!(back.modules.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
        Ok(())
    }
}
