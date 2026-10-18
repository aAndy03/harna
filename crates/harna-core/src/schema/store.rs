//! Storage: everything harna knows lives in `<repo>/.harna/`, versioned in git.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::schema::atomic;
use crate::schema::digest_data::{Digest, DigestIndex, DigestShard, DigestStamp, Module};
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

    /// The parity templates file (0.3.22): declared fan-out shapes for
    /// recurring change types. Absence is the natural empty state —
    /// `load_templates` returns the default instead of an error.
    pub fn templates_path(&self) -> PathBuf {
        self.root.join(TEMPLATES)
    }

    /// Persist the parity templates document.
    pub fn save_templates(&self, t: &crate::schema::parity_data::ParityTemplates) -> anyhow::Result<()> {
        let raw = toml::to_string_pretty(t)?;
        atomic::write(self.templates_path(), raw)
            .with_context(|| format!("writing {}", TEMPLATES))?;
        Ok(())
    }

    /// Load the parity templates document; a missing file is an empty set
    /// (templates are adopted as data — until the first `template add`
    /// there is nothing declared and nothing to notice).
    pub fn load_templates(&self) -> anyhow::Result<crate::schema::parity_data::ParityTemplates> {
        if !self.templates_path().exists() {
            return Ok(Default::default());
        }
        let t: crate::schema::parity_data::ParityTemplates = read_toml(&self.templates_path())?;
        check_schema_version(t.schema_version)?;
        Ok(t)
    }

    /// The debt register file (0.3.23): deliberate deferrals with a
    /// review trigger. Absence is the natural empty state.
    pub fn deferred_path(&self) -> PathBuf {
        self.root.join(DEFERRED)
    }

    /// Persist the debt register document.
    pub fn save_deferred(
        &self,
        r: &crate::schema::deferred_data::DeferredRegister,
    ) -> anyhow::Result<()> {
        let raw = toml::to_string_pretty(r)?;
        atomic::write(self.deferred_path(), raw)
            .with_context(|| format!("writing {}", DEFERRED))?;
        Ok(())
    }

    /// Load the debt register document; a missing file is an empty set —
    /// a repo declares its deferrals as they happen, and no register
    /// means nothing was deferred. (An unparseable register errors here;
    /// callers choose to surface or skip.)
    pub fn load_deferred(&self) -> anyhow::Result<crate::schema::deferred_data::DeferredRegister> {
        if !self.deferred_path().exists() {
            return Ok(Default::default());
        }
        let r: crate::schema::deferred_data::DeferredRegister = read_toml(&self.deferred_path())?;
        check_schema_version(r.schema_version)?;
        Ok(r)
    }

    /// The glossary file (0.3.26): agent vocabulary -> harna vocabulary.
    /// Absence is the natural empty state.
    pub fn terms_path(&self) -> PathBuf {
        self.root.join(TERMS)
    }

    /// Persist the glossary document.
    pub fn save_terms(&self, t: &crate::schema::terms_data::Terms) -> anyhow::Result<()> {
        let raw = toml::to_string_pretty(t)?;
        atomic::write(self.terms_path(), raw)
            .with_context(|| format!("writing {}", TERMS))?;
        Ok(())
    }

    /// Load the glossary document; a missing file is an empty set —
    /// a repo grows its vocabulary as sessions reveal the mis-mapped
    /// words. (An unparseable glossary errors here; callers choose to
    /// surface or skip.)
    pub fn load_terms(&self) -> anyhow::Result<crate::schema::terms_data::Terms> {
        if !self.terms_path().exists() {
            return Ok(Default::default());
        }
        let t: crate::schema::terms_data::Terms = read_toml(&self.terms_path())?;
        check_schema_version(t.schema_version)?;
        Ok(t)
    }

    /// The decline register file (0.3.27): deliberate product refusals.
    /// Absence is the natural empty state.
    pub fn non_goals_path(&self) -> PathBuf {
        self.root.join(NON_GOALS)
    }

    /// Persist the decline register document.
    pub fn save_non_goals(&self, g: &crate::schema::non_goals_data::NonGoals) -> anyhow::Result<()> {
        let raw = toml::to_string_pretty(g)?;
        atomic::write(self.non_goals_path(), raw)
            .with_context(|| format!("writing {}", NON_GOALS))?;
        Ok(())
    }

    /// Load the decline register; a missing file is an empty set —
    /// refusals are declared when they are agreed. (Unparseable errors
    /// here; callers choose to surface or skip.)
    pub fn load_non_goals(&self) -> anyhow::Result<crate::schema::non_goals_data::NonGoals> {
        if !self.non_goals_path().exists() {
            return Ok(Default::default());
        }
        let g: crate::schema::non_goals_data::NonGoals = read_toml(&self.non_goals_path())?;
        check_schema_version(g.schema_version)?;
        Ok(g)
    }

    /// The dead-ends register file (0.3.27): tried-and-failed records
    /// with verdicts. Absence is the natural empty state.
    pub fn lessons_path(&self) -> PathBuf {
        self.root.join(LESSONS)
    }

    /// Persist the dead-ends register document.
    pub fn save_lessons(&self, l: &crate::schema::lessons_data::Lessons) -> anyhow::Result<()> {
        let raw = toml::to_string_pretty(l)?;
        atomic::write(self.lessons_path(), raw)
            .with_context(|| format!("writing {}", LESSONS))?;
        Ok(())
    }

    /// Load the dead-ends register; a missing file is an empty set —
    /// lessons are recorded when failures are owned. (Unparseable
    /// errors here; callers choose to surface or skip.)
    pub fn load_lessons(&self) -> anyhow::Result<crate::schema::lessons_data::Lessons> {
        if !self.lessons_path().exists() {
            return Ok(Default::default());
        }
        let l: crate::schema::lessons_data::Lessons = read_toml(&self.lessons_path())?;
        check_schema_version(l.schema_version)?;
        Ok(l)
    }

    /// The roadmap file (0.3.28): declared milestones, never
    /// generated. Absence is the natural empty state.
    pub fn roadmap_path(&self) -> PathBuf {
        self.root.join(ROADMAP)
    }

    /// Persist the roadmap document.
    pub fn save_roadmap(&self, r: &crate::schema::roadmap_data::Roadmap) -> anyhow::Result<()> {
        let raw = toml::to_string_pretty(r)?;
        atomic::write(self.roadmap_path(), raw)
            .with_context(|| format!("writing {}", ROADMAP))?;
        Ok(())
    }

    /// Load the roadmap; a missing file is an empty set — a project
    /// declares a roadmap when it wants one. (Unparseable errors here;
    /// callers choose to surface or skip.)
    pub fn load_roadmap(&self) -> anyhow::Result<crate::schema::roadmap_data::Roadmap> {
        if !self.roadmap_path().exists() {
            return Ok(Default::default());
        }
        let r: crate::schema::roadmap_data::Roadmap = read_toml(&self.roadmap_path())?;
        check_schema_version(r.schema_version)?;
        Ok(r)
    }

    /// The advisory ledger file (0.3.31): accepted Info findings with
    /// reasons. Absence is the natural empty state.
    pub fn acknowledged_path(&self) -> PathBuf {
        self.root.join(ACKNOWLEDGED)
    }

    /// Persist the advisory ledger document.
    pub fn save_acknowledged(
        &self,
        a: &crate::schema::acknowledged_data::Acknowledged,
    ) -> anyhow::Result<()> {
        let raw = toml::to_string_pretty(a)?;
        atomic::write(self.acknowledged_path(), raw)
            .with_context(|| format!("writing {}", ACKNOWLEDGED))?;
        Ok(())
    }

    /// Load the advisory ledger; a missing file is an empty set —
    /// nothing is acknowledged until someone accepts it in writing.
    /// (Unparseable errors here; callers choose to surface or skip.)
    pub fn load_acknowledged(
        &self,
    ) -> anyhow::Result<crate::schema::acknowledged_data::Acknowledged> {
        if !self.acknowledged_path().exists() {
            return Ok(Default::default());
        }
        let a: crate::schema::acknowledged_data::Acknowledged =
            read_toml(&self.acknowledged_path())?;
        check_schema_version(a.schema_version)?;
        Ok(a)
    }

    /// Move one live file into the archive's `kind` subdirectory and
    /// return the archive path. A same-volume rename — the bytes never
    /// change, only the surface does; that is the whole point (the file
    /// stays honest data, just not in the way).
    pub fn archive_file(&self, src: &Path, kind: &str) -> anyhow::Result<PathBuf> {
        let name = src
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("archiving {}: no file name", src.display()))?;
        let dir = self.archive_dir().join(kind);
        if !src.exists() {
            // Idempotent: already archived (or never live) is a no-op.
            return Ok(dir.join(name));
        }
        fs::create_dir_all(&dir)?;
        // An earlier archive with the same name keeps its bytes: the new
        // one takes the next free `_N` (rename would clobber it).
        let name = Path::new(name);
        let stem = name.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = name
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let dest = atomic::free_name(&dir, &stem, &ext);
        fs::rename(src, &dest)
            .with_context(|| format!("archiving {} -> {}", src.display(), dest.display()))?;
        Ok(dest)
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
                atomic::write(&candidate, content)?;
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

    /// The digest index alone (0.3.16): stamps plus compact per-module
    /// records — the whole path list at a fraction of the map's bytes.
    /// Sharded repos only; a missing index is the caller's signal to fall
    /// back (bounded reads do it via `for_each_shard`, the monolith's
    /// stamps live in the monolith itself).
    pub fn load_index(&self) -> anyhow::Result<DigestIndex> {
        let index_path = self.digest_dir().join("index.json");
        let raw = fs::read_to_string(&index_path)
            .with_context(|| format!("reading {}", index_path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", index_path.display()))
    }

    /// One shard file (0.3.16), `file` relative to `.harna/digest/` as the
    /// index records it: the parse unit is a container, never the map.
    pub fn load_shard(&self, file: &str) -> anyhow::Result<DigestShard> {
        let path = self.digest_dir().join(file);
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
    }

    /// Bounded module stream (0.3.16): visits every digest module in
    /// digest order, one shard in memory at a time — the visitor holds a
    /// `&Module` and decides what to keep, so a query never materializes
    /// the whole map. The legacy monolith joins the stream as a single
    /// pass (bilingual, like `load_digest`); its stamps ride along the
    /// same way the sharded index's do. The visitor returns `false` to
    /// stop early. Returns `false` when no map exists at all (the
    /// caller owns the "run `harna scan` first" refusal).
    pub fn for_each_shard(
        &self,
        mut visit: impl FnMut(&DigestStamp, &DigestShard) -> bool,
    ) -> anyhow::Result<bool> {
        let index_path = self.digest_dir().join("index.json");
        if index_path.is_file() {
            let index = self.load_index()?;
            let stamp = DigestStamp {
                generated_at: index.generated_at,
                extractor_version: index.extractor_version,
            };
            for shard in &index.shards {
                let parsed = self.load_shard(&shard.file)?;
                if !visit(&stamp, &parsed) {
                    return Ok(true);
                }
            }
            return Ok(true);
        }
        let path = self.digest_path();
        if !path.is_file() {
            return Ok(false);
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let d: Digest =
            serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
        let stamp = DigestStamp {
            generated_at: d.generated_at,
            extractor_version: d.extractor_version,
        };
        let shard = DigestShard { modules: d.modules };
        visit(&stamp, &shard);
        Ok(true)
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
            atomic::write(dir.join(&file), serde_json::to_string_pretty(&shard)?)?;
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
        atomic::write(dir.join("index.json"), serde_json::to_string_pretty(&index)?)?;
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
        let slug = slug(title, "decision");
        let content = crate::schema::decisions::markdown(title, body, supersedes);
        let dir = self.decisions_dir();
        fs::create_dir_all(&dir)?;
        // Two same-day decisions with one title take `_N` — a record is
        // appended, never silently replaced.
        let stem = format!("{}-{slug}", chrono::Utc::now().format("%Y-%m-%d"));
        let file = atomic::free_name(&dir, &stem, ".md");
        atomic::write_new(&file, content)?;
        Ok(file)
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
mod tests {
    use super::*;

    #[test]
    fn normalize_root_lexicalizes_and_prefixes_long_paths() {
        let v = format!("{}{}?{}", '\\', '\\', '\\'); // the verbatim head, backslash backslash question backslash

        // Short existing path: canonicalized, stays plain (no verbatim prefix).
        assert!(!normalize_root(Path::new(".")).to_string_lossy().starts_with(&v));

        // Lexical fallback for a NON-existing path: dots resolved, separators
        // joined to the path's own style, head never popped.
        let mixed = format!("some{}repo/sub/../deep", '\\');
        assert_eq!(
            normalize_root(Path::new(&mixed)).to_string_lossy(),
            "some/repo/deep",
            "mixed separators normalize onto the |/| style"
        );
        let stray = format!("..{}keep", '\\');
        assert_eq!(normalize_root(Path::new(&stray)).to_string_lossy(), stray);

        // A 60-segment path passes the prefix mark on Windows and rides the
        // verbatim form; short paths never grow one.
        let mut deep = String::from(if cfg!(windows) { format!("C:{}deep", '\\') } else { "/deep".into() });
        for i in 0..60 {
            deep.push_str(&format!("{}segment-segment-{:02}", '\\', i));
        }
        let normalized = normalize_root(Path::new(&deep));
        if cfg!(windows) {
            assert!(
                normalized.to_string_lossy().starts_with(&v),
                "long paths ride the verbatim form: {:?}",
                normalized
            );
        } else {
            assert!(normalized.to_string_lossy().starts_with("/deep"));
        }
        assert_eq!(
            normalize_root(Path::new("sub/here")).to_string_lossy(),
            "sub/here"
        );
    }

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
    fn the_glossary_round_trips_and_defaults_to_empty() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!("harna-terms-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(tmp.join(".harna"));
        let store = Store::open(&tmp)?;
        store.init()?;

        // No glossary yet: the empty state, not an error.
        assert!(store.load_terms()?.is_empty());

        let mut t = crate::schema::terms_data::Terms::default();
        t.terms.push(crate::schema::terms_data::Term {
            word: "engine".into(),
            aliases: vec!["service".into()],
            means: "owns a concern end-to-end".into(),
        });
        store.save_terms(&t)?;
        let back = store.load_terms()?;
        assert_eq!(back, t, "save -> load is byte-faithful");
        std::fs::remove_dir_all(tmp.join(".harna"))?;
        Ok(())
    }

    #[test]
    fn decline_and_deadend_registers_round_trip_and_default_to_empty() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!("harna-registers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(tmp.join(".harna"));
        let store = Store::open(&tmp)?;
        store.init()?;

        // Absent registers: the empty state, not an error.
        assert!(store.load_non_goals()?.is_empty());
        assert!(store.load_lessons()?.is_empty());

        let mut g = crate::schema::non_goals_data::NonGoals::default();
        g.upsert(crate::schema::non_goals_data::NonGoal {
            id: "no-remote-state".into(),
            refusal: "harna does not probe live production systems".into(),
            reason: "production awareness is a read-only surface".into(),
            recorded: "2026-10-17".into(),
        });
        store.save_non_goals(&g)?;
        assert_eq!(store.load_non_goals()?, g, "decline register is byte-faithful");

        let mut l = crate::schema::lessons_data::Lessons::default();
        l.upsert(crate::schema::lessons_data::Lesson {
            id: "ps-roundtrip".into(),
            claim: "bulk-edit sources via PowerShell".into(),
            verdict: "encoding corruption".into(),
            evidence: vec!["commit d4e81ed".into()],
            recorded: "2026-10-17".into(),
        });
        store.save_lessons(&l)?;
        assert_eq!(store.load_lessons()?, l, "dead-ends register is byte-faithful");
        std::fs::remove_dir_all(tmp.join(".harna"))?;
        Ok(())
    }

    #[test]
    fn the_roadmap_round_trips_and_defaults_to_empty() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!("harna-roadmap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(tmp.join(".harna"));
        let store = Store::open(&tmp)?;
        store.init()?;

        // No roadmap yet: the empty state, not an error.
        assert!(store.load_roadmap()?.is_empty());

        let mut r = crate::schema::roadmap_data::Roadmap::default();
        r.upsert(crate::schema::roadmap_data::Milestone {
            id: "0.3.28".into(),
            title: "roadmap as data".into(),
            state: "current".into(),
            notes: "declared, never generated".into(),
        });
        store.save_roadmap(&r)?;
        assert_eq!(store.load_roadmap()?, r, "save -> load is byte-faithful");
        std::fs::remove_dir_all(tmp.join(".harna"))?;
        Ok(())
    }

    #[test]
    fn the_advisory_ledger_round_trips_and_defaults_to_empty() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!("harna-ack-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(tmp.join(".harna"));
        let store = Store::open(&tmp)?;
        store.init()?;

        // Nothing acknowledged: the empty state, not an error.
        assert!(store.load_acknowledged()?.is_empty());

        let mut a = crate::schema::acknowledged_data::Acknowledged::default();
        a.upsert(crate::schema::acknowledged_data::Acknowledgement {
            check: "hotspot".into(),
            file: "big.rs".into(),
            reason: "tracked elsewhere".into(),
            recorded: "2026-10-17".into(),
        });
        store.save_acknowledged(&a)?;
        assert_eq!(store.load_acknowledged()?, a, "save -> load is byte-faithful");
        std::fs::remove_dir_all(tmp.join(".harna"))?;
        Ok(())
    }

    #[test]
    fn same_day_same_title_decisions_never_overwrite() -> anyhow::Result<()> {
        let tmp =
            std::env::temp_dir().join(format!("harna-decision-collide-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(tmp.join(".harna"));
        let store = Store::open(&tmp)?;
        let first = store.save_decision("Refactor", "first record", None)?;
        let second = store.save_decision("Refactor", "second record", None)?;
        assert_ne!(first, second);
        assert!(second.to_string_lossy().ends_with("-refactor_1.md"), "{second:?}");
        assert!(std::fs::read_to_string(&first)?.contains("first record"));
        assert!(std::fs::read_to_string(&second)?.contains("second record"));
        std::fs::remove_dir_all(tmp.join(".harna"))?;
        Ok(())
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
                      role: None,
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
                       role: None,
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

    #[test]
    fn bounded_stream_visits_in_digest_order_with_stamps_and_stops_early() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!(
            "harna-bounded-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp)?;
        store.save_digest(&shard_manifest(), &shard_digest())?;

        // Full stream: digest order across shards, stamps from the index.
        let mut seen: Vec<String> = Vec::new();
        let mut stamps = Vec::new();
        store.for_each_shard(|stamp, shard| {
            stamps.push((stamp.generated_at.clone(), stamp.extractor_version));
            for m in &shard.modules {
                seen.push(m.path.clone());
            }
            true
        })?;
        assert_eq!(
            seen,
            vec![
                "crates/t/src/one/a.rs",
                "crates/t/src/one/nested/deep.rs",
                "crates/t/src/two/b.rs",
                "crates/t/src/stray.rs",
            ]
        );
        assert!(stamps.iter().all(|(at, _)| at == "then"));
        assert!(stamps.iter().all(|(_, v)| *v == crate::search::digest::EXTRACTOR_VERSION));

        // Early stop: the visitor's false ends the stream after one module.
        let mut seen: Vec<String> = Vec::new();
        store.for_each_shard(|_, shard| {
            match shard.modules.first() {
                Some(m) => {
                    seen.push(m.path.clone());
                    false
                }
                None => true,
            }
        })?;
        assert_eq!(seen.len(), 1);
        let _ = std::fs::remove_dir_all(&tmp);
        Ok(())
    }

    #[test]
    fn profile_copies_rules_and_manifest_verbatim() -> anyhow::Result<()> {
        // A source repo's .harna with a distinctive rule + layer; init
        // --from copies them byte-honest so the adopter prunes real data.
        let src = std::env::temp_dir().join(format!(
            "harna-profile-src-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let dst = std::env::temp_dir().join(format!(
            "harna-profile-dst-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = (std::fs::remove_dir_all(&src), std::fs::remove_dir_all(&dst));
        std::fs::create_dir_all(src.join(".harna"))?;
        std::fs::write(
            src.join(".harna/rules.toml"),
            "[[rules]]\nid = \"src-rule\"\ntext = \"t\"\nrationale = \"why\"\norigin = \"profile test\"\n",
        )?;
        std::fs::write(
            src.join(".harna/manifest.toml"),
            "[[layers]]\nname = \"transport\"\n\n[[layers.containers]]\nname = \"wire\"\npath = \"src\"\n",
        )?;
        let home = Store::open(&dst)?;
        home.init()?;
        let applied = home.apply_profile(&src)?;

        assert!(applied.manifest && applied.rules);
        assert_eq!(home.load_rules()?.rules[0].id, "src-rule");
        let m = home.load_manifest()?;
        assert_eq!(m.layers[0].name, "transport");
        assert_eq!(m.layers[0].containers[0].name, "wire");
        let _ = (std::fs::remove_dir_all(&src), std::fs::remove_dir_all(&dst));
        Ok(())
    }

    #[test]
    fn profile_tolerates_missing_pieces_refuses_garbage() -> anyhow::Result<()> {
        let src = std::env::temp_dir().join(format!(
            "harna-profile-bad-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let dst = std::env::temp_dir().join(format!(
            "harna-profile-bad-dst-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = (std::fs::remove_dir_all(&src), std::fs::remove_dir_all(&dst));

        // Nothing there at all: clear refusal.
        let home = Store::open(&dst)?;
        home.init()?;
        assert!(home.apply_profile(&src).is_err());

        // Garbage manifest: refused BEFORE any overwrite — the starter
        // rules stay intact (a broken profile must not poison .harna/).
        std::fs::create_dir_all(src.join(".harna"))?;
        std::fs::write(src.join(".harna/manifest.toml"), "not [ valid toml {{{")?;
        let applied = home.apply_profile(&src);
        assert!(applied.is_err());
        assert!(home.load_rules()?.rules.is_empty(), "the starter rules survive a refused profile");
        let _ = (std::fs::remove_dir_all(&src), std::fs::remove_dir_all(&dst));
        Ok(())
    }

    #[test]
    fn archive_file_renames_the_bytes_out_of_the_live_surface() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!(
            "harna-archive-file-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp)?;
        store.init()?;
        let src = store.scores_dir().join("demo.toml");
        std::fs::create_dir_all(store.scores_dir())?;
        std::fs::write(&src, "x = 1")?;

        let dest = store.archive_file(&src, "scores")?;
        assert!(dest.ends_with("archive/scores/demo.toml"));
        assert!(!src.exists(), "the live surface is lean");
        let filed = std::fs::read_to_string(&dest)?;
        assert_eq!(filed, "x = 1", "a rename: the bytes never change");
        // Idempotent: archiving a missing source is a no-op returning the path.
        let again = store.archive_file(&src, "scores")?;
        assert!(again.ends_with("archive/scores/demo.toml"));
        // A second live file with the same name never clobbers the first
        // archive: it takes the next `_N`.
        std::fs::write(&src, "x = 2")?;
        let second = store.archive_file(&src, "scores")?;
        assert!(second.ends_with("archive/scores/demo_1.toml"), "{second:?}");
        assert_eq!(std::fs::read_to_string(&dest)?, "x = 1", "first archive intact");
        assert_eq!(std::fs::read_to_string(&second)?, "x = 2");
        let _ = std::fs::remove_dir_all(&tmp);
        Ok(())
    }

    #[test]
    fn bounded_stream_is_bilingual_and_reports_missing_maps() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!(
            "harna-bounded-legacy-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp)?;
        store.init()?;

        // No map at all: Ok(false), the caller owns the scan refusal.
        assert!(!store.for_each_shard(|_, _| true)?);

        // A pre-0.3.15 repo: the monolith streams as a single pass and its
        // stamps ride along the same way the index's do.
        std::fs::write(store.digest_path(), serde_json::to_string_pretty(&shard_digest())?)?;
        let mut passes = 0usize;
        let mut seen: Vec<String> = Vec::new();
        store.for_each_shard(|stamp, shard| {
            passes += 1;
            assert_eq!(stamp.generated_at, "then");
            for m in &shard.modules {
                seen.push(m.path.clone());
            }
            true
        })?;
        assert_eq!(passes, 1);
        assert_eq!(seen.len(), 4);
        let _ = std::fs::remove_dir_all(&tmp);
        Ok(())
    }
}
