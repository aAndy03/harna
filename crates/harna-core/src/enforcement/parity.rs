//! Component `parity`: the change-propagation ledger.
//!
//! Philosophy: a change never lands in isolation, but what propagates is
//! *coupling*, not distance. A parity event computes its fan-out from the
//! digest diff — changed public surface and the importers of changed modules —
//! and every target must end as `ack` (work done), `noop` (nothing to do,
//! with a reason), or `auto` (harna verified the target's surface is
//! unchanged across the generation).
//!
//! Revisions: re-opening the same id with a new diff bumps its `generation`
//! and invalidates every old acknowledgment; unchanged targets are
//! re-verified automatically (no re-ack tax).
//!
//! The ledger lives in `.harna/parity/<id>.toml`, git-tracked like every
//! other harna datum: a debt is greppable, blameable, reviewable.

use anyhow::Context as _;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::schema::store::Store;
use crate::search::diff::{self, DigestDiff};
use crate::search::digest::{self, Digest};

/// The statuses a parity target can carry live in the data file
/// `statuses.toml` (constants-are-data); this module imports the loader.
use crate::enforcement::violation::{Severity, Violation};

mod auto;
pub mod history;
pub mod policy;
mod resolve;
pub mod statuses;
mod views;
pub use auto::*;
pub use resolve::*;
pub use views::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParityEvent {
    pub id: String,
    /// What kind of change: "digest-diff", "rule-change", "manual", ...
    #[serde(default = "default_kind")]
    pub kind: String,
    /// One-line description of the source change.
    #[serde(default)]
    pub source: String,
    /// Bumped on every re-open; acknowledgments are valid for one generation.
    pub generation: u32,
    /// Git ref whose committed digest the fan-out was computed against.
    pub baseline_ref: String,
    pub created: String,
    pub updated: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<ParityTarget>,
}

