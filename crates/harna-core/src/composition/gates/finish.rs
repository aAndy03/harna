//! Component `finish` (1.1.0): the end of a work session in one command.
//!
//! The proof's agents each closed a session by hand — scan, render,
//! validate, parity, handoff — and each missed a different step (a stale
//! map committed, stale views, a parity event never opened). `finish` does
//! what is safe to do on its own (refresh the map and the views), checks
//! the change, and lists what is left with the command for each. It never
//! commits, never opens a parity event and never writes a handoff: those
//! carry the agent's judgement.

use std::path::Path;

use crate::schema::store::Store;

/// One line of the checklist.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub ok: bool,
    pub what: String,
    /// The command that settles it, when it is not settled.
    pub todo: Option<String>,
}

#[derive(Debug, Default)]
pub struct Finish {
    pub steps: Vec<Step>,
    /// The blocking findings of `validate --changed`, when there are any.
    pub findings: String,
}

impl Finish {
    pub fn ready(&self) -> bool {
        self.steps.iter().all(|s| s.ok)
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for s in &self.steps {
            out.push_str(&format!(
                "  [{}] {}{}\n",
                if s.ok { "ok" } else { "todo" },
                s.what,
                s.todo
                    .as_deref()
                    .map(|t| format!(" — {t}"))
                    .unwrap_or_default()
            ));
        }
        if !self.findings.is_empty() {
            out.push_str(&self.findings);
        }
        let open = self.steps.iter().filter(|s| !s.ok).count();
        out.push_str(&if open == 0 {
            "finish: ready to commit (run your tests first; harna runs no project code)\n"
                .to_string()
        } else {
            format!("finish: {open} thing(s) before the commit\n")
        });
        out
    }
}

pub fn run(store: &Store, root: &Path) -> anyhow::Result<Finish> {
    let mut f = Finish::default();
    let step = |ok: bool, what: String, todo: Option<String>| Step { ok, what, todo };

    // 1) the map follows the code
    let scan = crate::composition::scan::run(store, root, crate::search::digest::MAX_FILES, false)?;
    f.steps
        .push(step(true, format!("map: {}", scan.render()), None));

    // 2) the generated views follow the data
    let views = crate::generation::views::sync::all(root, store, true)?;
    f.steps.push(step(
        true,
        if views.changed.is_empty() {
            "views: current".into()
        } else {
            format!("views: rendered {}", views.changed.join(", "))
        },
        None,
    ));

    // 3) the change holds the rules
    let changed = crate::enforcement::validate::changed_rust_files(root).unwrap_or_default();
    let found = crate::enforcement::validate::run_on(
        root,
        store,
        crate::search::digest::MAX_FILES,
        Some(changed),
        None,
    )?;
    let report = crate::enforcement::validate::acknowledge::gate_for(store, found, false);
    f.steps.push(step(
        !report.blocks,
        format!(
            "validate --changed: {} error(s), {} info",
            report.errors, report.infos
        ),
        report
            .blocks
            .then(|| "fix the errors listed below".to_string()),
    ));
    if report.blocks {
        f.findings = report.render();
    }

    // 4) a public surface that moved owes its importers a look
    if crate::search::diff::head_exists(root) {
        if let (Ok(base), Ok(now)) = (
            crate::search::diff::digest_at_ref(root, "HEAD"),
            store.load_digest(),
        ) {
            let d = crate::search::diff::diff(&base, &now);
            let moved =
                d.signatures_removed.len() + d.modules_removed.len() + d.items_changed.len();
            let open = crate::enforcement::parity::open_debt_count(store).unwrap_or(0);
            if moved > 0 && open == 0 {
                f.steps.push(step(
                    false,
                    format!("public surface: {moved} signature(s) or module(s) removed or changed since HEAD"),
                    Some("`harna parity open <id>`, then resolve each target (ack or noop)".into()),
                ));
            } else if open > 0 {
                f.steps.push(step(
                    false,
                    format!("parity: {open} open target(s)"),
                    Some("`harna parity show --open`, then resolve each".into()),
                ));
            } else {
                f.steps
                    .push(step(true, "parity: nothing owed".into(), None));
            }
        }
    }

    // 5) the next session finds where this one stopped
    let handoff = crate::schema::journal::handoff::load(store);
    f.steps.push(match handoff {
        Some(h) => step(
            true,
            format!(
                "handoff: recorded {} — update it if this session moved on",
                h.created.get(..10).unwrap_or(&h.created)
            ),
            None,
        ),
        None => step(
            false,
            "handoff: none recorded".into(),
            Some("`harna handoff --summary .. --next ..`".into()),
        ),
    });

    // 6) the roadmap stays truthful
    if let Some(now) = store.load_roadmap().ok().and_then(|r| r.current().cloned()) {
        f.steps.push(step(
            true,
            format!(
                "roadmap NOW: {} — if this session finished it, `harna roadmap set {} done`",
                now.id, now.id
            ),
            None,
        ));
    }
    Ok(f)
}
