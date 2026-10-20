//! Command group `deferred`: the debt register — deliberate deferrals
//! with review triggers, promises to reconsider.

use std::path::Path;

use clap::Subcommand;

use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum DeferredCmd {
    /// Add or replace an entry (same id overwrites).
    Add {
        /// Stable id, e.g. fixture-builder-dedup.
        id: String,
        /// What collecting this deferral would touch (component name,
        /// module or container path).
        #[arg(long)]
        component: String,
        /// Why this is deliberately deferred, not an accident.
        #[arg(long)]
        reason: String,
        /// When to revisit: a date (2027-01-15), a version the project must
        /// reach (version:1.2.0), or a roadmap milestone (milestone:beta, or
        /// a declared id such as 0.9.0).
        #[arg(long)]
        review: String,
    },
    /// Remove an entry (it was collected, folded into a version, or the
    /// ask it guarded against never came).
    Remove { id: String },
    /// List the open deferrals, overdue ones flagged.
    List,
}

pub fn run(store: &Store, root: &Path, cmd: DeferredCmd) -> anyhow::Result<()> {
    let _ = root;
    match cmd {
        DeferredCmd::Add {
            id,
            component,
            reason,
            review,
        } => {
            let kind_hint = match harna_core::schema::registers::ledgers::deferred::review_kind(&review) {
                harna_core::schema::registers::ledgers::deferred::ReviewKind::IsoDate => "that date".to_string(),
                harna_core::schema::registers::ledgers::deferred::ReviewKind::Version => {
                    format!("version {review}")
                }
                harna_core::schema::registers::ledgers::deferred::ReviewKind::Milestone => format!(
                    "milestone `{review}` is done (declare it first: `harna roadmap add {review} --title \"..\"`; undeclared ids are checks' ghosts)"
                ),
            };
            let mut r = store.load_deferred()?;
            r.upsert(
                harna_core::schema::registers::ledgers::deferred::DeferredEntry {
                    id: id.clone(),
                    component,
                    reason,
                    review: review.clone(),
                    recorded: chrono::Local::now().format("%Y-%m-%d").to_string(),
                },
            );
            store.save_deferred(&r)?;
            println!(
                "deferred `{id}` registered — review when {kind_hint} arrives (harna status surfaces it, overdue reviews cost a debt)",
            );
        }
        DeferredCmd::Remove { id } => {
            let mut r = store.load_deferred()?;
            if !r.remove(&id) {
                anyhow::bail!("no deferred entry `{id}` registered");
            }
            store.save_deferred(&r)?;
            println!("deferred `{id}` removed (the promise is closed — say why in the decision log if it was collected)");
        }
        DeferredCmd::List => {
            let r = store.load_deferred()?;
            if r.entries.is_empty() {
                println!("debt register empty — no deliberate deferrals declared (decisions may still carry prose deferrals; `harna deferred add` tracks them as reviewed data)");
            }
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            let roadmap = store.load_roadmap().unwrap_or_default();
            for e in &r.entries {
                let kind = harna_core::schema::registers::ledgers::deferred::review_kind(&e.review);
                let due = match kind {
                    harna_core::schema::registers::ledgers::deferred::ReviewKind::Milestone => {
                        harna_core::schema::registers::ledgers::deferred::milestone_due(
                            &e.review,
                            &|id| roadmap.reached(id),
                        )
                    }
                    _ => harna_core::schema::registers::ledgers::deferred::review_due(
                        &e.review,
                        env!("CARGO_PKG_VERSION"),
                        &today,
                    ),
                };
                println!(
                    "- [{}] `{}` review {}{}{}",
                    e.id,
                    e.component,
                    e.review,
                    if due {
                        " — OVERDUE (costs a debt)"
                    } else if kind
                        == harna_core::schema::registers::ledgers::deferred::ReviewKind::Milestone
                    {
                        " (milestone)"
                    } else {
                        ""
                    },
                    if e.recorded.is_empty() {
                        String::new()
                    } else {
                        format!(" (recorded {})", e.recorded)
                    },
                );
                println!("  why: {}", e.reason);
            }
        }
    }
    Ok(())
}
