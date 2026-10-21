//! Storage: everything harna knows lives in `<repo>/.harna/`, versioned in git.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::schema::map::{Digest, DigestIndex, DigestShard, DigestStamp, Module};
use crate::schema::model::manifest::{module_homes, Manifest};
use crate::schema::model::rules::Rules;

pub mod atomic;
pub mod contracts;
mod digest_io;
pub mod migrate;
pub mod plugins;
pub mod register_io;
pub mod services;

/// Handle to the `.harna/` directory of a repository.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

const MANIFEST: &str = "manifest.toml";
const RULES: &str = "rules.toml";
/// The `.gitattributes` line that keeps the code map LF on every checkout.
pub const MAP_EOL_LINE: &str = ".harna/digest/** text eol=lf";
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
/// The qualities register (0.4.0): long-term intent — what the software
/// must hold over its life. Absence is the natural empty state.
const QUALITIES: &str = "qualities.toml";
/// The advisory ledger (0.3.31): accepted Info findings with reasons.
/// Absence is the natural empty state (nothing acknowledged yet).
const ACKNOWLEDGED: &str = "acknowledged.toml";
/// The ratchet ledger (0.3.44): allowed error counts that only go down.
const BASELINES: &str = "baselines.toml";
/// The command registry (0.3.46): what to run, what it proves.
const COMMANDS: &str = "commands.toml";

/// Every entry harna reads or writes in `.harna/` (0.9.0): the doctor names
/// anything else (a typo reads as nothing), and docs/FORMAT.md lists each.
pub const KNOWN: &[&str] = &[
    MANIFEST,
    RULES,
    DIGEST,
    "digest",
    DECISIONS,
    SCORES,
    ARCHIVE,
    TEMPLATES,
    DEFERRED,
    TERMS,
    NON_GOALS,
    LESSONS,
    ROADMAP,
    QUALITIES,
    ACKNOWLEDGED,
    BASELINES,
    COMMANDS,
    "handoff.toml",
    "parity",
    "plugins",
    "contracts",
    "services.toml",
    "backup",
];

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
        crate::SCHEMA_BASELINE
    )
}

fn rules_starter() -> String {
    format!(
        "# harna rules: each rule has id, text, rationale, scope, origin\n\
# optional: env = \"dev\"|\"prod\"|... restricts a rule to one environment\n\
# optional: check = \"<check-id>\" binds documentation to enforcement\n\
schema_version = {}\n",
        crate::SCHEMA_BASELINE
    )
}

/// The stock agent instructions `harna init --agents` writes when the repo
/// has no AGENTS.md (0.3.47: never over a hand-written one). Each step
/// names its CLI command and its MCP tool (1.0.1, proof: agents on the CLI
/// guessed `harna decision add` from the MCP names alone).
pub const AGENTS_TEMPLATE: &str = "\
# Agent instructions — harna

This project keeps its architecture as data in `.harna/` with harna: layers
and containers, rules bound to checks, decisions, the roadmap, services.
Use the tools; do not hand-edit `.harna/`. Every step has a CLI command and,
when harna-mcp is wired, an MCP tool:

| Step | CLI | MCP |
|---|---|---|
| Start every session (a summary; add a section for detail) | `harna onboarding [rules\\|conventions\\|all]` | `harna_onboarding` |
| Before code: where an idea lives, what applies | `harna plan \"<idea>\"` | `harna_place` |
| Who imports a module you will change | `harna dependents <module>` | `harna_dependents` |
| New component, born wired and registered | `harna scaffold` | `harna_scaffold` |
| Declare a container over a folder | `harna container add <name> --layer <layer> --path <dir>` | `harna_manifest_edit` |
| After edits | `harna validate --changed` | `harna_validate` (`changed_only`) |
| Refresh the code map | `harna scan` | `harna_scan` |
| A structural change owes follow-ups | `harna parity open <id>`, then `resolve` | `harna_parity` |
| A convention is agreed | `harna add-rule --id .. --text .. --rationale ..` (`--replace` to change one) | `harna_add_rule` |
| A non-obvious choice | `harna log-decision --title .. --body ..` | `harna_log_decision` |
| Where the project is going | `harna roadmap list`, `harna roadmap set <id> done` | `harna_roadmap` |
| Before a release (after the tests) | `harna preflight` | `harna_preflight` |
| Before stopping or compaction | `harna handoff --summary .. --next ..` | `harna_handoff` |

