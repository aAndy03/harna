//! Command group `acknowledge`: the advisory ledger — lock an accepted
//! advisory with a reason, re-arm it, or list what is locked. Errors
//! are never acknowledgeable; gates are not suggestions.

use std::path::Path;

use clap::Subcommand;

use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum AckCmd {
    /// Lock away an accepted advisory: a specific Info finding (check +
    /// file) seen, reasoned about, kept with a why. Errors are never
    /// acknowledgeable — gates are not suggestions.
    Add {
        /// The check that fired, e.g. no-copy-paste.
        check: String,
        /// The file it fired on, repo-relative.
        #[arg(long)]
        file: String,
        /// Why the advisory is accepted rather than fixed.
        #[arg(long)]
        reason: String,
    },
    /// Re-arm an acknowledged advisory by check + file.
    Remove {
        /// The check that was acknowledged.
        check: String,
        /// The file it was acknowledged on, repo-relative.
        #[arg(long)]
        file: String,
    },
    /// List the locked-away advisories with their reasons.
    List,
}

pub fn run(store: &Store, root: &Path, cmd: AckCmd) -> anyhow::Result<()> {
    let _ = root;
    match cmd {
        AckCmd::Add { check, file, reason } => {
            let mut a = store.load_acknowledged()?;
            a.upsert(harna_core::schema::acknowledged_data::Acknowledgement {
                check: check.clone(),
                file,
                reason,
                recorded: chrono::Local::now().format("%Y-%m-%d").to_string(),
            });
            store.save_acknowledged(&a)?;
            println!(
                "advisory `{check}` locked for its file ({}) — Infos only; an error by that name still gates",
                a.entries.len()
            );
        }
        AckCmd::Remove { check, file } => {
            let mut a = store.load_acknowledged()?;
            if !a.remove(&check, &file) {
                anyhow::bail!("no acknowledgment for `{check}` on `{file}`");
            }
            store.save_acknowledged(&a)?;
            println!("acknowledgment removed — the advisory nags again (as it should, until re-accepted)");
        }
        AckCmd::List => {
            let a = store.load_acknowledged()?;
            if a.entries.is_empty() {
                println!("advisory ledger empty — accepted Infos belong here with a reason (`harna acknowledge add <check> --file <f> --reason \"..\"`)");
            }
            for e in &a.entries {
                let date = if e.recorded.is_empty() { String::new() } else { format!(" (recorded {})", e.recorded) };
                println!("  - [{}] {}{}\n    why: {}", e.check, e.file, date, e.reason);
            }
        }
    }
    Ok(())
}
