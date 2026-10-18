//! Storage: everything harna knows lives in `<repo>/.harna/`, versioned in git.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::schema::atomic;
use crate::schema::digest_data::{Digest, DigestIndex, DigestShard, DigestStamp, Module};
use crate::schema::manifest::{module_homes, Manifest};
use crate::schema::rules::Rules;

mod digest_io;
mod registers;

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
/// Filed state (0.3.18): closed parity events, pruned score snapshots and
/// superseded decisions move here, per kind.
const ARCHIVE: &str = "archive";
/// Parity templates (0.3.22): declared fan-out shapes for recurring
/// change types.
const TEMPLATES: &str = "templates.toml";
/// The debt register (0.3.23): deliberate deferrals with a review
/// trigger — promises to reconsider, not parity debts.
const DEFERRED: &str = "deferred.toml";
/// The glossary (0.3.26): agent vocabulary -> harna vocabulary.
/// Absence is the natural empty state.
const TERMS: &str = "terms.toml";
/// The decline register (0.3.27): deliberate product refusals.
/// Absence is the natural empty state.
const NON_GOALS: &str = "non-goals.toml";
/// The dead-ends register (0.3.27): tried-and-failed records with
/// verdicts. Absence is the natural empty state.
const LESSONS: &str = "lessons.toml";
/// The roadmap (0.3.28): declared milestones — never generated.
/// Absence is the natural empty state (a project may not want one).
const ROADMAP: &str = "roadmap.toml";
/// The advisory ledger (0.3.31): accepted Info findings with reasons.
/// Absence is the natural empty state (nothing acknowledged yet).
const ACKNOWLEDGED: &str = "acknowledged.toml";

/// What `apply_profile` actually copied over the starters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProfileApplied {
    /// The source's manifest.toml replaced the starter.
    pub manifest: bool,
    /// The source's rules.toml replaced the starter.
    pub rules: bool,
}

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
- Born modular: per-scope files from the first line. Scaffold the
  components first, THEN fill them — never write one oversized file
  intending to split it later (0.3.26 gates this at commit: `born-size`).
- Vocabulary: harna's terms are fixed points (`.harna/terms.toml`).
  An agent's own words may map differently — check the glossary before
  assuming a disagreement; aliases are harvested, meanings are not
  negotiable.
- After edits: `harna_validate` with `changed_only: true`. Fix violations.
- Structural changes: `harna_parity_open` -> `harna_parity_auto` ->
  resolve targets. Open debts gate validation.
- Persist conventions the moment they are agreed: `harna_add_rule`
  (optionally bind `check`), `harna_log_decision` for non-obvious choices.
- Decline explicitly: `harna non-goal add` records a deliberate product
  refusal (its data carries what is refused and why) — plan/place refuse
  matching proposals until the user supersedes it; `harna lesson add`
  records tried-and-failed directions with verdicts and evidence so new
  sessions never re-suggest them.
- Roadmap (0.3.28): `.harna/roadmap.toml` carries declared milestones and
  their states (declared|current|done|dropped). Asked where the project
  stands and what is next, read it before answering; keep `current`
  truthful (set `harna roadmap set <id> done` when shipped); deferred
  reviews may key to a milestone id instead of a date or version.
- Release gate (0.3.29): `harna preflight` composes validation + doctor +
  parity + map into one PASS/FAIL — run it after the suite, before the
  commit. It never runs the suite itself (harna executes no project
  code); a FAIL exits 2.
- Truthfulness (0.3.30): `harna validate-intent` cross-checks what the
  records claim (package version, changelog head, roadmap NOW/DONEs,
  parity silence) against the state (files, tags, ledger) — DRIFTs name
  both sides; run it whenever coherence is claimed, not just at release.
- Seams (0.3.31): `harna acknowledge add <check> --file <f> --reason`
  locks an accepted advisory (errors never lock); `harna decompose
  <file>` plans the split before you cut; a manifest path missing with
  its file name alive elsewhere is a rename-linkage error naming the
  move.
- Honesty (0.3.32): `validate --changed` sees untracked files
  (untracked dirs expand); re-resolving parity warns what it
  overwrites; granularity names its number; agent-wiring counts the
  harna-mcp binary.