fn default_kind() -> String {
    "digest-diff".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParityTarget {
    /// Module path relative to repo root (same identity as digest modules).
    pub path: String,
    /// Why this target is in the fan-out.
    #[serde(default)]
    pub reason: String,
    /// open | ack | noop | auto
    pub status: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub by: String,
    #[serde(default)]
    pub when: String,
    /// Importer of a breaking upstream change (0.3.35): `auto` never
    /// waives it — only a human ack/noop resolves it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub breaking: bool,
    /// Earlier verdicts this target carried, oldest first (0.3.43: a
    /// re-resolve or a re-open used to overwrite the note with no trace).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<history::Amendment>,
}

impl ParityEvent {
    pub fn open_targets(&self) -> Vec<&ParityTarget> {
        self.targets.iter().filter(|t| t.status == "open").collect()
    }

    pub fn target(&self, path: &str) -> Option<&ParityTarget> {
        self.targets.iter().find(|t| t.path == path)
    }

    /// Counts by resolution: (open, origin, auto, hand-resolved ack/noop).
    pub fn tally(&self) -> (usize, usize, usize, usize) {
        let n = |s: &str| self.targets.iter().filter(|t| t.status == s).count();
        let (open, origin, auto) = (n("open"), n("origin"), n("auto"));
        (
            open,
            origin,
            auto,
            self.targets.len() - open - origin - auto,
        )
    }

    /// One line after `parity open` (0.3.42, font-manager: "0 open / 7
    /// targets" read as "no targets"): the fan-out size first, then what is
    /// owed and how the rest resolved.
    pub fn summary(&self) -> String {
        let (open, origin, auto, hand) = self.tally();
        let owed = if open == 0 {
            "0 open — nothing owed".to_string()
        } else {
            format!("{open} open — owed: ack or noop each")
        };
        format!(
            "parity `{}` gen{}: {} target(s): {owed}; {origin} origin (your change), {auto} auto-resolved, {hand} hand-resolved",
            self.id,
            self.generation,
            self.targets.len()
        )
    }

    /// A closed event in one line: history, not a work list.
    pub fn render_closed(&self) -> String {
        let (_, origin, auto, hand) = self.tally();
        format!(
            "[gen{}] {} ({}): closed — {} target(s): {origin} origin, {auto} auto, {hand} hand-resolved\n",
            self.generation,
            self.id,
            self.kind,
            self.targets.len()
        )
    }

    /// Debt view: only the still-unresolved targets with their reasons.
    pub fn render_open(&self) -> String {
        if self.open_targets().is_empty() {
            return format!("parity `{}`: nothing open\n", self.id);
        }
        let open = self.open_targets().len();
        let mut out = format!(
            "[gen{}] {} ({}): {} open — {}, baseline `{}`\n",
            self.generation, self.id, self.kind, open, self.source, self.baseline_ref
        );
        for t in self.open_targets() {
            out.push_str(&format!(
                "  ! open {:4} {}  {}{}\n",
                "",
                t.path,
                t.reason,
                if t.note.is_empty() {
                    String::new()
                } else {
                    format!("  — {}", t.note)
                }
            ));
        }
        out
    }

    /// Token-efficient rendering for agent consumption.
    pub fn render(&self) -> String {
        let open = self.open_targets().len();
        let mut out = format!(
            "[gen{}] {} ({}): {} open / {} resolved — {}, baseline `{}`\n",
            self.generation,
            self.id,
            self.kind,
            open,
            self.targets.len() - open,
            self.source,
            self.baseline_ref
        );
        for t in &self.targets {
            let mark = match t.status.as_str() {
                "open" => "!",
                "auto" => "✓",
                "origin" => "·",
                _ => "-",
            };
            out.push_str(&format!(
                "  {mark} {:4} {}  {}{}\n",
                t.status,
                t.path,
                t.reason,
                if t.note.is_empty() {
                    String::new()
                } else {
                    format!("  — {}", t.note)
                }
            ));
            // Resolution provenance matters on manual decisions.
            if (t.status == "ack" || t.status == "noop") && !t.by.is_empty() {
                out.push_str(&format!(
                    "      resolved by {} @ {}\n",
                    t.by,
                    t.when.get(..10).unwrap_or("")
                ));
            }
            out.push_str(&history::render(&t.history));
        }
        out
    }
}

/// Open a new parity event, or revise an existing one with a fresh diff.
pub fn open(
    root: &Path,
    store: &Store,
    id: &str,
    kind: &str,
    source: &str,
    baseline_ref: &str,
    max_files: usize,
) -> anyhow::Result<ParityEvent> {
    let baseline = diff::digest_at_ref(root, baseline_ref)?;
    let fresh = digest::build(root, max_files)?;
    let crate_names = digest::workspace_crate_names(root);
    let d = diff::diff(&baseline, &fresh);
    open_from_digests(
        store,
        id,
        kind,
        source,
        baseline_ref,
        &baseline,
        &fresh,
        &d,
        &crate_names,
    )
}

/// Fan-out computation over two already-built digests — the testable core.
// Nine inputs on purpose: this is the seam tests drive directly (store,
// identity, provenance, both digests, the diff, workspace crates); a
// wrapper struct would only rename the same nine fields.
#[allow(
    clippy::too_many_arguments,
    reason = "the parity event constructor takes the whole diff context; a struct would only rename the list"
)]
pub fn open_from_digests(
    store: &Store,
    id: &str,
    kind: &str,
    source: &str,
    baseline_ref: &str,
    baseline: &Digest,
    fresh: &Digest,
    d: &DigestDiff,
    crate_names: &[String],
) -> anyhow::Result<ParityEvent> {
    if id.is_empty() {
        anyhow::bail!("parity event id must not be empty");
    }
    // Before any work: a traversing id must never reach the disk.
    check_id(id)?;
    store.init()?;
    let parity_dir = parity_dir(store);
    std::fs::create_dir_all(&parity_dir)?;

    // Changed modules -> human/agent-readable reasons.
    let mut reasons: HashMap<String, Vec<String>> = HashMap::new();
    for path in &d.modules_added {
        push_reason(&mut reasons, path, "module added");
    }
    for path in &d.modules_removed {
        push_reason(&mut reasons, path, "module removed");
    }
    for (path, sig) in &d.signatures_added {
        push_reason(&mut reasons, path, &format!("signature added: {sig}"));
    }
    for (path, sig) in &d.signatures_removed {
        // A split moves signatures into the module's own family; that is
        // not a breakage for importers (parity_policy::moved_within_family).
        let why = match policy::moved_within_family(path, sig, fresh) {
            Some(to) => format!("signature moved to {to}: {sig}"),
            None => format!("signature removed: {sig}"),
        };
        push_reason(&mut reasons, path, &why);
    }
    for (path, dep) in &d.deps_added {
        push_reason(&mut reasons, path, &format!("dep added: {dep}"));
    }
    for (path, dep) in &d.deps_removed {
        push_reason(&mut reasons, path, &format!("dep removed: {dep}"));
    }

    // Ring 2: importers of every changed module (from whichever digest still
    // knows them — a removed module's importers only exist in the baseline).
    // Each importer also learns which of its upstream changes are breaking.
    let mut importers: HashMap<String, Vec<String>> = HashMap::new();
    let mut upstream_breaking: HashMap<String, Vec<String>> = HashMap::new();
    // Crate-aware reach (0.3.44): the name -> dir index over both maps.
    let name_dirs = {
        let root = store
            .manifest_path()
            .parent()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let dirs: std::collections::BTreeSet<String> = fresh
            .modules
            .iter()
            .chain(baseline.modules.iter())
            .filter_map(|m| m.crate_of().0)
            .collect();
        digest::crate_name_dirs(&root, &dirs.into_iter().collect::<Vec<_>>())
    };
    for changed in reasons.keys() {
        let breaking_here: Vec<String> = reasons[changed]
            .iter()
            .filter(|r| policy::is_breaking(r))
            .map(|r| format!("{changed}: {r}"))
            .collect();
        let digest_for = if fresh.modules.iter().any(|m| &m.path == changed) {
            fresh
        } else {
            baseline
        };
        let Some(target) = digest_for.modules.iter().find(|m| &m.path == changed) else {
            continue;
        };
        for m in &digest_for.modules {
            if &m.path == changed {
                continue;
            }
            if m.deps
                .iter()
                .any(|dep| digest::dep_reaches(m, dep, target, crate_names, &name_dirs))
            {
                importers
                    .entry(m.path.clone())
                    .or_default()
                    .push(format!("imports {} which changed", changed));
                upstream_breaking
                    .entry(m.path.clone())
                    .or_default()
                    .extend(breaking_here.iter().cloned());
            }
        }
    }

    // Prior generation, if any: re-open = revision.
    let existing = load_event(store, id).ok();
    let generation = existing.as_ref().map_or(1, |e| e.generation + 1);
    let prior: HashMap<&str, &ParityTarget> = existing
        .as_ref()
        .map(|e| e.targets.iter().map(|t| (t.path.as_str(), t)).collect())
        .unwrap_or_default();

    // Union of changed ∪ importers, deterministically ordered.
    let mut all: Vec<String> = reasons.keys().chain(importers.keys()).cloned().collect();
    all.sort();
    all.dedup();

    if all.is_empty() && existing.is_none() {
        anyhow::bail!(
            "no structural change against `{baseline_ref}` — nothing to open \
                 (commit code first, then run parity against an older baseline)"
        );
    }
    // Revision with no further change: everything auto-verifies clean.

    let mut targets = Vec::new();
    for path in &all {
        let changed_here: Vec<String> = reasons.get(path).cloned().into_iter().flatten().collect();
        let importer_here: Vec<String> = importers.get(path).cloned().unwrap_or_default();
        let mut breaking_up: Vec<String> = upstream_breaking.get(path).cloned().unwrap_or_default();
        breaking_up.sort();
        breaking_up.dedup();
        let prior_target = prior.get(path.as_str()).copied();
        // Who needs a human (0.3.35): origins and additive importers do not;
        // importers of a breaking change do (parity_policy).
        let placed = policy::place(&changed_here, &breaking_up, prior_target, generation);
        let reason = if !changed_here.is_empty() {
            changed_here.join("; ")
        } else if placed.breaking {
            policy::breaking_reason(&breaking_up)
        } else {
            importer_here.join("; ")
        };
        let keeps_provenance = matches!(placed.status, "ack" | "noop");
        let by = prior_target
            .filter(|_| keeps_provenance)
            .map(|p| p.by.clone())
            .unwrap_or_default();
        let when = prior_target
            .filter(|_| keeps_provenance)
            .map(|p| p.when.clone())
            .unwrap_or_default();
        // The audit trail survives the re-open: a hand verdict the new
        // generation does not keep becomes history, not silence.
        let history = prior_target
            .map(|p| {
                let was = history::Amendment {
                    status: p.status.clone(),
                    note: p.note.clone(),
                    by: p.by.clone(),
                    when: p.when.clone(),
                };
                history::carry(&p.history, was, placed.status)
            })
            .unwrap_or_default();
        targets.push(ParityTarget {
            path: path.clone(),
            reason,
            status: placed.status.into(),
            note: placed.note,
            by,
            when,
            breaking: placed.breaking,
            history,
        });
    }

    let today = Utc::now().date_naive().to_string();
    let event = ParityEvent {
        id: id.into(),
        kind: if kind.is_empty() {
            "digest-diff".into()
        } else {
            kind.into()
        },
        source: if source.is_empty() {
            format!(
                "digest diff vs `{baseline_ref}` ({} moved items)",
                d.modules_added.len()
                    + d.modules_removed.len()
                    + d.signatures_added.len()
                    + d.signatures_removed.len()
                    + d.deps_added.len()
                    + d.deps_removed.len()
            )
        } else {
            source.into()
        },
        generation,
        baseline_ref: baseline_ref.into(),
        created: existing.map(|e| e.created).unwrap_or_else(|| today.clone()),
        updated: today,
        targets,
    };
    save_event(store, &event)?;
    Ok(event)
}