How to work:
- Plan first; build in the stages the roadmap declares, not all at once.
- Born modular: scaffold the components first, then fill them; never write
  one oversized file intending to split it later.
- Data (lists of facts, prices, flags) lives in data files the code reads.
- Refusals (`harna non-goal add`) and dead ends (`harna lesson add`) are
  data too: read them before proposing, record them when they happen.
- `harna tools` lists the optional tools; `harna <command> --help` says how.
- Commit `.harna/` together with the code change it describes.
";

impl Store {
    /// Open the store rooted at `root` (the repository root). The root is
    /// normalized first (0.3.24): lexically resolved to its plain absolute
    /// form, and on Windows joined under the verbatim `\\?\` prefix once it
    /// pushes past 240 characters — deep application paths stay within the
    /// classic `MAX_PATH` reach of every fs call this store makes.
    pub fn open(root: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            root: normalize_root(root).join(".harna"),
        })
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

    /// Whether `.gitattributes` keeps the map's line endings as written.
    pub fn map_eol_pinned(&self) -> bool {
        fs::read_to_string(self.repo_root().join(".gitattributes"))
            .is_ok_and(|t| t.lines().any(|l| l.trim() == MAP_EOL_LINE))
    }

    /// Add [`MAP_EOL_LINE`] to `.gitattributes` (0.9.5, cost field test:
    /// under `core.autocrlf` git restored the map with CRLF, a scan wrote
    /// the same content back as LF, and `git status` stayed dirty). True
    /// when the line was added.
    pub fn pin_map_eol(&self) -> anyhow::Result<bool> {
        if self.map_eol_pinned() {
            return Ok(false);
        }
        let path = self.repo_root().join(".gitattributes");
        let mut text = fs::read_to_string(&path).unwrap_or_default();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(MAP_EOL_LINE);
        text.push('\n');
        atomic::write(&path, text)?;
        Ok(true)
    }

    /// The core registers that hold a project's own data — present and no
    /// longer the stock starter (0.4.9): `init --profile`/`--from` replace
    /// only starters, so a second init never wipes a living manifest.
    pub fn own_data(&self) -> Vec<&'static str> {
        let flat = |t: &str| t.replace("\r\n", "\n").trim().to_string();
        let own = |path: PathBuf, starter: String| {
            fs::read_to_string(path).is_ok_and(|t| flat(&t) != flat(&starter))
        };
        let mut v = Vec::new();
        if own(self.manifest_path(), manifest_starter()) {
            v.push(MANIFEST);
        }
        if own(self.rules_path(), rules_starter()) {
            v.push(RULES);
        }
        v
    }

    /// Repository root (everything above `.harna/`).
    pub fn repo_root(&self) -> PathBuf {
        self.root
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.root.clone())
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
                format!(
                    "{} is not a valid manifest.toml — profile refused",
                    manifest_src.display()
                )
            })?;
            applied.manifest = true;
        }
        let rules_src = base.join(RULES);
        if rules_src.is_file() {
            let src = fs::read_to_string(&rules_src)
                .with_context(|| format!("reading {}", rules_src.display()))?;
            toml::from_str::<crate::schema::model::rules::Rules>(&src).with_context(|| {
                format!(
                    "{} is not a valid rules.toml — profile refused",
                    rules_src.display()
                )
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

    /// Typed save that keeps the file's leading `#` comment header — the
    /// prose a human wrote above the data survives every scaffold/move.
    /// Refuse a save that would drop keys the file holds but harna does
    /// not know (0.9.2, field tests: `container set`, `add-rule` and
    /// `service ready` silently deleted a typo'd `chek`, which had already
    /// turned a rule advisory without a word). `loaded` is the file as
    /// harna reads it now.
    pub(crate) fn guard_unknown<T: serde::Serialize>(
        &self,
        path: &Path,
        loaded: anyhow::Result<T>,
    ) -> anyhow::Result<()> {
        let (Ok(raw), Ok(before)) = (fs::read_to_string(path), loaded) else {
            return Ok(()); // absent, or unreadable: other gates say so
        };
        let keys = migrate::dropped_keys(&raw, &before);
        if !keys.is_empty() {
            anyhow::bail!(
                "{} holds {} that harna does not know — saving would drop it. Fix the key (a typo?) or remove it, then retry (`harna migrate --check` names them all)",
                path.file_name().and_then(|n| n.to_str()).unwrap_or("the file"),
                keys.join(", ")
            );
        }
        Ok(())
    }

    pub fn save_manifest(&self, m: &Manifest) -> anyhow::Result<()> {
        self.guard_unknown(&self.manifest_path(), self.load_manifest())?;
        let path = self.manifest_path();
        let header = header_comments(&fs::read_to_string(&path).unwrap_or_default());
        // stamped with what the data needs (0.4.9): an older harna refuses
        // data it would strip, and still reads data it can hold
        let mut m = m.clone();
        m.schema_version = m.required_schema();
        let body = toml::to_string_pretty(&m)?;
        atomic::write(&path, keep_header(&header, &body))
    }

    pub fn load_rules(&self) -> anyhow::Result<Rules> {
        let r: Rules = read_toml(&self.rules_path())?;
        check_schema_version(r.schema_version)?;
        Ok(r)
    }

    pub fn save_rules(&self, r: &Rules) -> anyhow::Result<()> {
        self.guard_unknown(&self.rules_path(), self.load_rules())?;
        let mut r = r.clone();
        r.schema_version = r.required_schema();
        // the file's header comments survive an edit, as the manifest's do
        // (0.5.9 field test: add-rule dropped them)
        let path = self.rules_path();
        let header = header_comments(&fs::read_to_string(&path).unwrap_or_default());
        let body = toml::to_string_pretty(&r)?;
        atomic::write(&path, keep_header(&header, &body))
    }
}

fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let raw = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
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

/// The leading comment block of a TOML file (after an optional first
/// `schema_version` line), blank lines included.
fn header_comments(raw: &str) -> String {
    let mut out = String::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.starts_with('#') || t.is_empty() {
            out.push_str(line);
            out.push('\n');
        } else if t.starts_with("schema_version") && out.trim().is_empty() {
            continue;
        } else {
            break;
        }
    }
    if out.trim().is_empty() {
        String::new()
    } else {
        out
    }
}

/// Re-insert a header after the serialized `schema_version` line.
fn keep_header(header: &str, body: &str) -> String {
    if header.is_empty() {
        return body.to_string();
    }
    match body.split_once('\n') {
        Some((first, rest)) if first.starts_with("schema_version") => {
            format!(
                "{first}\n\n{}\n{}",
                header.trim_matches('\n'),
                rest.trim_start_matches('\n')
            )
        }
        _ => format!("{}\n\n{body}", header.trim_matches('\n')),
    }
}

/// A repo written by a *newer* harna must never be silently misread.
/// A file whose version is the baseline carries no stamp (0.9.0: parity
/// events and contracts gained one without changing a byte on disk).
pub fn baseline_schema() -> u32 {
    crate::SCHEMA_BASELINE
}

pub fn is_baseline_schema(v: &u32) -> bool {
    *v <= crate::SCHEMA_BASELINE
}

/// Every reader refuses data written by a newer harna instead of
/// misreading it (0.9.0: the format-freeze rule for every TOML file).
pub fn check_schema_version(found: u32) -> anyhow::Result<()> {
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
    let sep: char = if text.contains('\\') && !text.contains('/') {
        '\\'
    } else {
        '/'
    };
    let leading = text.starts_with(sep);
    let mut parts: Vec<&str> = Vec::new();
    for seg in text.split(['\\', '/']).filter(|s| !s.is_empty()) {
        match seg {
            "." => {}
            ".." => {
                // A drive head ("C:") or a leading separator marks root —
                // never pop past it.
                let head_reserved = parts.len() == 1 && (leading || parts[0].contains(':'));
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
