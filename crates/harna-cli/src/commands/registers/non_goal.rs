//! Command group `non-goal`: deliberate product refusals with reasons —
//! proposals police, not code.

use std::path::Path;

use clap::Subcommand;

use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum NonGoalCmd {
    /// Add or replace a refusal (same id overwrites).
    Add {
        /// Stable id, e.g. no-remote-state.
        id: String,
        /// What is refused, in imperative form.
        #[arg(long)]
        refusal: String,
        /// Why the refusal was made — the supersession briefing's evidence.
        #[arg(long)]
        reason: String,
    },
    /// Remove a refusal by id (record why in the decision log).
    Remove { id: String },
    /// List the refusals in force.
    List,
}

pub fn run(store: &Store, root: &Path, cmd: NonGoalCmd) -> anyhow::Result<()> {
    let _ = root;
    match cmd {
        NonGoalCmd::Add {
            id,
            refusal,
            reason,
        } => {
            let mut g = store.load_non_goals()?;
            g.upsert(harna_core::schema::registers::non_goals::NonGoal {
                id: id.clone(),
                refusal,
                reason,
                recorded: chrono::Local::now().format("%Y-%m-%d").to_string(),
            });
            store.save_non_goals(&g)?;
            println!(
                "refusal `{id}` in force ({}) — plan/place suppress matching proposals; superseding needs the user's agreement + `harna non-goal remove` + a logged decision",
                g.goals.len()
            );
        }
        NonGoalCmd::Remove { id } => {
            let mut g = store.load_non_goals()?;
            if !g.remove(&id) {
                anyhow::bail!("no refusal `{id}` recorded");
            }
            store.save_non_goals(&g)?;
            println!("refusal `{id}` removed — log WHY it was superseded (`harna log-decision`), or the removal itself is the betrayal risk");
        }
        NonGoalCmd::List => {
            let g = store.load_non_goals()?;
            if g.goals.is_empty() {
                println!("no refusals recorded — non-goals are deliberate product decisions (\"we will not do X\"), kept in data via `harna non-goal add`");
            }
            for e in &g.goals {
                let date = if e.recorded.is_empty() {
                    String::new()
                } else {
                    format!(" (recorded {})", e.recorded)
                };
                println!(
                    "  - [{}] {}{}\n    why: {}",
                    e.id, e.refusal, date, e.reason
                );
            }
        }
    }
    Ok(())
}
