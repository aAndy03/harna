//! Command group `hooks`: install harna's own enforcement (git pre-commit,
//! Claude Code hooks, CI) and audit it; `harna hook <event>` is what the
//! installed Claude Code hooks call.

use std::io::Read as _;
use std::path::Path;

use clap::Subcommand;

use harna_core::composition::{hook_events, hook_setup};
use harna_core::generation::hooks::Target;
use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum HooksCmd {
    /// Write and wire the hooks: git pre-commit (+ core.hooksPath), Claude
    /// Code hooks merged into .claude/settings.json, a CI workflow. Never
    /// overwrites a file you wrote (unless --force).
    Install {
        /// Only these targets: git | claude | ci (repeatable; default all).
        #[arg(long = "target")]
        targets: Vec<String>,
        /// Command the Claude Code hooks run (default: `harna` on PATH).
        #[arg(long, default_value = "harna")]
        bin: String,
        /// Replace files that were not written by harna.
        #[arg(long)]
        force: bool,
    },
    /// Which hooks are installed and current.
    Status {
        #[arg(long, default_value = "harna")]
        bin: String,
    },
}

pub fn run(store: &Store, root: &Path, cmd: HooksCmd) -> anyhow::Result<()> {
    let _ = store;
    match cmd {
        HooksCmd::Install { targets, bin, force } => {
            let targets = if targets.is_empty() {
                Target::ALL.to_vec()
            } else {
                targets.iter().map(|t| Target::parse(t)).collect::<anyhow::Result<Vec<_>>>()?
            };
            let report = hook_setup::install(root, &targets, &bin, force)?;
            print!("{}", report.render());
            if hook_setup::needs_attention(&report) {
                eprintln!("harna: some hooks were left untouched — see above");
            }
        }
        HooksCmd::Status { bin } => print!("{}", hook_setup::status(root, &bin)),
    }
    Ok(())
}

/// `harna hook <event>`: answer an agent runtime's hook call — streams
/// and exit code are the protocol.
pub fn answer(store: &Store, root: &Path, event: &str) -> anyhow::Result<()> {
    let mut input = String::new();
    if event == "stop" {
        let _ = std::io::stdin().read_to_string(&mut input);
    }
    let reply = hook_events::answer(root, store, event, &input)?;
    print!("{}", reply.stdout);
    eprint!("{}", reply.stderr);
    if reply.code != 0 {
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
        std::process::exit(reply.code);
    }
    Ok(())
}
