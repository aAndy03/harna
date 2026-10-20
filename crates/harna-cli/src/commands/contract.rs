//! Command group `contract` (0.7.2): freeze a container's surface as a
//! versioned contract; show what one holds.

use clap::Subcommand;

use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum ContractCmd {
    /// Freeze the container's surface (its provides, with signatures) at a
    /// version; a breaking change is accepted by freezing a new major.
    Freeze {
        container: String,
        /// Semver, e.g. 1.0.0 (default: the next minor, or 1.0.0).
        #[arg(long)]
        version: Option<String>,
    },
    /// What a contract holds: version, names, signatures.
    Show { container: String },
}

pub fn run(store: &Store, cmd: ContractCmd) -> anyhow::Result<()> {
    use harna_core::schema::store::contracts::semver;
    match cmd {
        ContractCmd::Freeze { container, version } => {
            let manifest = store.load_manifest()?;
            let digest = store.load_digest()?;
            let previous = store.load_contract(&container)?;
            let version = version.unwrap_or_else(|| match &previous {
                Some(p) => {
                    let (a, b, _) = semver(&p.version);
                    format!("{a}.{}.0", b + 1)
                }
                None => "1.0.0".into(),
            });
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            let c = harna_core::generation::structure::edit::contract::freeze(
                &manifest,
                &digest,
                &container,
                &version,
                previous.as_ref(),
                &today,
            )?;
            store.save_contract(&c)?;
            println!(
                "contract `{container}` frozen at v{version}: {} name(s){} — validate now holds the code to it (check contract)",
                c.items.len(),
                previous
                    .map(|p| format!(" (was v{})", p.version))
                    .unwrap_or_default()
            );
        }
        ContractCmd::Show { container } => match store.load_contract(&container)? {
            Some(c) => {
                println!(
                    "contract `{}` v{} (frozen {})",
                    c.container, c.version, c.frozen
                );
                for (n, sig) in &c.items {
                    println!("  {n}: {sig}");
                }
            }
            None => println!("no contract for `{container}` (harna contract freeze {container})"),
        },
    }
    Ok(())
}
