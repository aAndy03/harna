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
    /// Generate a plugin folder's api.toml from a release's source (for
    /// plugin maintainers; re-run on upgrade). Check api-parity reads it.
    Api {
        /// The plugin folder to write api.toml into.
        dir: std::path::PathBuf,
        /// The crate name as code spells it, e.g. gpui_kit.
        #[arg(long = "crate")]
        crate_name: String,
        /// The release the source is, e.g. 0.8.0.
        #[arg(long)]
        version: String,
        /// prefix=<crate src dir>, repeatable: "=<src>" for the crate's own
        /// root, "component=<src>" for a crate it re-exports as a module.
        #[arg(long = "mount", required = true)]
        mounts: Vec<String>,
    },
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
        PluginCmd::Api {
            dir,
            crate_name,
            version,
            mounts,
        } => {
            let mounts: Vec<(String, std::path::PathBuf)> = mounts
                .iter()
                .map(|m| {
                    m.split_once('=')
                        .map(|(p, s)| (p.to_string(), std::path::PathBuf::from(s)))
                        .ok_or_else(|| {
                            anyhow::anyhow!("--mount takes prefix=<src dir> (got `{m}`)")
                        })
                })
                .collect::<anyhow::Result<_>>()?;
            println!(
                "{}",
                plugin::write_api(&dir, &crate_name, &version, &mounts)?
            );
        }
    }
    Ok(())
}
