//! Command group `service` (0.8.0): which services exist and what each
//! owns (`.harna/services.toml`); the corridors they talk through, with
//! their message shapes (0.8.1).

use clap::Subcommand;

use harna_core::schema::store::services::{Corridor, Message};
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
    /// The readiness facets every service must answer before a release
    /// (comma-separated, your words: health,backups,migrations); "" clears.
    Require {
        #[arg(required = true, value_delimiter = ',')]
        facets: Vec<String>,
    },
    /// Answer a facet for a service: where it lives, or `n/a: why`;
    /// "" removes the answer.
    Ready {
        service: String,
        facet: String,
        answer: String,
    },
    /// How services talk: a queue, an event bus, an API.
    Corridor {
        #[command(subcommand)]
        cmd: CorridorCmd,
    },
}

#[derive(Subcommand)]
pub enum CorridorCmd {
    /// Declare (or replace) a corridor: who sends, who receives, what it
    /// carries.
    Add {
        name: String,
        /// event | queue | api | rpc | … (your word).
        #[arg(long, default_value = "")]
        kind: String,
        /// Sending services (comma-separated).
        #[arg(long, value_delimiter = ',')]
        from: Vec<String>,
        /// Receiving services (comma-separated).
        #[arg(long, value_delimiter = ',')]
        to: Vec<String>,
        /// A message and its shape: Name=<file>::<Type> or Name=<schema
        /// file>; repeatable.
        #[arg(long = "message")]
        messages: Vec<String>,
    },
    Remove {
        name: String,
    },
}

pub fn run(store: &Store, cmd: ServiceCmd) -> anyhow::Result<()> {
    // every write goes through the one core the MCP tool uses (0.8.4)
    use harna_core::generation::structure::edit::services as edit;
    let mut services = store.load_services()?;
    let manifest = store.load_manifest()?;
    match cmd {
        ServiceCmd::Add {
            name,
            description,
            containers,
            owns,
            owners,
            runs,
        } => {
            let line = edit::put_service(
                &mut services,
                &manifest,
                &name,
                edit::ServiceEdit {
                    description: Some(description),
                    containers: Some(containers),
                    owns: Some(owns),
                    owners: Some(owners),
                    runs: Some(runs),
                },
                true,
            )?;
            println!("added {line}");
        }
        ServiceCmd::Set {
            name,
            description,
            containers,
            owns,
            owners,
            runs,
        } => {
            let line = edit::put_service(
                &mut services,
                &manifest,
                &name,
                edit::ServiceEdit {
                    description,
                    containers,
                    owns,
                    owners,
                    runs,
                },
                false,
            )?;
            println!("set {line}");
        }
        ServiceCmd::Remove { name } => println!("{}", edit::remove_service(&mut services, &name)?),
        ServiceCmd::Require { facets } => {
            println!("{}", edit::set_require(&mut services, Some(facets))?)
        }
        ServiceCmd::Ready {
            service,
            facet,
            answer,
        } => println!(
            "{}",
            edit::set_ready(&mut services, &service, &facet, &answer)?
        ),
        ServiceCmd::Corridor { cmd } => match cmd {
            CorridorCmd::Add {
                name,
                kind,
                from,
                to,
                messages,
            } => {
                let replaced = services.corridor(name.trim()).is_some();
                let c = Corridor {
                    name,
                    kind,
                    from,
                    to,
                    messages: messages
                        .iter()
                        .map(|m| Message::parse(m))
                        .collect::<anyhow::Result<_>>()?,
                };
                let line = edit::put_corridor(&mut services, &store.repo_root(), c)?;
                println!("{} {line}", if replaced { "replaced" } else { "added" });
            }
            CorridorCmd::Remove { name } => {
                println!("{}", edit::remove_corridor(&mut services, &name)?)
            }
        },
        ServiceCmd::Show { name } => {
            match name {
                None if services.is_empty() => {
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
                    if !services.require.is_empty() {
                        let unready = services.unready();
                        println!(
                            "readiness ({}): {}",
                            services.require.join(", "),
                            if unready.is_empty() {
                                "every service answers".to_string()
                            } else {
                                format!(
                                    "missing {}",
                                    unready
                                        .iter()
                                        .map(|(s, f)| format!("{s}/{f}"))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )
                            }
                        );
                    }
                }
                Some(n) if services.get(&n).is_none() && services.corridor(&n).is_some() => {
                    let c = services.corridor(&n).expect("checked");
                    println!("{}", c.line());
                    for m in &c.messages {
                        println!("  {} — shape {}", m.name, m.shape);
                    }
                }
                Some(n) => {
                    let Some(s) = services.get(&n) else {
                        anyhow::bail!("no service or corridor `{n}`");
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
                    for f in &services.require {
                        match s.ready.get(f) {
                            Some(a) => println!("  ready {f}: {a}"),
                            None => println!("  ready {f}: MISSING"),
                        }
                    }
                    for (f, a) in s
                        .ready
                        .iter()
                        .filter(|(f, _)| !services.require.contains(f))
                    {
                        println!("  ready {f}: {a} (not required)");
                    }
                }
            }
            return Ok(());
        }
    }
    store.save_services(&services)
}
