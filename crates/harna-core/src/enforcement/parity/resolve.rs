//! Resolving targets: one at a time or in bulk, with the previous
//! verdict surfaced before it is overwritten.

use super::*;

use super::history::{push_capped, Amendment};

/// Set a verdict; a settled hand verdict (ack/noop) moves to history first.
fn amend(t: &mut ParityTarget, status: &str, note: &str, actor: &str, now: &str) {
    let was = previous(t);
    if was.is_hand() {
        push_capped(&mut t.history, was);
    }
    t.status = status.into();
    t.note = note.into();
    t.by = actor.into();
    t.when = now.into();
}

/// A target's current verdict as an amendment record.
pub(crate) fn previous(t: &ParityTarget) -> Amendment {
    Amendment {
        status: t.status.clone(),
        note: t.note.clone(),
        by: t.by.clone(),
        when: t.when.clone(),
    }
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
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut missing: Vec<String> = Vec::new();
    for (path, status, note) in entries {
        match event.targets.iter_mut().find(|t| &t.path == path) {
            Some(t) => amend(t, status, note, actor, &now),
            None => missing.push(path.clone()),
        }
    }
    event.updated = Utc::now().date_naive().to_string();
    save_event(store, &event)?;
    Ok((event, missing))
}

/// Resolve one target of one event.
/// What a re-resolve would overwrite: the previous verdict on an
/// already-settled target, if any. Callers print it LOUDLY before
/// overwriting — resolution notes are audit trail, and silent loss is
/// how wrong counts go unnoticed (0.3.32 dogfood: 7/3 corrected to 6/4
/// with no trace of the original). Open targets have nothing to lose.
pub fn previous_resolution(event: &ParityEvent, path: &str) -> Option<String> {
    let t = event.target(path)?;
    if t.status == "open" {
        return None;
    }
    Some(format!(
        "target `{path}` was already {} by {} ({}) — kept as history, then overwritten",
        t.status, t.by, t.when
    ))
}

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
    let mut event =
        load_event(store, id).with_context(|| format!("parity event `{id}` not found"))?;
    let target = event
        .targets
        .iter_mut()
        .find(|t| t.path == path)
        .with_context(|| {
            format!("event `{id}` has no target `{path}` — re-open to refresh the fan-out")
        })?;
    amend(
        target,
        status,
        note,
        actor,
        &Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    );
    event.updated = Utc::now().date_naive().to_string();
    save_event(store, &event)?;
    Ok(event)
}

#[cfg(test)]
mod tests;