fn push_reason(map: &mut HashMap<String, Vec<String>>, path: &str, why: &str) {
    let key = path.to_string();
    let entry = map.entry(key).or_default();
    if !entry.iter().any(|r| r == why) {
        entry.push(why.to_string());
    }
}

/// Heuristic: does internal dep id `dep` of module `importer_stem` point at
/// module `module_path`? Delegates to the shared graph semantics in
/// `digest::dep_points_at` (single resolver for fan-out, layer boundaries,
/// and dependents queries).
pub fn dep_matches(
    importer_stem: &str,
    dep: &str,
    module_path: &str,
    crate_names: &[String],
) -> bool {
    digest::dep_points_at(importer_stem, dep, module_path, crate_names)
}

/// crates/<crate>/src/a/b.rs -> a/b ; src/a.rs -> a
pub fn crate_rel(module_path: &str) -> Option<String> {
    let rel = if let Some((_, tail)) = module_path.split_once("/src/") {
        tail.to_string()
    } else {
        module_path.strip_prefix("src/")?.to_string()
    };
    let rel = rel.trim_end_matches(".rs").to_string();
    let rel = rel.trim_end_matches("/mod").to_string();
    if rel.is_empty() {
        None
    } else {
        Some(rel)
    }
}

fn parity_dir(store: &Store) -> std::path::PathBuf {
    store
        .manifest_path()
        .parent()
        .context("store has no parent")
        .expect("store has parent")
        .join("parity")
}