- Before compaction or running out of context: record the current burst
  with `harna_handoff` (CLI `harna handoff`) — `harna_onboarding` carries it
  to the next session.
- Commit `.harna/` together with the code change it describes (atomic PRs).
- Do not hand-edit files under `.harna/`; use the tools.

Autonomous ritual (trusted agent, auto-mode — every step is an audit
point a human can replay): plan -> scaffold -> implement -> scan ->
parity open -> resolve (real acks + noops) -> validate --changed ->
handoff -> atomic commit.
";

impl Store {
    /// Open the store rooted at `root` (the repository root). The root is
    /// normalized first (0.3.24): lexically resolved to its plain absolute
    /// form, and on Windows joined under the verbatim `\\?\` prefix once it
    /// pushes past 240 characters — deep application paths stay within the
    /// classic `MAX_PATH` reach of every fs call this store makes.
    pub fn open(root: &Path) -> anyhow::Result<Self> {
        Ok(Self { root: normalize_root(root).join(".harna") })
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
                    atomic::write(&path, updated).with_context(|| format!("writing {}", path.display()))?;
                    Ok(true)
                }
            }
            None => {
                atomic::write(&path, AGENTS_TEMPLATE)
                    .with_context(|| format!("writing {}", path.display()))?;
                Ok(true)
            }
        }
    }

    fn write_starter(&self, path: PathBuf, name: &str, body: String) -> anyhow::Result<()> {
        if !path.exists() {
            atomic::write(path, body).with_context(|| format!("writing {name}"))?;
        }
        Ok(())
    }

    /// Copy another repo's profile over the freshly written starters
    /// (0.3.21 `init --from`): a profile is the source repo's
    /// `rules.toml` + `manifest.toml`, copied **verbatim** as a starting
    /// point to prune — not a template to re-render, never reinterpreted.
    /// `from` may be the repo root (reads `<from>/.harna/…`) or a
    /// `.harna/` directory itself. A piece the source does not have
    /// leaves the starter in place; a source piece that fails to parse
    /// is refused BEFORE anything is overwritten (a broken profile must
    /// not become your data). Returns what actually landed.
    pub fn apply_profile(&self, from: &Path) -> anyhow::Result<ProfileApplied> {
        let base = if from.join(".harna").is_dir() {
            from.join(".harna")
        } else if from.join(MANIFEST).is_file() || from.join(RULES).is_file() {
            from.to_path_buf()
        } else {
            anyhow::bail!(
                "no profile at {} — expected a repo root with .harna/ or a directory holding manifest.toml/rules.toml",
                from.display()
            );
        };

        // Validate the source pieces first: a profile that would not
        // load must never reach .harna/ (refuse, not rescue).
        let mut applied = ProfileApplied::default();
        let manifest_src = base.join(MANIFEST);
        if manifest_src.is_file() {
            let src = fs::read_to_string(&manifest_src)
                .with_context(|| format!("reading {}", manifest_src.display()))?;
            toml::from_str::<Manifest>(&src).with_context(|| {
                format!("{} is not a valid manifest.toml — profile refused", manifest_src.display())
            })?;
            applied.manifest = true;
        }
        let rules_src = base.join(RULES);
        if rules_src.is_file() {
            let src = fs::read_to_string(&rules_src)
                .with_context(|| format!("reading {}", rules_src.display()))?;
            toml::from_str::<crate::schema::rules::Rules>(&src).with_context(|| {
                format!("{} is not a valid rules.toml — profile refused", rules_src.display())
            })?;
            applied.rules = true;
        }
        if applied.manifest {
            fs::copy(&manifest_src, self.manifest_path())
                .with_context(|| format!("copying {}", manifest_src.display()))?;
        }
        if applied.rules {
            fs::copy(&rules_src, self.rules_path())
                .with_context(|| format!("copying {}", rules_src.display()))?;
        }
        Ok(applied)
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

    /// Archive directory (0.3.18): filed state lives under
    /// `.harna/archive/<kind>/` — closed parity events, pruned score
    /// snapshots, superseded decisions. Git-tracked like every ledger
    /// (history stays greppable), out of the live surface.
    pub fn archive_dir(&self) -> PathBuf {
        self.root.join(ARCHIVE)
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

}

fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

/// Filesystem-safe stem: lowercase alphanumerics survive, the rest
/// becomes `-`; an empty result takes `fallback`. Shared by shard stems
/// and decision slugs.
fn slug(raw: &str, fallback: &str) -> String {
    let slug: String = raw
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        fallback.into()
    } else {
        slug
    }
}

