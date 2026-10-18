//! The auto-verification pass: re-check open targets against their
//! baseline without opening a new generation.

use super::*;

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
        // An importer of a breaking change keeps its own surface by
        // definition — that is not evidence it stopped using what was
        // removed. Only a human resolves it.
        if t.breaking {
            if !fresh_modules.contains_key(t.path.as_str())
                && !t.reason.contains("module deleted from current tree")
            {
                t.reason.push_str("; module deleted from current tree");
            }
            open += 1;
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
