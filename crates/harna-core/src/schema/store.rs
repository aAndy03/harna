//! Storage: everything harna knows lives in `<repo>/.harna/`, versioned in git.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::search::digest::Digest;
use crate::schema::manifest::Manifest;
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

    pub fn load_digest(&self) -> anyhow::Result<Digest> {
        let raw = fs::read_to_string(self.digest_path())
            .with_context(|| format!("reading {}", self.digest_path().display()))?;
        serde_json::from_str(&raw).context("parsing digest.json")
    }

    pub fn save_digest(&self, d: &Digest) -> anyhow::Result<()> {
        let raw = serde_json::to_string_pretty(d)?;
        fs::write(self.digest_path(), raw)?;
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
        let decisions = crate::schema::decisions::list(&store)?;
        assert!(decisions
            .iter()
            .any(|d| d.supersedes.as_deref() == Some("old.md")));
        std::fs::remove_dir_all(tmp.join(".harna"))?;
        Ok(())
    }
}
