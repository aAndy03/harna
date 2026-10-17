//! Command group `lesson`: tried-and-failed records with verdicts —
//! failure owned once, never re-suggested.

use std::path::Path;

use clap::Subcommand;

use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum LessonCmd {
    /// Add or replace a tried-and-failed record (same id overwrites).
    Add {
        /// Stable id, e.g. ps-roundtrip-source-files.
        id: String,
        /// The claim — what was attempted, in the direction of the failure.
        #[arg(long)]
        claim: String,
        /// The verdict — why it did not work out.
        #[arg(long)]
        verdict: String,
        /// Evidence pointers: decisions, commits, events (repeatable).
        #[arg(long = "evidence")]
        evidence: Vec<String>,
    },
    /// Remove a recorded dead end by id.
    Remove { id: String },
    /// List the recorded dead ends.
    List,
}

pub fn run(store: &Store, root: &Path, cmd: LessonCmd) -> anyhow::Result<()> {
    let _ = root;
    match cmd {
        LessonCmd::Add { id, claim, verdict, evidence } => {
            let mut l = store.load_lessons()?;
            l.upsert(harna_core::schema::lessons_data::Lesson {
                id: id.clone(),
                claim,
                verdict,
                evidence,
                recorded: chrono::Local::now().format("%Y-%m-%d").to_string(),
            });
            store.save_lessons(&l)?;
            println!(
                "lesson `{id}` recorded ({}) — new sessions meet this dead end at plan/place before re-suggesting it",
                l.lessons.len()
            );
        }
        LessonCmd::Remove { id } => {
            let mut l = store.load_lessons()?;
            if !l.remove(&id) {
                anyhow::bail!("no lesson `{id}` recorded");
            }
            store.save_lessons(&l)?;
            println!("lesson `{id}` removed — say why the verdict no longer holds (decision log), otherwise it was not stale");
        }
        LessonCmd::List => {
            let l = store.load_lessons()?;
            if l.lessons.is_empty() {
                println!("no dead ends recorded — tried-and-failed directions belong here as data (`harna lesson add`), so failure is owned once and not re-suggested");
            }
            for e in &l.lessons {
                let date = if e.recorded.is_empty() { String::new() } else { format!(" (recorded {})", e.recorded) };
                println!("  - [{}] {}{}", e.id, e.claim, date);
                println!("    verdict: {}", e.verdict);
                if !e.evidence.is_empty() {
                    println!("    evidence: {}", e.evidence.join("; "));
                }
            }
        }
    }
    Ok(())
}
