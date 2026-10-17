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

use std::collections::{HashMap, HashSet};
use std::path::Path;
use anyhow::Context as _;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::schema::store::Store;
use crate::search::digest::{self, Digest};
use crate::search::diff::{self, DigestDiff};

/// The statuses a parity target can carry live in the data file
/// `statuses.toml` (constants-are-data); this module imports the loader.
use crate::enforcement::statuses;
use crate::enforcement::violation::{Severity, Violation};

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
}

impl ParityEvent {
    pub fn open_targets(&self) -> Vec<&ParityTarget> {
        self.targets.iter().filter(|t| t.status == "open").collect()
    }

    pub fn target(&self, path: &str) -> Option<&ParityTarget> {
        self.targets.iter().find(|t| t.path == path)
    }

    /// Debt view: only the still-unresolved targets with their reasons.
    pub fn render_open(&self) -> String {
        if self.open_targets().is_empty() {
            return format!("parity `{}`: nothing open\n", self.id);
        }
        let open = self.open_targets().len();
        let mut out = format!(
            "[gen{}] {} ({}): {} open — {}, baseline `{}`\n",
            self.generation,
            self.id,
            self.kind,
            open,
            self.source,
            self.baseline_ref
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
    open_from_digests(store, id, kind, source, baseline_ref, &baseline, &fresh, &d, &crate_names)}

/// Fan-out computation over two already-built digests — the testable core.
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
        push_reason(&mut reasons, path, &format!("signature removed: {sig}"));
    }
    for (path, dep) in &d.deps_added {
        push_reason(&mut reasons, path, &format!("dep added: {dep}"));
    }
    for (path, dep) in &d.deps_removed {
        push_reason(&mut reasons, path, &format!("dep removed: {dep}"));
    }

