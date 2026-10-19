//! Command group `command` (0.3.46): the repo's own commands as data —
//! what to run, what it proves, where it runs. Onboarding carries them;
//! `doc-claims` checks prose against them.

use std::path::Path;

use clap::Subcommand;

use harna_core::schema::registers::commands::Command;
use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum CommandCmd {
    /// Add or replace a command (same name overwrites).
    Add {
        /// Stable id, e.g. test-spec.
        name: String,
        /// The exact invocation, e.g. "./x test-spec".
        #[arg(long)]
        run: String,
        /// What a pass proves, e.g. "spec tests under tests/specs".
        #[arg(long)]
        proves: String,
        /// local | ci | both (default both).
        #[arg(long, default_value = "both")]
        runs: String,
        /// Paths it covers (repeatable); none = the whole repo.
        #[arg(long = "scope")]
        scopes: Vec<String>,
    },
    /// Remove a command by name.
    Remove { name: String },
    /// List the registry.
    List,
}

pub fn run(store: &Store, _root: &Path, cmd: CommandCmd) -> anyhow::Result<()> {
    let mut c = store.load_commands()?;
    match cmd {
        CommandCmd::Add {
            name,
            run,
            proves,
            runs,
            scopes,
        } => {
            if !["local", "ci", "both"].contains(&runs.as_str()) {
                anyhow::bail!("--runs is one of local | ci | both");
            }
            c.upsert(Command {
                name: name.clone(),
                run,
                proves,
                runs,
                scope: scopes,
            });
            store.save_commands(&c)?;
            println!(
                "command `{name}` recorded ({} in the registry)",
                c.commands.len()
            );
        }
        CommandCmd::Remove { name } => {
            if !c.remove(&name) {
                anyhow::bail!("no command `{name}` registered");
            }
            store.save_commands(&c)?;
            println!("command `{name}` removed");
        }
        CommandCmd::List if c.is_empty() => println!("no commands registered"),
        CommandCmd::List => {
            for x in &c.commands {
                println!("{} [{}]: {} — proves {}", x.name, x.runs, x.run, x.proves);
            }
        }
    }
    Ok(())
}