/// An event id becomes a file name under `.harna/parity/`: it must stay a
/// plain stem (`harna parity open ../manifest` would otherwise overwrite
/// `.harna/manifest.toml`).
fn check_id(id: &str) -> anyhow::Result<()> {
    let plain = !id.is_empty()
        && !id.starts_with('.')
        && !id.contains("..")
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !plain {
        anyhow::bail!(
            "parity event id `{id}` must be a plain name: ASCII letters, digits, `-`, `_`, `.`"
        );
    }
    Ok(())
}

fn load_event(store: &Store, id: &str) -> anyhow::Result<ParityEvent> {
    check_id(id)?;
    let raw = std::fs::read_to_string(parity_dir(store).join(format!("{id}.toml")))
        .with_context(|| format!("reading parity event `{id}`"))?;
    toml::from_str(&raw).with_context(|| format!("parsing parity event `{id}`"))
}

fn save_event(store: &Store, event: &ParityEvent) -> anyhow::Result<()> {
    check_id(&event.id)?;
    let dir = parity_dir(store);
    std::fs::create_dir_all(&dir)?;
    let raw = toml::to_string_pretty(event)?;
    // Atomic: a truncated event file would trip the fail-closed gate.
    crate::schema::store::atomic::write(dir.join(format!("{}.toml", event.id)), raw)?;
    Ok(())
}

fn read_events(dir: &std::path::Path) -> anyhow::Result<Vec<ParityEvent>> {
    let mut events = Vec::new();
    if dir.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(dir)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
            .collect();
        entries.sort();
        for p in entries {
            let raw = std::fs::read_to_string(&p)?;
            let ev: ParityEvent =
                toml::from_str(&raw).with_context(|| format!("parsing {:?}", p))?;
            events.push(ev);
        }
    }
    Ok(events)
}

/// All events, most recently updated first.
pub fn list_events(store: &Store) -> anyhow::Result<Vec<ParityEvent>> {
    let mut events = read_events(&parity_dir(store))?;
    events.sort_by(|a, b| {
        b.updated
            .cmp(&a.updated)
            .then(b.generation.cmp(&a.generation))
    });
    Ok(events)
}

/// One event by id, straight off the live ledger.
pub fn event(store: &Store, id: &str) -> anyhow::Result<ParityEvent> {
    load_event(store, id)
}

/// The events FILED in the archive (0.3.22): the release notes read the
/// filed ledger without moving anything live; same bytes, other shelf.
pub fn archived_events(store: &Store) -> anyhow::Result<Vec<ParityEvent>> {
    let dir = store.archive_dir().join("parity");
    let mut events = read_events(&dir)?;
    events.sort_by(|a, b| b.updated.cmp(&a.updated));
    Ok(events)
}

/// Archive the closed events (0.3.18): every event with no open targets
/// moves to `.harna/archive/parity/` — the ledger's follow-through debt is
/// what gates validation, and a closed event owes nothing. Returns the
/// archived ids. Open events stay live no matter how old: an open target
/// is a promise, not history.
pub fn archive_closed(store: &Store) -> anyhow::Result<Vec<String>> {
    let mut archived = Vec::new();
    for ev in list_events(store)? {
        if ev.open_targets().is_empty() {
            let path = parity_dir(store).join(format!("{}.toml", ev.id));
            store.archive_file(&path, "parity")?;
            archived.push(ev.id);
        }
    }
    Ok(archived)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod fixtures;
