//! Reading the ledger: the rendered views, the open-debt count and the
//! declared-shape orientation validate prints.

use super::*;

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
        .join("\n")
        // How much of the hand-resolved ledger looks stamped (0.3.35).
        + &if open_only {
            String::new()
        } else {
            format!("\n{}", crate::enforcement::parity::policy::stamp_report(&events).render())
        })
}

/// Total number of unresolved (open) targets across all events.
pub fn open_debt_count(store: &Store) -> anyhow::Result<usize> {
    Ok(list_events(store)?
        .iter()
        .map(|e| e.open_targets().len())
        .sum())
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
