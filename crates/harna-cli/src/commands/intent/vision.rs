//! `harna vision` (1.1.2): keep the owner's vision as written.

use clap::Subcommand;

use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum VisionCmd {
    /// Keep the vision: from a file (the prompt as the owner wrote it) or text.
    Set {
        /// A file holding the vision, e.g. VISION.md.
        #[arg(long)]
        file: Option<String>,
        /// The vision as text.
        #[arg(long)]
        text: Option<String>,
    },
    /// Print the vision.
    Show,
}

pub fn run(store: &Store, root: &std::path::Path, cmd: VisionCmd) -> anyhow::Result<()> {
    match cmd {
        VisionCmd::Set { file, text } => {
            let body = match (file, text) {
                (Some(f), None) => std::fs::read_to_string(root.join(&f))
                    .map_err(|e| anyhow::anyhow!("cannot read `{f}`: {e}"))?,
                (None, Some(t)) => t,
                _ => anyhow::bail!("give --file <path> or --text \"..\" (one of them)"),
            };
            store.save_vision(&body)?;
            let line = harna_core::schema::registers::assumptions::vision::summary(&body)
                .unwrap_or_default();
            println!(
                "vision kept in .harna/vision.md ({line}) — plan toward it in stages; record guesses with `harna assume add`"
            );
        }
        VisionCmd::Show => match store.load_vision() {
            Some(v) => print!("{v}"),
            None => println!("no vision kept — `harna vision set --file <prompt.md>`"),
        },
    }
    Ok(())
}
