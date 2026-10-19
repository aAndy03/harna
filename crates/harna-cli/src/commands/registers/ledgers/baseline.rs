//! Command group `baseline` (0.3.44): the ratchet ledger — today's errors
//! frozen per (check, file), allowed to fall, never to grow.

use std::path::Path;

use clap::Subcommand;

use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum BaselineCmd {
    /// Freeze today's error counts per (check, file); growth then fails.
    Record {
        /// Only this check (e.g. no-panic); default: every erroring check.
        #[arg(long)]
        check: Option<String>,
    },
    /// Lower each baseline to the errors left (never raises one).
    Tighten,
    /// List the ledger.
    Show,
}

pub fn run(store: &Store, root: &Path, cmd: BaselineCmd) -> anyhow::Result<()> {
    let (action, check) = match cmd {
        BaselineCmd::Record { check } => ("record", check),
        BaselineCmd::Tighten => ("tighten", None),
        BaselineCmd::Show => ("show", None),
    };
    print!(
        "{}",
        harna_core::enforcement::validate::ratchet::command(root, store, action, check.as_deref())?
    );
    Ok(())
}
