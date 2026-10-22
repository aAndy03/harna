//! Command group `roadmap`: declared milestones, never generated —
//! the project's path as data.

use std::path::Path;

use clap::Subcommand;

use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum RoadmapCmd {
    /// Add or replace a milestone (same id overwrites).
    Add {
        /// Stable id, e.g. 0.3.28 or phase-h.
        id: String,
        /// What the milestone delivers, in one line.
        #[arg(long)]
        title: String,
        /// One of declared|current|done|dropped (default declared).
        #[arg(long, default_value = "declared")]
        state: String,
        /// Optional longer goal statement.
        #[arg(long)]
        notes: Option<String>,
    },
    /// Move a milestone's state (the whole point of the roadmap: NOW moves).
    Set {
        /// Milestone id.
        id: String,
        /// One of declared|current|done|dropped.
        state: String,
    },
    /// Remove a milestone by id (say why in the decision log if it's a drop).
    Remove { id: String },
    /// List the declared path.
    List,
}

pub fn run(store: &Store, root: &Path, cmd: RoadmapCmd) -> anyhow::Result<()> {
    let _ = root;
    match cmd {
        RoadmapCmd::Add {
            id,
            title,
            state,
            notes,
        } => {
            if !harna_core::schema::registers::roadmap::Milestone::STATES.contains(&state.as_str())
            {
                anyhow::bail!("unknown state `{state}` — one of declared|current|done|dropped");
            }
            let mut r = store.load_roadmap()?;
            // the state it was given, not "declared" (1.1.9 field test)
            let said = state.clone();
            r.upsert(harna_core::schema::registers::roadmap::Milestone {
                id: id.clone(),
                title,
                state,
                notes: notes.unwrap_or_default(),
            });
            store.save_roadmap(&r)?;
            println!(
                "milestone `{id}` recorded as `{said}` ({} on the roadmap) — the roadmap is never generated; coherence is checked, ownership is yours",
                r.milestones.len()
            );
        }
        RoadmapCmd::Set { id, state } => {
            if !harna_core::schema::registers::roadmap::Milestone::STATES.contains(&state.as_str())
            {
                anyhow::bail!("unknown state `{state}` — one of declared|current|done|dropped");
            }
            let mut r = store.load_roadmap()?;
            if !r.milestones.iter().any(|m| m.id.eq_ignore_ascii_case(&id)) {
                anyhow::bail!("no milestone `{id}` declared");
            }
            for m in r.milestones.iter_mut() {
                if m.id.eq_ignore_ascii_case(&id) {
                    m.state = state.clone();
                }
            }
            store.save_roadmap(&r)?;
            println!(
                "milestone `{id}` is now `{state}` — when a milestone lands `done`, milestone-keyed deferred reviews go due"
            );
        }
        RoadmapCmd::Remove { id } => {
            let mut r = store.load_roadmap()?;
            if !r.remove(&id) {
                anyhow::bail!("no milestone `{id}` declared");
            }
            store.save_roadmap(&r)?;
            println!("milestone `{id}` removed — log why (decision log), or the path misrepresented itself");
        }
        RoadmapCmd::List => {
            let r = store.load_roadmap()?;
            if r.milestones.is_empty() {
                println!("no milestones declared — a roadmap is declared data, never generated (`harna roadmap add <id> --title \"..\"`)");
            }
            for m in &r.milestones {
                let notes = if m.notes.is_empty() {
                    String::new()
                } else {
                    format!("\n    notes: {}", m.notes)
                };
                println!("  [{}] {} = {}{}", m.state, m.id, m.title, notes);
            }
        }
    }
    Ok(())
}
