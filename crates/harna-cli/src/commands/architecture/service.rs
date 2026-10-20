//! Command group `service` (0.8.0): which services exist and what each
//! owns (`.harna/services.toml`).

use clap::Subcommand;

use harna_core::schema::store::services::Service;
use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum ServiceCmd {
    /// Declare a service: the containers only it runs, the resources only
    /// it writes.
    Add {
        name: String,
        #[arg(long, default_value = "")]
        description: String,
        /// Containers it runs (comma-separated).
        #[arg(long, value_delimiter = ',')]
        containers: Vec<String>,
        /// Resources it alone writes, `kind:name` (db:ledger, queue:sync).
        #[arg(long, value_delimiter = ',')]
        owns: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        owners: Vec<String>,
        /// Folder of its deploy unit (crate or package).
        #[arg(long, default_value = "")]
        runs: String,
    },
    /// Change a service; a given list replaces the old one ("" clears it).
    Set {
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long, value_delimiter = ',')]
        containers: Option<Vec<String>>,
        #[arg(long, value_delimiter = ',')]
        owns: Option<Vec<String>>,
        #[arg(long, value_delimiter = ',')]
        owners: Option<Vec<String>>,
        #[arg(long)]
        runs: Option<String>,
    },
    Remove {
        name: String,
    },
    /// Every service, or one in full.
    Show {
        name: Option<String>,
    },
}

fn clean(v: Vec<String>) -> Vec<String> {
    v.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn run(store: &Store, cmd: ServiceCmd) -> anyhow::Result<()> {
    let mut services = store.load_services()?;
    let manifest = store.load_manifest()?;
    let known = |c: &str| {
        manifest
            .layers
            .iter()
            .flat_map(|l| &l.containers)
            .any(|x| x.name == c)
    };
    let refuse_unknown = |s: &Service| -> anyhow::Result<()> {
        if let Some(c) = s.containers.iter().find(|c| !known(c)) {
            anyhow::bail!("no container `{c}` in the manifest (harna container add first)");
        }
        Ok(())
    };
    match cmd {
        ServiceCmd::Add {
            name,
            description,
            containers,
            owns,
            owners,
            runs,
        } => {
            if services.get(&name).is_some() {
                anyhow::bail!("service `{name}` exists — harna service set {name} ...");
            }
            let s = Service {
                name: name.clone(),
                description,
                containers: clean(containers),
                owns: clean(owns),
                owners: clean(owners),
                runs,
            };
            refuse_unknown(&s)?;
            println!("added {}", s.line());
            services.services.push(s);
        }
        ServiceCmd::Set {
            name,
            description,
            containers,
            owns,
            owners,
            runs,
        } => {
            let Some(s) = services.services.iter_mut().find(|s| s.name == name) else {
                anyhow::bail!("no service `{name}` (harna service add)");
            };
            if let Some(d) = description {
                s.description = d;
            }
            if let Some(c) = containers {
                s.containers = clean(c);
            }
            if let Some(o) = owns {
                s.owns = clean(o);
            }
            if let Some(o) = owners {
                s.owners = clean(o);
            }
            if let Some(r) = runs {
                s.runs = r;
            }
            let s = s.clone();
            refuse_unknown(&s)?;
            println!("set {}", s.line());
        }
        ServiceCmd::Remove { name } => {
            let before = services.services.len();
            services.services.retain(|s| s.name != name);
            if services.services.len() == before {
                anyhow::bail!("no service `{name}`");
            }
            println!("removed service `{name}`");
        }
        ServiceCmd::Show { name } => {
            match name {
                None if services.services.is_empty() => {
                    println!("no services declared — harna service add <name> --containers ... --owns db:...")
                }
                None => {
                    for l in services.lines() {
                        println!("{l}");
                    }
                    let shared: Vec<String> = manifest
                        .layers
                        .iter()
                        .flat_map(|l| &l.containers)
                        .filter(|c| services.service_of(&c.name).is_none())
                        .map(|c| c.name.clone())
                        .collect();
                    if !shared.is_empty() {
                        println!("shared (no service runs them alone): {}", shared.join(", "));
                    }
                }
                Some(n) => {
                    let Some(s) = services.get(&n) else {
                        anyhow::bail!("no service `{n}`");
                    };
                    println!("service {}", s.name);
                    if !s.description.is_empty() {
                        println!("  {}", s.description);
                    }
                    let list = |v: &[String]| {
                        if v.is_empty() {
                            "none".to_string()
                        } else {
                            v.join(", ")
                        }
                    };
                    println!("  runs containers: {}", list(&s.containers));
                    println!("  owns: {}", list(&s.owns));
                    println!("  owners: {}", list(&s.owners));
                    if !s.runs.is_empty() {
                        println!("  deploys from: {}", s.runs);
                    }
                }
            }
            return Ok(());
        }
    }
    store.save_services(&services)
}