    // Ring 2: importers of every changed module (from whichever digest still
    // knows them — a removed module's importers only exist in the baseline).
    let mut importers: HashMap<String, Vec<String>> = HashMap::new();
    for changed in reasons.keys() {
        let digest_for = if fresh.modules.iter().any(|m| &m.path == changed) {
            fresh
        } else {
            baseline
        };
        for m in &digest_for.modules {
            if &m.path == changed {
                continue;
            }
            let importer_stem = digest::crate_of(&m.path).1;
            if m.deps
                .iter()
                .any(|dep| dep_matches(&importer_stem, dep, changed, crate_names))
            {
                importers
                    .entry(m.path.clone())
                    .or_default()
                    .push(format!("imports {} which changed", changed));
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

    if all.is_empty() {
        if existing.is_none() {
            anyhow::bail!(
                "no structural change against `{baseline_ref}` — nothing to open \
                 (commit code first, then run parity against an older baseline)"
            );
        }
        // Revision with no further change: everything auto-verifies clean.
    }

    let mut targets = Vec::new();
    for path in &all {
        let changed_here: Vec<String> = reasons
            .get(path)
            .cloned()
            .into_iter()
            .flatten()
            .collect();
        let importer_here: Vec<String> = importers.get(path).cloned().unwrap_or_default();
        let reason = if !changed_here.is_empty() {
            changed_here.join("; ")
        } else {
            importer_here.join("; ")
        };
        let prior_target = prior.get(path.as_str());
        let (status, note) = match (changed_here.is_empty(), prior_target) {
            // Still structurally changed: fresh debt, old resolutions invalidated.
            (false, Some(prev)) if prev.status != "open" => (
                "open",
                format!("re-opened at gen {generation} (was {})", prev.status),
            ),
            (false, _) => ("open", String::new()),
            // Importer-only with prior history: re-verify. Acknowledgments die
            // with their generation; an unchanged surface verifies as `auto`.
            (true, Some(prev)) if prev.status == "open" => ("open", prev.note.clone()),
            (true, Some(prev)) => (
                "auto",
                if prev.note.is_empty() {
                    format!("re-opened at gen {generation} (was {})", prev.status)
                } else {
                    format!("{}; re-opened at gen {generation} (was {})", prev.note, prev.status)
                },
            ),
            (true, None) => ("open", String::new()),
        };
        let by = prior_target.map(|p| p.by.clone()).unwrap_or_default();
        let when = prior_target.map(|p| p.when.clone()).unwrap_or_default();
        targets.push(ParityTarget {
            path: path.clone(),
            reason,
            status: status.into(),
            note,
            by,
            when,
        });
    }

    let today = Utc::now().date_naive().to_string();
    let event = ParityEvent {
        id: id.into(),
        kind: if kind.is_empty() { "digest-diff".into() } else { kind.into() },
        source: if source.is_empty() {
            format!("digest diff vs `{baseline_ref}` ({} moved items)", d.modules_added.len() + d.modules_removed.len() + d.signatures_added.len() + d.signatures_removed.len() + d.deps_added.len() + d.deps_removed.len())
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

/// Auto-verification pass: re-check every OPEN target of an event against
/// the event's baseline digest and the current tree, without opening a new
/// generation. A target whose module still exists and whose shipped surface
/// (signatures + internal deps) is unchanged vs the baseline verifies `auto`
/// — the propagation left no trace there and the re-ack tax is waived.
/// Changed or deleted modules stay open with their state recorded.
///
/// Auto never adds targets: it is a verification pass, not a fan-out.
pub fn auto(
    root: &Path,
    store: &Store,
    id: &str,
    max_files: usize,
) -> anyhow::Result<(ParityEvent, usize, usize)> {
    let event = load_event(store, id).with_context(|| format!("parity event `{id}` not found"))?;
    let baseline = diff::digest_at_ref(root, &event.baseline_ref)?;
    let fresh = digest::build(root, max_files)?;
    auto_from(store, &event, &baseline, &fresh)
}

/// Testable core of the auto pass.
pub fn auto_from(
    store: &Store,
    event: &ParityEvent,
    baseline: &Digest,
    fresh: &Digest,
) -> anyhow::Result<(ParityEvent, usize, usize)> {
    let base_modules: HashMap<&str, &crate::search::digest::Module> =
        baseline.modules.iter().map(|m| (m.path.as_str(), m)).collect();
    let fresh_modules: HashMap<&str, &crate::search::digest::Module> =
        fresh.modules.iter().map(|m| (m.path.as_str(), m)).collect();

    let mut verified = 0usize;
    let mut open = 0usize;
    let mut updated = event.clone();
    updated.updated = Utc::now().date_naive().to_string();
    for t in &mut updated.targets {
        if t.status != "open" {
            continue;
        }
        match (base_modules.get(t.path.as_str()), fresh_modules.get(t.path.as_str())) {
            // Present in both and surface-identical: nothing propagated here.
            (Some(base), Some(fresh)) if surface_equal(base, fresh) => {
                t.status = "auto".into();
                t.note = format!(
                    "auto-verified surface unchanged vs baseline `{}` (harna parity auto)",
                    event.baseline_ref
                );
                t.by = String::new();
                t.when = Utc::now().to_string();
                verified += 1;
            }
            // Added by this change (no baseline): verification is meaningless.
            (None, Some(_)) => {
                open += 1;
            }
            // Deleted from the tree: the debt is real, keep it visible.
            (Some(_), None) => {
                if !t.reason.contains("module deleted from current tree") {
                    t.reason.push_str("; module deleted from current tree");
                }
                open += 1;
            }
            // Surface moved since the baseline: genuine debt, stays open.
            _ => {
                open += 1;
            }
        }
    }
    save_event(store, &updated)?;
    Ok((updated, verified, open))
}

fn surface_equal(a: &crate::search::digest::Module, b: &crate::search::digest::Module) -> bool {
    let sa: HashSet<&str> = a.signatures.iter().map(|s| s.as_str()).collect();
    let sb: HashSet<&str> = b.signatures.iter().map(|s| s.as_str()).collect();
    let da: HashSet<&str> = a.deps.iter().map(|s| s.as_str()).collect();
    let db: HashSet<&str> = b.deps.iter().map(|s| s.as_str()).collect();
    sa == sb && da == db
}

/// Sweep auto-verification across ALL events, not just one.
///
/// `harna parity auto` with no id sweeps every event; each event runs its own
/// re-derivation against its own baseline (no new generations). One fresh
/// scan backs the whole sweep.
pub fn auto_all(
    root: &Path,
    store: &Store,
    max_files: usize,
) -> anyhow::Result<Vec<String>> {
    let fresh = digest::build(root, max_files)?;
    auto_sweep(store, |ev| diff::digest_at_ref(root, &ev.baseline_ref), &fresh)
}

/// Testable sweep: baselines come from the injected fetcher (git only at
/// the CLI/MCP boundary).
pub fn auto_sweep(
    store: &Store,
    baseline_of: impl Fn(&ParityEvent) -> anyhow::Result<Digest>,
    fresh: &Digest,
) -> anyhow::Result<Vec<String>> {
    let mut lines = Vec::new();
    for ev in list_events(store)? {
        if ev.open_targets().is_empty() {
            lines.push(format!("parity `{}`: no open targets", ev.id));
            continue;
        }
        let baseline = baseline_of(&ev)?;
        let (updated, verified, open) = auto_from(store, &ev, &baseline, fresh)?;
        lines.push(format!(
            "parity `{}`: {} auto-verified, {} still open",
            updated.id, verified, open
        ));
    }
    if lines.is_empty() {
        lines.push("no parity events".into());
    }
    Ok(lines)
}

/// Resolve several targets of one event atomically (single load/save).
///
/// `entries` is (path, status, note); missing targets are reported back, and
/// everything that did match is applied — the event is saved once.
pub fn resolve_bulk(
    store: &Store,
    id: &str,
    entries: &[(String, String, String)],
    actor: &str,
) -> anyhow::Result<(ParityEvent, Vec<String>)> {
    if entries.is_empty() {
        anyhow::bail!("no targets given");
    }
    for (_, status, _) in entries {
        let allowed = statuses::all();
        if !allowed.iter().any(|s| s == status) {
            anyhow::bail!("status must be one of: {}", allowed.join(", "));
        }
    }
    let mut event =
        load_event(store, id).with_context(|| format!("parity event `{id}` not found"))?;
    let now = Utc::now().to_string();
    let mut missing: Vec<String> = Vec::new();
    for (path, status, note) in entries {
        match event.targets.iter_mut().find(|t| &t.path == path) {
            Some(t) => {
                t.status = status.clone();
                t.note = note.clone();
                t.by = actor.into();
                t.when = now.clone();
            }
            None => missing.push(path.clone()),
        }
    }
    event.updated = Utc::now().date_naive().to_string();
    save_event(store, &event)?;
    Ok((event, missing))
}

/// Resolve one target of one event.
pub fn resolve(
    store: &Store,
    id: &str,
    path: &str,
    status: &str,
    note: &str,
    actor: &str,
) -> anyhow::Result<ParityEvent> {
    let allowed = statuses::all();
    if !allowed.iter().any(|s| s == status) {
        anyhow::bail!("status must be one of: {}", allowed.join(", "));
    }
    let mut event = load_event(store, id)
        .with_context(|| format!("parity event `{id}` not found"))?;
    let target = event
        .targets
        .iter_mut()
        .find(|t| t.path == path)
        .with_context(|| format!("event `{id}` has no target `{path}` — re-open to refresh the fan-out"))?;
    target.status = status.into();
    target.note = note.into();
    target.by = actor.into();
    target.when = Utc::now().to_string();
    event.updated = Utc::now().date_naive().to_string();
    save_event(store, &event)?;
    Ok(event)
}

/// Render all events (or one), most recently updated first.
/// With `open_only`, skip events with no open targets and render them
/// as debt views.
pub fn show(store: &Store, only: Option<&str>, open_only: bool) -> anyhow::Result<String> {
    let mut events = list_events(store)?;
    if let Some(only) = only {
        events.retain(|e| e.id == only);
        if events.is_empty() {
            anyhow::bail!("parity event `{only}` not found");
        }
    }
    if open_only {
        events.retain(|e| e.open_targets().len() > 0);
    }
    if events.is_empty() {
        return Ok(if open_only {
            "no open parity debts".into()
        } else {
            "no parity events — nothing owed".into()
        });
    }
    Ok(events
        .iter()
        .map(|e| if open_only { e.render_open() } else { e.render() })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Total number of unresolved (open) targets across all events.
pub fn open_debt_count(store: &Store) -> anyhow::Result<usize> {
    Ok(list_events(store)?.iter().map(|e| e.open_targets().len()).sum())
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

/// The declared-shape orientation (0.3.22, check `fanout-shape`, Info):
/// an event whose fan-out exceeds a template's ceiling over its scope is
/// not an error — it is a shape somebody declared for exactly this kind
/// of change, and the count walked past it. Names the template, both
/// numbers and the declared reason; severity rides the registry (a bound
/// rule may promote it, like every check).
pub fn check_shape(store: &Store, out: &mut Vec<crate::enforcement::violation::Violation>) {
    let templates = match store.load_templates() {
        Ok(t) => t,
        // Unreadable templates document: skip (the registry's default is
        // advisory anyway; a broken file must not invent findings).
        Err(_) => return,
    };
    if templates.is_empty() {
        return;
    }
    let events = match list_events(store) {
        Ok(e) => e,
        Err(_) => return,
    };
    for ev in events {
        for t in &templates.templates {
            // The shaped fan is the module count INSIDE the template's
            // scope — a template declares one change type's shape, not the
            // whole event's reach. Empty scope = every target.
            let in_scope = ev
                .targets
                .iter()
                .filter(|target| {
                    t.scope.is_empty()
                        || target.path.starts_with(&format!("{}/", t.scope))
                        || target.path == t.scope
                })
                .count();
            if in_scope == 0 || in_scope <= t.max_targets {
                continue;
            }
            out.push(Violation {
                check: "fanout-shape".into(),
                file: format!(".harna/parity/{}.toml", ev.id),
                line: 0,
                severity: Severity::Info,
                message: format!(
                    "event `{}` carries {in_scope} target(s) over `{scope}`; template `{}` declares at most {max}",
                    ev.id,
                    t.id,
                    scope = if t.scope.is_empty() { "*" } else { &t.scope },
                    max = t.max_targets,
                ),
                hint: "either the change type exploded (ask why), or the template's \
                       ceiling is stale (harna templates add replaces it)"
                    .into(),
            });
        }
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

fn load_event(store: &Store, id: &str) -> anyhow::Result<ParityEvent> {
    let raw = std::fs::read_to_string(parity_dir(store).join(format!("{id}.toml")))
        .with_context(|| format!("reading parity event `{id}`"))?;
    toml::from_str(&raw).with_context(|| format!("parsing parity event `{id}`"))
}

fn save_event(store: &Store, event: &ParityEvent) -> anyhow::Result<()> {
    let dir = parity_dir(store);
    std::fs::create_dir_all(&dir)?;
    let raw = toml::to_string_pretty(event)?;
    std::fs::write(dir.join(format!("{}.toml", event.id)), raw)?;
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
    events.sort_by(|a, b| b.updated.cmp(&a.updated).then(b.generation.cmp(&a.generation)));
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
mod tests {
    use super::*;
    use crate::schema::store::Store;
    use crate::search::digest::Module;

    /// Workspace crate names for the fixture digests (crate `x` is a member
    /// so cross-crate imports like `x::core::store::load` resolve).
    fn workspace() -> Vec<String> {
        vec!["x".to_string()]
    }

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

    fn digests() -> (Digest, Digest) {
        let baseline = Digest {
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

    #[test]
    fn fan_out_is_coupling_not_distance() {
        let tmp = std::env::temp_dir().join(format!("harna-parity-fanout-{}", std::process::id()));
        // Windows reuses process ids: a stale dir from a previous run would
        // carry old parity events in.
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        let (baseline, fresh) = digests();
        let d = diff::diff(&baseline, &fresh);
        let event = open_from_digests(
            &store,
            "store-change",
            "digest-diff",
            "store: load -> load_v2",
            "test-ref",
            &baseline,
            &fresh,
            &d,
            &workspace(),
        )
        .unwrap();

        // changed: store.rs (sig) + added.rs (new module)
        // importer: crates/y/src/api.rs imports workspace crate x's store
        // NOT a target: logger.rs — physically adjacent, structurally untouched.
        let paths: Vec<&str> = event.targets.iter().map(|t| t.path.as_str()).collect();
        assert!(paths.contains(&"crates/x/src/core/store.rs"));
        assert!(paths.contains(&"crates/x/src/core/added.rs"));
        assert!(paths.contains(&"crates/y/src/api.rs"), "importer enters ring 2");
        assert!(!paths.contains(&"crates/x/src/core/logger.rs"), "no geometric spread");
    }

    #[test]
    fn revision_invalidates_acks_and_auto_verifies_unchanged() {
        let tmp = std::env::temp_dir().join(format!("harna-parity-rev-{}", std::process::id()));
        // Windows reuses process ids: a stale dir from a previous run would
        // carry old parity events in.
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        let (baseline, fresh) = digests();
        let d = diff::diff(&baseline, &fresh);
        let mut event = open_from_digests(
            &store,
            "rev-test",
            "digest-diff",
            "",
            "gen1",
            &baseline,
            &fresh,
            &d,
            &workspace(),
        )
        .unwrap();
        assert_eq!(event.generation, 1);
        // Human/agent resolves everything as ack.
        for t in event.targets.clone() {
            event = resolve(&store, "rev-test", &t.path, "ack", "did it", "user").unwrap();
        }
        assert!(event.open_targets().is_empty());

        // Revision 2: same diff again (load_v2 revised). load_v2 sig present in
        // a fresh scan built later; simulate by re-opening: all acks invalidate,
        // the unchanged importer module auto-verifies... it did not change, but
        // it imports a changed module, so it stays in debt — no, revision rules:
        // importer whose OWN surface is unchanged becomes `auto`.
        let event2 = open_from_digests(
            &store,
            "rev-test",
            "digest-diff",
            "",
            "gen2",
            &baseline,
            &fresh,
            &d,
            &workspace(),
        )
        .unwrap();
        assert_eq!(event2.generation, 2);
        let store_target = event2.targets.iter().find(|t| t.path.ends_with("store.rs")).unwrap();
        let importer = event2.targets.iter().find(|t| t.path.ends_with("api.rs")).unwrap();
        assert_eq!(store_target.status, "open", "changed module freshly owed");
        assert_eq!(importer.status, "auto", "unchanged importer auto-verified");
        assert!(importer.note.contains("re-opened at gen 2 (was ack)"));    }

    #[test]
    fn resolve_validates_status_and_target() {
        let tmp = std::env::temp_dir().join(format!("harna-parity-res-{}", std::process::id()));
        // Windows reuses process ids: a stale dir from a previous run would
        // carry old parity events in.
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        let (baseline, fresh) = digests();
        let d = diff::diff(&baseline, &fresh);
        open_from_digests(&store, "r", "digest-diff", "", "x", &baseline, &fresh, &d, &workspace()).unwrap();
        assert!(resolve(&store, "r", "nonexistent", "ack", "", "a").is_err());
        assert!(resolve(&store, "r", "crates/x/src/core/store.rs", "bogus", "", "a").is_err());
        let ev = resolve(&store, "r", "crates/x/src/core/store.rs", "noop", "already routed", "agent").unwrap();
        let t = ev.targets.iter().find(|t| t.path == "crates/x/src/core/store.rs").unwrap();
        assert_eq!(t.status, "noop");
    }

    #[test]
    fn archive_files_closed_events_and_keeps_open_promises() {
        let tmp = std::env::temp_dir().join(format!("harna-parity-arch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        let (baseline, fresh) = digests();
        let d = diff::diff(&baseline, &fresh);
        // `closed`: every target resolved. `owed`: stays open.
        let mut closed = open_from_digests(&store, "closed", "digest-diff", "", "x", &baseline, &fresh, &d, &workspace()).unwrap();
        for t in closed.targets.clone() {
            closed = resolve(&store, "closed", &t.path, "noop", "nothing needed", "test").unwrap();
        }
        open_from_digests(&store, "owed", "digest-diff", "", "x", &baseline, &fresh, &d, &workspace()).unwrap();

        let archived = archive_closed(&store).unwrap();
        assert_eq!(archived, vec!["closed".to_string()]);
        assert!(
            store.archive_dir().join("parity/closed.toml").is_file(),
            "the filed event keeps its bytes, out of the live surface"
        );
        let live = list_events(&store).unwrap();
        assert_eq!(live.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["owed"]);

        // Idempotent: a second pass finds nothing (open stays open).
        assert!(archive_closed(&store).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn auto_pass_clears_untouched_importers_keeps_real_debt() {
        let tmp = std::env::temp_dir().join(format!("harna-parity-auto-{}", std::process::id()));
        // Windows reuses process ids: a stale dir from a previous run would
        // carry old parity events in.
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        let (baseline, fresh) = digests();
        let d = diff::diff(&baseline, &fresh);
        let event = open_from_digests(
            &store,
            "auto-test",
            "digest-diff",
            "",
            "HEAD",
            &baseline,
            &fresh,
            &d,
            &workspace(),
        )
        .unwrap();
        // gen1 fresh event: changed modules + importer all open.
        assert!(event.target("crates/y/src/api.rs").map_or(false, |t| t.status == "open"));

        // Auto pass with unchanged tree: the importer's surface is identical
        // to baseline -> auto; the changed modules keep their debt.
        let (ev, verified, open) = auto_from(&store, &event, &baseline, &fresh).unwrap();
        assert_eq!(verified, 1, "importer auto-verified");
        assert_eq!(open, 2, "changed modules stay owed");
        let importer = ev.target("crates/y/src/api.rs").unwrap();
        assert_eq!(importer.status, "auto");
        assert!(importer.note.contains("auto-verified surface unchanged"));

        // Auto never opens new targets and never bumps the generation.
        assert_eq!(ev.generation, event.generation);
        assert_eq!(ev.targets.len(), event.targets.len());
    }

    #[test]
    fn auto_flags_deleted_target_modules() {
        let tmp = std::env::temp_dir().join(format!("harna-parity-del-{}", std::process::id()));
        // Windows reuses process ids: a stale dir from a previous run would
        // carry old parity events in.
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        let (baseline, fresh) = digests();
        let d = diff::diff(&baseline, &fresh);
        let event = open_from_digests(
            &store,
            "del-test",
            "digest-diff",
            "",
            "HEAD",
            &baseline,
            &fresh,
            &d,
            &workspace(),
        )
        .unwrap();

        // A tree where the importer module is deleted entirely.
        let aftermath = Digest {
            generated_at: "later".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: fresh.modules.iter().filter(|m| !m.path.ends_with("api.rs")).cloned().collect(),
        };
        let (ev, _v, open) = auto_from(&store, &event, &baseline, &aftermath).unwrap();
        let importer = ev
            .target("crates/y/src/api.rs")
            .expect("target kept visible");
        assert_eq!(importer.status, "open", "deletion debt is never auto-cleared");
        assert!(importer.reason.contains("module deleted from current tree"));
        assert!(open >= 1);
    }

    #[test]
    fn auto_pass_sweeps_every_event() {
        let tmp = std::env::temp_dir().join(format!("harna-parity-sweep-{}", std::process::id()));
        // Windows reuses process ids: a stale dir from a previous run would
        // carry old parity events in.
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        let (baseline, fresh) = digests();
        let d = diff::diff(&baseline, &fresh);
        open_from_digests(
            &store,
            "sweep-a",
            "digest-diff",
            "",
            "HEAD",
            &baseline,
            &fresh,
            &d,
            &workspace(),
        )
        .unwrap();
        open_from_digests(
            &store,
            "sweep-b",
            "digest-diff",
            "",
            "HEAD",
            &baseline,
            &fresh,
            &d,
            &workspace(),
        )
        .unwrap();

        let lines = auto_sweep(&store, |_| Ok(baseline.clone()), &fresh).unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| l.contains("auto-verified, 2 still open")));
        // Sweeping leaves both events with their changed modules still owed.
        for id in ["sweep-a", "sweep-b"] {
            let ev = crate::enforcement::parity::list_events(&store)
                .unwrap()
                .into_iter()
                .find(|e| e.id == id)
                .unwrap();
            assert_eq!(ev.open_targets().len(), 2);
        }
    }

    #[test]
    fn bulk_resolve_applies_and_reports_missing() {
        let tmp = std::env::temp_dir().join(format!("harna-parity-bulk-{}", std::process::id()));
        // Windows reuses process ids: a stale dir from a previous run would
        // carry old parity events in.
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        let (baseline, fresh) = digests();
        let d = diff::diff(&baseline, &fresh);
        let _event = open_from_digests(
            &store,
            "bulk-test",
            "digest-diff",
            "",
            "HEAD",
            &baseline,
            &fresh,
            &d,
            &workspace(),
        )
        .unwrap();

        let entries = vec![
            ("crates/x/src/core/store.rs".into(), "ack".into(), "routed through transiter".into()),
            ("crates/x/src/core/added.rs".into(), "noop".into(), "nothing needed".into()),
            ("crates/ghost.rs".into(), "noop".into(), String::new()),
        ];
        let (ev, missing) =
            resolve_bulk(&store, "bulk-test", &entries, "bulk test").unwrap();
        assert_eq!(missing, vec!["crates/ghost.rs".to_string()]);
        assert_eq!(ev.targets.iter().find(|t| t.path == "crates/x/src/core/store.rs").unwrap().status, "ack");
        assert_eq!(ev.targets.iter().find(|t| t.path == "crates/x/src/core/added.rs").unwrap().status, "noop");
        assert_eq!(ev.open_targets().len(), 1, "only the importer stays open");
        let rendered = ev.render();
        assert!(rendered.contains("resolved by bulk test"));
    }

    #[test]
    fn debt_view_hides_resolved_and_auto() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!("harna-parity-openview-{}", std::process::id()));
        // Windows reuses process ids: a stale dir from a previous run would
        // carry old parity events in.
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        let (baseline, fresh) = digests();
        let d = diff::diff(&baseline, &fresh);
        open_from_digests(
            &store,
            "debtview",
            "digest-diff",
            "",
            "HEAD",
            &baseline,
            &fresh,
            &d,
            &workspace(),
        )
        .unwrap();

        // The untouched importer auto-verifies; the changed store module is
        // resolved as noop; the added module keeps its debt.
        let (ev, _verified, _open) = {
            let baseline2 = baseline.clone();
            let fresh2 = fresh.clone();
            // re-derive via auto_from against the event on disk
            let event = list_events(&store)?.into_iter().find(|e| e.id == "debtview").unwrap();
            auto_from(&store, &event, &baseline2, &fresh2)?
        };
        assert_eq!(ev.open_targets().len(), 2);
        resolve(&store, "debtview", "crates/x/src/core/store.rs", "noop", "n/a", "t").unwrap();

        let text = show(&store, Some("debtview"), true).unwrap();
        assert!(text.contains("crates/x/src/core/added.rs"));
        assert!(!text.contains("auto"));
        assert!(!text.contains("noop"), "debt view lists only open targets");
        // Full view still shows provenance.
        let full = show(&store, Some("debtview"), false).unwrap();
        assert!(full.contains("auto"));
        assert!(full.contains("noop"));
        Ok(())
    }

    #[test]
    fn crate_rel_resolves_workspace_paths() {
        assert_eq!(
            crate_rel("crates/harna-core/src/schema/store.rs").as_deref(),
            Some("schema/store")
        );
        assert_eq!(crate_rel("src/main.rs").as_deref(), Some("main"));
        assert_eq!(
            crate_rel("crates/x/src/engine/billing/mod.rs").as_deref(),
            Some("engine/billing")
        );
    }

    #[test]
    fn super_and_self_edges_resolve_to_importer_context() {
        use crate::search::digest::{resolve_dep, DepTarget};

        // super::x from module a/b reaches a/x
        assert_eq!(
            resolve_dep("super::x", "a/b", &[]),
            Some(DepTarget { external: None, stem: "a/x".into() })
        );
        // bare super:: from a/b is the parent module itself
        assert_eq!(
            resolve_dep("super::", "a/b", &[]),
            Some(DepTarget { external: None, stem: "a".into() })
        );
        // self::x from a/b stays inside a/b
        assert_eq!(
            resolve_dep("self::x", "a/b", &[]),
            Some(DepTarget { external: None, stem: "a/b/x".into() })
        );
        // crate-root files cannot use super::
        assert_eq!(resolve_dep("super::x", "main", &[]), None);

        // And the fan-out matcher honors it: importer a/b.rs depends on
        // super::x; module a/x.rs is a target.
        assert!(dep_matches(
            "a/b",
            "super::x",
            "crates/t/src/a/x.rs",
            &[]
        ));
        assert!(!dep_matches(
            "a/b",
            "super::x",
            "crates/t/src/other/x.rs",
            &[]
        ));
    }

    #[test]
    fn shape_orientation_names_template_and_ceiling() {
        use crate::schema::parity_data::{ParityTemplate, ParityTemplates};

        let tmp = std::env::temp_dir().join(format!(
            "harna-parity-shape-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();

        let event = ParityEvent {
            id: "big-shape".into(),
            kind: "digest-diff".into(),
            source: "fixture".into(),
            generation: 1,
            baseline_ref: "HEAD".into(),
            created: "t".into(),
            updated: "t".into(),
            targets: vec![
                ParityTarget { path: "crates/x/src/schema/store.rs".into(), reason: "changed".into(), status: "open".into(), note: String::new(), by: "t".into(), when: String::new() },
                ParityTarget { path: "crates/x/src/schema/add.rs".into(), reason: "added".into(), status: "open".into(), note: String::new(), by: "t".into(), when: String::new() },
                ParityTarget { path: "crates/x/src/schema/drop.rs".into(), reason: "added".into(), status: "open".into(), note: String::new(), by: "t".into(), when: String::new() },
                ParityTarget { path: "crates/x/src/schema/fix.rs".into(), reason: "added".into(), status: "open".into(), note: String::new(), by: "t".into(), when: String::new() },
                ParityTarget { path: "crates/x/src/core/other.rs".into(), reason: "changed".into(), status: "open".into(), note: String::new(), by: "t".into(), when: String::new() },
            ],
        };
        save_event(&store, &event).unwrap();

        // Only the schema-scope template matches any of the event's targets.
        store
            .save_templates(&ParityTemplates {
                schema_version: crate::SCHEMA_VERSION,
                templates: vec![
                    ParityTemplate { id: "schema-change".into(), scope: "crates/x/src/schema".into(), max_targets: 2, rationale: "shard edits stay small".into() },
                    ParityTemplate { id: "engine-change".into(), scope: "crates/x/src/engine".into(), max_targets: 1, rationale: "no match".into() },
                ],
            })
            .unwrap();
        let mut out = Vec::new();
        check_shape(&store, &mut out);
        assert_eq!(out.len(), 1, "one template's ceiling was walked past");
        let v = &out[0];
        assert_eq!(v.check, "fanout-shape");
        assert_eq!(v.severity, Severity::Info);
        assert!(v.message.contains("big-shape"));
        assert!(v.message.contains("schema-change"), "names the template");
        assert!(v.message.contains("4 target(s)") || v.message.contains("carries 4"), "the actual shape");
        assert!(v.message.contains("at most 2"), "the declared shape");

        // A raised ceiling (or deleted templates) means nothing to notice.
        store
            .save_templates(&ParityTemplates {
                schema_version: crate::SCHEMA_VERSION,
                templates: vec![ParityTemplate {
                    id: "schema-change".into(),
                    scope: "crates/x/src/schema".into(),
                    max_targets: 50,
                    rationale: "raised".into(),
                }],
            })
            .unwrap();
        let mut out = Vec::new();
        check_shape(&store, &mut out);
        assert!(out.is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
