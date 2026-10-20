//! Command group `profile` (0.4.2): starting shapes as data — list what is
//! available, show what one would write. `harna init --profile` applies.

use clap::Subcommand;

use harna_core::generation::structure::profile;

#[derive(Subcommand)]
pub enum ProfileCmd {
    /// Profiles shipped with harna and those under ~/.harna/profiles.
    List,
    /// What a profile declares: layers, rules, qualities, next steps.
    Show {
        /// A profile name or a path to a profile folder.
        spec: String,
    },
}

pub fn run(cmd: ProfileCmd) -> anyhow::Result<()> {
    let home = profile::home_dir();
    match cmd {
        ProfileCmd::List => {
            for p in profile::list(home.as_deref()) {
                println!("{} — {} ({})", p.meta.name, p.meta.description, p.source);
            }
            if let Some(h) = home {
                println!("your own profiles: folders under {} (profile.toml + any of manifest/rules/qualities/terms/commands/non-goals)", h.display());
            }
        }
        ProfileCmd::Show { spec } => {
            let p = profile::resolve(&spec, home.as_deref())?;
            print!("{}", profile::describe(&p));
        }
    }
    Ok(())
}