fn shard_slug(raw: &str) -> String {
    slug(raw, "shard")
}

fn write_toml<T: serde::Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let raw = toml::to_string_pretty(value)?;
    atomic::write(path, raw)?;
    Ok(())
}

/// A repo written by a *newer* harna must never be silently misread.
fn check_schema_version(found: u32) -> anyhow::Result<()> {
    if found > crate::SCHEMA_VERSION {
        anyhow::bail!(
            "this repository's .harna data uses schema v{found}, but this harna understands v{} - \
             update harna (`cargo install harna-cli` or download the new release) before touching this repo",
            crate::SCHEMA_VERSION
        );
    }
    Ok(())
}

/// Past this character count, a plain Windows path needs the verbatim
/// `\\?\` form for `fs` calls to keep working (classic MAX_PATH = 260).
pub(crate) const LONG_PATH_PREFIX_AT: usize = 240;

/// Normalize a repository root (0.3.24 hardening):
/// 1. `canonicalize` when the path exists (resolves symlinks, `.` and `..`),
///    falling back to the raw path pre-init — the tool must open repos
///    before anything on disk exists.
/// 2. Reduce canonicalize's Windows verbatim form (`\\?\C:\…`,
///    `\\?\UNC\server\share`) back to the readable plain form — display,
///    tests and messages keep comparing straightforward strings.
/// 3. Re-apply the verbatim prefix only when the plain length passes the
///    `LONG_PATH_PREFIX_AT` mark, keeping long-path robustness without
///    changing how everyday paths look.
pub(crate) fn normalize_root(root: &Path) -> PathBuf {
    let exists = root.exists();
    let canonical = if exists {
        fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
    } else {
        root.to_path_buf()
    };
    let text = canonical.to_string_lossy().into_owned();
    let plain = if let Some(rest) = text.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{rest}")
    } else if let Some(rest) = text.strip_prefix("\\\\?\\") {
        rest.to_string()
    } else if exists {
        resolve_dots(&text)
    } else {
        // Non-existing: keep caller's form, still clean `.`/`..` lexically
        // relative to the resolved head.
        resolve_dots(&text)
    };
    if plain.len() > LONG_PATH_PREFIX_AT
        && !plain.starts_with("\\\\?\\")
        && plain.len() >= 2
        && plain.as_bytes()[1] == b':'
    {
        format!("\\\\?\\{plain}").into()
    } else {
        PathBuf::from(plain)
    }
}

/// Lexically resolve `.`/`..` segments without fs access (the canonicalize
/// fallback for roots that don't exist yet): the head of the path is never
/// popped by `..`, and the dominant separator is preserved.
fn resolve_dots(text: &str) -> String {
    let sep: char = if text.contains('\\') && !text.contains('/') { '\\' } else { '/' };
    let leading = text.starts_with(sep);
    let mut parts: Vec<&str> = Vec::new();
    for seg in text.split(['\\', '/']).filter(|s| !s.is_empty()) {
        match seg {
            "." => {}
            ".." => {
                // A drive head ("C:") or a leading separator marks root —
                // never pop past it.
                let head_reserved =
                    parts.len() == 1 && (leading || parts[0].contains(':'));
                if head_reserved {
                    continue;
                }
                // No base to pop INTO (relative `..\x`): the dots survive —
                // dropping them would silently change the path's meaning.
                if parts.pop().is_none() {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join(&sep.to_string());
    if leading && !joined.is_empty() {
        format!("{sep}{joined}")
    } else if joined.is_empty() {
        ".".into()
    } else {
        joined
    }
}
#[cfg(test)]
mod tests;
