//! Command group `plugin` (0.5.0): framework knowledge as versioned packs —
//! list what is available and installed, show one, add (or upgrade), remove.

use clap::Subcommand;

use harna_core::generation::structure::plugin;
use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum PluginCmd {
    /// Installed plugins, then the ones shipped or under ~/.harna/plugins.
    List,
    /// What a plugin carries: mirrors, sources, rules, terms.
    Show {
        /// A plugin name or a path to a plugin folder.
        spec: String,
    },
    /// Install (or upgrade) a plugin into .harna/plugins/<name>/.
    Add {
        /// A plugin name or a path to a plugin folder.
        spec: String,
    },
    /// Uninstall a plugin: its rules and terms stop applying.
    Remove { name: String },
}

pub fn run(store: &Store, cmd: PluginCmd) -> anyhow::Result<()> {
    let home = plugin::home_dir();
    match cmd {
        PluginCmd::List => {
            let installed = store.load_plugins()?;
            if installed.is_empty() {
                println!("installed: none");
            }
            for p in &installed {
                println!(
                    "installed: {} {} — {} rule(s), {} term(s)",
                    p.meta.name,
                    p.meta.version,
                    p.rules.rules.len(),
                    p.terms.terms.len()
                );
            }
            for p in plugin::available(home.as_deref()) {
                if !installed.iter().any(|i| i.meta.name == p.meta.name) {
                    println!(
                        "available: {} {} — {} ({})",
                        p.meta.name, p.meta.version, p.meta.description, p.source
                    );
                }
            }
        }
        PluginCmd::Show { spec } => {
            print!(
                "{}",
                plugin::describe(&plugin::resolve(&spec, home.as_deref())?)
            );
        }
        PluginCmd::Add { spec } => {
            let pack = plugin::resolve(&spec, home.as_deref())?;
            harna_core::enforcement::checks::admit_pack(&pack.rules())?;
            store.init()?;
            for line in plugin::add(store, &pack)? {
                println!("{line}");
            }
        }
        PluginCmd::Remove { name } => println!("{}", plugin::remove(store, &name)?),
    }
    Ok(())
}
