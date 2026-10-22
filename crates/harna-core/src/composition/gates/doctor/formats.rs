//! Doctor section (split 1.1.8): the format promise — every file reads
//! with this harna, words from a fixed set, unknown entries, the roadmap,
//! the ledgers, decisions and the handoff.

use super::{fail, ok, warn, CheckReport};
use crate::schema::store::Store;

/// Every file reads, is stamped as it needs and spells its fixed words.
pub(super) fn formats(store: &Store, checks: &mut Vec<CheckReport>) -> anyhow::Result<()> {
    // Every file reads with this harna, and is stamped as it needs (0.9.0:
    // lenient loads showed a newer file as empty)
    // manifest and rules have their own checks above: not twice
    let problems: Vec<String> = crate::schema::store::migrate::verify(store)
        .into_iter()
        // (their unknown keys are not: a typo there turned a rule advisory
        // silently, 0.9.2)
        .filter(|p| {
            !(p.starts_with("manifest.toml") || p.starts_with("rules.toml"))
                || p.contains("harna does not know")
        })
        .collect();
    if !problems.is_empty() {
        checks.push(fail(
            "readable",
            &format!(
                "{} file(s) do not read: {}",
                problems.len(),
                problems.join("; ")
            ),
            "fix the file, or upgrade harna when it was written by a newer one",
        ));
    }
    if let Ok(pending) = crate::schema::store::migrate::run(&store.repo_root(), store, true) {
        let due: Vec<&str> = pending
            .iter()
            .filter(|r| r.action == "would-migrate")
            .map(|r| r.file.as_str())
            .collect();
        if !due.is_empty() {
            checks.push(warn(
                "stamps",
                &format!("{} not stamped with the schema it needs", due.join(", ")),
                "run `harna migrate` (backups first; `--check` gates CI)",
            ));
        }
    }

    // Words from a fixed set (0.9.0): a hand-edited `runs = "CI"` or
    // `status = "retired"` read silently as the default before
    let mut odd: Vec<String> = Vec::new();
    if let Ok(r) = store.load_rules() {
        for rule in &r.rules {
            // (a severity outside error|info is rule-bindings' finding)
            if let Some(v) = rule
                .status
                .as_deref()
                .filter(|v| !matches!(*v, "active" | "deprecated"))
            {
                odd.push(format!(
                    "rules.toml {}: status `{v}` (active | deprecated)",
                    rule.id
                ));
            }
        }
    }
    if let Ok(c) = store.load_commands() {
        for cmd in &c.commands {
            if !matches!(cmd.runs.as_str(), "" | "local" | "ci" | "both") {
                odd.push(format!(
                    "commands.toml {}: runs `{}` (local | ci | both)",
                    cmd.name, cmd.runs
                ));
            }
        }
    }
    if let Ok(reg) = store.load_assumptions() {
        let states = crate::schema::registers::assumptions::STATES;
        for a in reg
            .assumptions
            .iter()
            .filter(|a| !states.contains(&a.state.as_str()))
        {
            odd.push(format!(
                "assumptions.toml {}: state `{}` ({}; read as open)",
                a.id,
                a.state,
                states.join(" | ")
            ));
        }
    }
    if let Ok(reg) = store.load_approvals() {
        let kinds = crate::schema::registers::approvals::KINDS;
        for r in reg
            .require
            .iter()
            .filter(|r| !kinds.contains(&r.kind.as_str()))
        {
            odd.push(format!(
                "approvals.toml require: kind `{}` ({}; checks nothing)",
                r.kind,
                kinds.join(" | ")
            ));
        }
    }
    let statuses = crate::enforcement::parity::statuses::all();
    for ev in crate::enforcement::parity::list_events(store).unwrap_or_default() {
        for t in &ev.targets {
            if !statuses.contains(&t.status) {
                odd.push(format!(
                    "parity/{}.toml {}: status `{}`",
                    ev.id, t.path, t.status
                ));
            }
        }
    }
    if !odd.is_empty() {
        checks.push(fail(
            "values",
            &format!(
                "{} value(s) outside their set: {}",
                odd.len(),
                odd.join("; ")
            ),
            "spell them from the set named; harna reads an unknown word as the default",
        ));
    }

    // Unknown entries (0.9.0): a file harna does not read is a typo or a
    // leftover — say so instead of reading nothing silently
    let unknown: Vec<String> = std::fs::read_dir(store.repo_root().join(".harna"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| !n.starts_with('.') && !crate::schema::store::KNOWN.contains(&n.as_str()))
        .collect();
    if !unknown.is_empty() {
        checks.push(warn(
            "format",
            &format!(
                "harna reads nothing in .harna/{}",
                unknown.join(", .harna/")
            ),
            "a typo of a known file (docs/FORMAT.md lists them), or a leftover to delete",
        ));
    }

    // Roadmap (0.3.28): stated data — parseable when declared; a
    // corrupt one would teach a future it does not actually have.
    match store.load_roadmap() {
        Ok(r) if r.is_empty() => checks.push(ok("roadmap", "none declared (optional)")),
        Ok(r) => {
            let problems = r.coherence();
            if problems.is_empty() {
                let now = r.current().map(|c| format!("now is `{}`", c.id));
                let summary = now.unwrap_or_else(|| {
                    format!("{} milestone(s) declared, none current", r.milestones.len())
                });
                checks.push(ok("roadmap", &summary));
            } else {
                checks.push(warn(
                    "roadmap",
                    &problems.join("; "),
                    "fix .harna/roadmap.toml — declared data must stay coherent",
                ));
            }
        }
        Err(e) => checks.push(fail(
            "roadmap",
            &format!("roadmap.toml unparseable: {e:#}"),
            "fix or delete .harna/roadmap.toml",
        )),
    }

    // Advisory ledger (0.3.31): accepted advisories — parseable when
    // locked; a corrupt ledger would silently re-arm everything (noisy
    // but safe) or, worse, be trusted blindly. Name the damage.
    match store.load_acknowledged() {
        Ok(a) if a.is_empty() => checks.push(ok("acknowledged", "none locked (optional)")),
        Ok(a) => checks.push(ok(
            "acknowledged",
            &format!("{} advisories locked with reasons", a.entries.len()),
        )),
        Err(e) => checks.push(fail(
            "acknowledged",
            &format!("acknowledged.toml unparseable: {e:#}"),
            "fix or delete .harna/acknowledged.toml",
        )),
    }

    // Decisions directory.
    if store.decisions_dir().is_dir() {
        let count = std::fs::read_dir(store.decisions_dir())
            .map(|d| d.flatten().count())
            .unwrap_or(0);
        checks.push(ok("decisions", &format!("{count} recorded")));
    } else {
        checks.push(warn(
            "decisions",
            "directory missing",
            "log your first decision with `harna log-decision`",
        ));
    }

    // Handoff: parseable when present — a corrupt burst journal would
    // silently strand the "where was I?" answer at session start.
    match crate::schema::journal::handoff::load(store) {
        Some(h) => checks.push(ok(
            "handoff",
            &format!("{} next step(s) recorded", h.declared_next.len()),
        )),
        None => {
            if crate::schema::journal::handoff::path(store).exists() {
                checks.push(fail(
                    "handoff",
                    "handoff.toml is unparseable",
                    "fix or delete .harna/handoff.toml",
                ));
            } else {
                checks.push(ok("handoff", "none recorded (optional)"));
            }
        }
    }

    Ok(())
}
