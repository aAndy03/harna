use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};

use harna_core::search::digest;
use harna_core::composition::onboarding;
use harna_core::schema::rules::Rule;
use harna_core::schema::store::Store;

/// CLI-side mirror of `scaffold::ScaffoldKind` so harna-core stays lean.
#[derive(Clone, clap::ValueEnum)]
enum KindArg {
    Primitive,
    Compound,
}

impl KindArg {
    fn to_core(self) -> harna_core::generation::scaffold::ScaffoldKind {
        match self {
            KindArg::Primitive => harna_core::generation::scaffold::ScaffoldKind::Primitive,
            KindArg::Compound => harna_core::generation::scaffold::ScaffoldKind::Compound,
        }
    }
}

#[derive(Parser)]
#[command(
    name = "harna",
    version,
    about = "Persistent agent context for Rust codebases — instructions become data in .harna/"
)]
struct Cli {
    /// Repository root (defaults to current directory).
    #[arg(long, global = true)]
    root: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize .harna/ in the repository.
    Init,
    /// Print the onboarding digest a fresh agent session starts with.
    Digest,
    /// List rules, optionally filtered by scope.
    Rules {
        /// Only rules applying to this scope path.
        #[arg(long)]
        scope: Option<String>,
        /// Only rules valid for this environment ("dev", "prod", ...). Env-tagged
        /// rules are hidden unless their env matches.
        #[arg(long)]
        env: Option<String>,
        /// Print the token-efficient agent rendering.
        #[arg(long)]
        agent: bool,
    },
    /// Add a rule and persist it to rules.toml.
    AddRule {
        /// Short stable id, e.g. no-hardcoded-urls.
        #[arg(long)]
        id: String,
        /// The rule in imperative form.
        #[arg(long)]
        text: String,
        /// Why the rule exists.
        #[arg(long)]
        rationale: String,
        /// Scope path(s) this rule applies to (repeatable; empty = global).
        #[arg(long = "scope")]
        scopes: Vec<String>,
        /// Restrict this rule to one environment ("dev", "prod", ...).
        #[arg(long)]
        env: Option<String>,
    },
    /// Record a decision (ADR-style markdown under .harna/decisions/).
    LogDecision {
        #[arg(long)]
        title: String,
        #[arg(long)]
        body: String,
    },
    /// Upgrade .harna/ data files to this tool's schema version (backups kept).
    Migrate,
    /// Print a component summary from the manifest.
    Manifest,
    /// Check the codebase for conformance violations (drift, hardcoded values, layer violations).
    Validate {
        /// Only check `.rs` files with uncommitted git changes (pre-commit / agent edit loop).
        #[arg(long)]
        changed: bool,
        /// Max files to check (safety cap).
        #[arg(long, default_value_t = 10_000)]
        max_files: usize,
    },
    /// Scaffold a new primitive/compound component and register it in the manifest.
    Scaffold {
        /// Layer name (created if missing).
        #[arg(long)]
        layer: String,
        /// Container name within the layer (created if missing).
        #[arg(long)]
        container: String,
        /// Component name in kebab-case, e.g. user-repo.
        #[arg(long)]
        name: String,
        /// primitive = leaf; compound = folder with mod.rs + starter child.
        #[arg(long, value_enum)]
        kind: KindArg,
        /// Explicit path; defaults to src/<layer>/<container>/<name>.
        #[arg(long)]
        path: Option<String>,
        /// One-line description embedded in the generated files.
        #[arg(long)]
        description: Option<String>,
    },
    /// Scaffold a transiter: a primitive dedicated to connecting two engines.
    Transiter {
        /// Layer the connection starts from, e.g. "api".
        #[arg(long)]
        from: String,
        /// Layer the connection reaches, e.g. "billing".
        #[arg(long)]
        to: String,
        /// Optional custom name; defaults to transiter-<from>-to-<to>.
        #[arg(long)]
        name: Option<String>,
    },
    /// Rebuild .harna/digest.json by scanning the repo.
    Scan {
        /// Max files to scan (safety cap).
        #[arg(long, default_value_t = 10_000)]
        max_files: usize,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let root = cli.root.unwrap_or_else(|| PathBuf::from("."));
    let store = Store::open(&root)?;

    match cli.command {
        Command::Init => {
            store.init()?;
            println!("initialized {}", store.manifest_path().parent().unwrap().display());
            println!("next: add rules with 'harna add-rule' and commit .harna/ to git");
        }
        Command::Digest => {
            let onb = onboarding::build(&store)?;
            print!("{}", onb.text);
            eprintln!(
                "\n[onboarding: {} rules, {} modules, manifest_empty={}]",
                onb.rule_count, onb.module_count, onb.manifest_empty
            );
        }
        Command::Rules { scope, env, agent } => {
            let rules = store.load_rules()?;
            let rendered = rules.render_for_agent(scope.as_deref().filter(|_| agent), env.as_deref());
            print!("{}", rendered);
        }
        Command::AddRule { id, text, rationale, scopes, env } => {
            let mut rules = store.load_rules()?;
            if rules.rules.iter().any(|r| r.id == id) {
                anyhow::bail!("rule id '{id}' already exists");
            }
            let origin = format!("user via harna CLI, {}", chrono::Utc::now().date_naive());
            rules.rules.push(Rule {
                id,
                text,
                rationale,
                scope: scopes,
                tags: vec![],
                origin,
                examples: vec![],
                env,
            });
            store.save_rules(&rules)?;
            println!("rule added; total: {}", rules.rules.len());
        }
        Command::LogDecision { title, body } => {
            let path = store.save_decision(&title, &body)?;
            println!("decision saved: {}", path.display());
        }
        Command::Manifest => {
            let m = store.load_manifest()?;
            if m.is_empty() {
                println!("manifest is empty — nothing recorded yet");
            } else {
                println!(
                    "{} layers, {} containers, {} components",
                    m.layers.len(),
                    m.container_count(),
                    m.component_count()
                );
            }
        }
        Command::Migrate => {
            let reports = harna_core::schema::migrate::run(&root, &store)?;
            for r in &reports {
                println!("{}", r.render());
            }
        }
        Command::Validate { changed, max_files } => {
            let files = if changed {
                let changed_files = harna_core::enforcement::validate::changed_rust_files(&root)?;
                if changed_files.is_empty() {
                    println!("no changed .rs files to check (everything committed or clean)");
                    return Ok(());
                }
                println!("checking {} changed file(s)…", changed_files.len());
                Some(changed_files)
            } else {
                None
            };
            let violations = harna_core::enforcement::validate::run_on(&root, &store, max_files, files)?;
            if violations.is_empty() {
                println!("no violations found");
            } else {
                for v in &violations {
                    println!("{}", v.render());
                }
                println!("\n{} violation(s)", violations.len());
            }
        }
        Command::Scaffold { layer, container, name, kind, path, description } => {
            let req = harna_core::generation::scaffold::ScaffoldRequest {
                layer,
                container,
                name,
                kind: kind.to_core(),
                path,
                description: description.unwrap_or_default(),
            };
            let res = harna_core::generation::scaffold::run(&root, &store, &req)?;
            for f in &res.files {
                println!("created: {}", f);
            }
            println!(
                "registered {} component(s) in {}",
                res.components.join(", "),
                res.registered_in
            );
        }
        Command::Transiter { from, to, name } => {
            let res = harna_core::generation::scaffold::scaffold_transiter(&root, &store, &from, &to, name)?;
            for f in &res.files {
                println!("created: {}", f);
            }
            println!("transiter registered in {}", res.registered_in);
        }
        Command::Scan { max_files } => {
            let d = digest::build(&root, max_files)?;
            store.save_digest(&d)?;
            println!(
                "digest written: {} modules, {} signatures",
                d.module_count(),
                d.signature_count()
            );
        }
    }
    Ok(())
}
