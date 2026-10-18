//! Command group `notes`: review checklists and release drafts,
//! stdout only.

use std::path::Path;

use clap::Subcommand;

use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum NotesCmd {
    /// A review checklist: an event's fan-out (or the diff from a ref),
    /// the scoped rules and the registered tests over the changed paths.
    Review {
        /// Parity event id; omit to review against `--from`'s diff.
        id: Option<String>,
        /// Git ref to diff against (default HEAD) when no event is given.
        #[arg(long)]
        from: Option<String>,
    },
    /// A release/PR note draft: decisions + parity events since a ref.
    Release {
        /// Git ref the draft covers up to (default HEAD).
        #[arg(long, default_value = "HEAD")]
        from: String,
    },
}

pub fn run(store: &Store, root: &Path, cmd: NotesCmd) -> anyhow::Result<()> {
    let _ = root;
    match cmd {
        NotesCmd::Review { id, from } => {
            if id.is_none() && from.is_none() {
                anyhow::bail!("give a parity event id, or --from <ref> to review a range");
            }
            let text = harna_core::composition::briefing::notes::review(
                store,
                id.as_deref(),
                from.as_deref(),
            )?;
            println!("{text}");
        }
        NotesCmd::Release { from } => {
            let text = harna_core::composition::briefing::notes::release(store, &from)?;
            println!("{text}");
        }
    }
    Ok(())
}
