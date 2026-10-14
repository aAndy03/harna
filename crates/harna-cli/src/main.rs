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
    Data,
}

impl KindArg {
    fn to_core(self) -> harna_core::generation::scaffold::ScaffoldKind {
        match self {
            KindArg::Primitive => harna_core::generation::scaffold::ScaffoldKind::Primitive,
            KindArg::Compound => harna_core::generation::scaffold::ScaffoldKind::Compound,
            KindArg::Data => harna_core::generation::scaffold::ScaffoldKind::Data,
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
        /// Bind the rule to an executable check id (violations cite the rule).
        #[arg(long)]
        check: Option<String>,
    },
    /// Deprecate a rule: visibly dead in the file, removed later.
    DeprecateRule {
        id: String,
        /// Why it is being retired (appended to the rationale).
        #[arg(long)]
        reason: Option<String>,
    },
    /// Re-activate a deprecated rule.
    ActivateRule { id: String },
    /// Remove a rule outright (git history keeps the record).
    RemoveRule { id: String },
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
        /// Info violations also block (default: only Error blocks).
        #[arg(long)]
        strict: bool,
        /// Emit a JSON report instead of prose.
        #[arg(long)]
        json: bool,
        /// Max files to check (safety cap).
        #[arg(long, default_value_t = 10_000)]
        max_files: usize,
    },
    /// One-shot health line: map size, rules, parity debts, digest age.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Who imports this module? ("Who breaks if I change this?")
    Dependents {
        /// Digest module path, crate-relative stem, or file name.
        module: String,
        #[arg(long)]
        json: bool,
    },
    /// Print the codebase's own hierarchy from the committed digest.
    Tree,
    /// Placement briefing for an idea, before code exists (PLAN step).
    Place {
        /// The idea in plain words, e.g. "timezone picker data".
        idea: String,
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
        /// Disable mtime reuse and re-parse every respected file.
        #[arg(long)]
        full: bool,
        /// Max files to scan (safety cap).
        #[arg(long, default_value_t = 10_000)]
        max_files: usize,
    },
    /// Generate deny.toml (compile-time layering) from the manifest.
    Deny,
    /// Compare the working tree's code map against a committed digest.
    Diff {
        /// Git ref whose .harna/digest.json is the baseline.
        #[arg(long, default_value = "HEAD")]
        from: String,
        /// Max files to scan (safety cap).
        #[arg(long, default_value_t = 10_000)]
        max_files: usize,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Parity ledger: open, show, and resolve change-propagation debts.
    Parity {
        #[command(subcommand)]
        cmd: ParityCmd,
    },
}

#[derive(Subcommand)]
enum ParityCmd {
    /// Open (or re-open = revise) a parity event from structural changes.
    Open {
        /// Event id, e.g. change-store-api.
        id: String,
        /// Event kind (rule-change, manual, digest-diff...).
        #[arg(long)]
        kind: Option<String>,
        /// Git ref whose committed digest is the baseline.
        #[arg(long, default_value = "HEAD")]
        from: String,
        #[arg(long, default_value_t = 10_000)]
        max_files: usize,
    },
    /// Show parity events with open-debt counts.
    Show {
        #[arg(long)]
        id: Option<String>,
        /// Only events with unresolved targets (debt view).
        #[arg(long)]
        open: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Resolve one target of one event.
    Resolve {
        id: String,
        /// Module path of the target (as listed by `parity show`).
        path: String,
        /// ack (work done) | noop (nothing needed) | auto (verified)
        #[arg(long)]
        status: String,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        by: Option<String>,
    },
    /// Re-verify open targets against the event's baseline (no new generation).
    Auto {
        /// Event id; omit to sweep every event.
        id: Option<String>,
        #[arg(long, default_value_t = 10_000)]
        max_files: usize,
    },
    /// Resolve several targets of one event in one go.
    Bulk {
        /// Event id.
        id: String,
        /// Status applied to every listed target: ack | noop | auto.
        #[arg(long)]
        status: String,
        /// Note applied to every listed target.
        #[arg(long)]
        note: Option<String>,
        /// File with module paths, one per line; omit to read paths from stdin.
        #[arg(long)]
        paths: Option<PathBuf>,
        /// Resolve every currently-open target of the event (ignores --paths).
        #[arg(long)]
        all_open: bool,
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
        Command::AddRule { id, text, rationale, scopes, env, check } => {
            let mut rules = store.load_rules()?;
            if rules.rules.iter().any(|r| r.id == id) {
                anyhow::bail!("rule id '{id}' already exists");
            }
            if let Some(c) = &check {
                if !harna_core::schema::rules::BUILTIN_CHECKS.contains(&c.as_str()) {
                    anyhow::bail!(
                        "unknown check `{c}` — known: {}",
                        harna_core::schema::rules::BUILTIN_CHECKS.join(", ")
                    );
                }
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
                check,
                status: None,
            });
            store.save_rules(&rules)?;
            println!("rule added; total: {}", rules.rules.len());
        }
        Command::DeprecateRule { id, reason } => {
            let mut rules = store.load_rules()?;
            rules.deprecate(&id, reason.as_deref().unwrap_or(""))?;
            store.save_rules(&rules)?;
            println!("rule `{id}` deprecated (visible in the file, no longer enforced by agents)");
        }
        Command::ActivateRule { id } => {
            let mut rules = store.load_rules()?;
            rules.activate(&id)?;
            store.save_rules(&rules)?;
            println!("rule `{id}` re-activated");
        }
        Command::RemoveRule { id } => {
            let mut rules = store.load_rules()?;
            rules.remove(&id)?;
            store.save_rules(&rules)?;
            println!("rule `{id}` removed (history lives in git)");
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
        Command::Validate { changed, max_files, strict, json } => {
            let files = if changed {
                let changed_files = harna_core::enforcement::validate::changed_rust_files(&root)?;
                if changed_files.is_empty() {
                    if json { println!("{{\"violations\":[],\"clean\":true,\"counts\":{{\"error\":0,\"info\":0}}}}"); }
                    else { println!("no changed .rs files to check (everything committed or clean)"); }
                    return Ok(());
                }
                println!("checking {} changed file(s)…", changed_files.len());
                Some(changed_files)
            } else {
                None
            };
            let violations = harna_core::enforcement::validate::run_on(&root, &store, max_files, files)?;

            let errors = violations.iter().filter(|v| v.severity == harna_core::enforcement::validate::Severity::Error).count();
            let infos = violations.len() - errors;
            let gate = errors > 0 || (strict && infos > 0);

            if json {
                let report = serde_json::json!({
                    "violations": violations,
                    "clean": violations.is_empty(),
                    "counts": { "error": errors, "info": infos },
                    "blocks": gate,
                });
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else if violations.is_empty() {
                println!("no violations found");
            } else {
                for v in &violations {
                    println!("{}", v.render());
                }
                println!("\n{} violation(s) ({e} error, {i} info)", violations.len(), e = errors, i = infos);
            }

            if gate {
                // Non-zero exit so pre-commit hooks and CI can gate on this.
                eprintln!("harna: conformance gate failed ({} error, {} info{})",
                    errors, infos,
                    if strict && infos > 0 && errors == 0 { " — strict mode" } else { "" });
                {
                    use std::io::Write as _;
                    let _ = std::io::stdout().flush();
                }
                process::exit(2);
            }
        }
        Command::Status { json } => {
            let s = harna_core::composition::status::build(&store)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&s)?);
            } else {
                print!("{}", s.render());
            }
        }
        Command::Place { idea } => {
            print!("{}", harna_core::composition::place::build(&store, &idea)?);
        }
        Command::Tree => {
            let d = store.load_digest().map_err(|_| {
                anyhow::anyhow!("no code map — run `harna scan` first")
            })?;
            print!("{}", d.render_tree());
        }
        Command::Dependents { module, json } => {
            let r = harna_core::search::dependents::build(&store, &module)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", r.render());
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
            println!("{}", res.message);
        }
        Command::Transiter { from, to, name } => {
            let res = harna_core::generation::scaffold::scaffold_transiter(&root, &store, &from, &to, name)?;
            println!("{}", res.message);
        }
        Command::Scan { max_files, full } => {
            let started = std::time::Instant::now();
            let previous = store.load_digest().ok();
            let (d, reused, rescanned) =
                digest::build_incremental(&root, max_files, previous.as_ref(), full)?;
            store.save_digest(&d)?;
            let elapsed = started.elapsed();
            println!(
                "digest written: {} modules, {} signatures (reused {reused}, scanned {rescanned}, {} ms)",
                d.module_count(),
                d.signature_count(),
                elapsed.as_millis()
            );
        }
        Command::Deny => {
            let manifest = store.load_manifest()?;
            let plan = harna_core::generation::deny::plan(&root, &manifest)?;
            let path = harna_core::generation::deny::generate(&root, &manifest)?;
            println!("deny.toml written: {}", path.display());
            for r in &plan.rules {
                let deps = if r.wrappers.is_empty() {
                    "nobody".to_string()
                } else {
                    r.wrappers.join(", ")
                };
                println!("  {} may be depended on by: {}", r.crate_name, deps);
            }
            println!("CI: `cargo deny check bans` enforces the layer graph at compile time.");
        }
        Command::Diff { from, max_files, json } => {
            let fresh = digest::build(&root, max_files)?;
            let baseline = harna_core::search::diff::digest_at_ref(&root, &from)?;
            let mut d =
                harna_core::search::diff::diff(&baseline, &fresh);
            d.from = from.clone();
            d.to = "working-tree".into();
            if json {
                println!("{}", serde_json::to_string_pretty(&d)?);
            } else {
                print!("{}", d.render());
            }
        }
        Command::Parity { cmd } => match cmd {
            ParityCmd::Open { id, kind, from, max_files } => {
                let ev = harna_core::enforcement::parity::open(
                    &root,
                    &store,
                    &id,
                    kind.as_deref().unwrap_or("digest-diff"),
                    "",
                    &from,
                    max_files,
                )?;
                let open = ev.open_targets().len();
                println!(
                    "parity `{}` gen{}: {} open / {} targets — resolve with 'harna parity resolve'",
                    ev.id,
                    ev.generation,
                    open,
                    ev.targets.len()
                );
            }
            ParityCmd::Show { id, open, json } => {
                if json {
                    let mut events = harna_core::enforcement::parity::list_events(&store)?;
                    if let Some(only) = id.as_deref() {
                        events.retain(|e| e.id == only);
                        if events.is_empty() {
                            anyhow::bail!("parity event `{only}` not found");
                        }
                    }
                    if open {
                        events.retain(|e| e.open_targets().len() > 0);
                    }
                    println!("{}", serde_json::to_string_pretty(&events)?);
                } else {
                    print!(
                        "{}",
                        harna_core::enforcement::parity::show(&store, id.as_deref(), open)?
                    );
                }
            }
            ParityCmd::Resolve { id, path, status, note, by } => {
                let ev = harna_core::enforcement::parity::resolve(
                    &store,
                    &id,
                    &path,
                    &status,
                    note.as_deref().unwrap_or(""),
                    by.as_deref().unwrap_or("user via harna CLI"),
                )?;
                let target = ev.targets.iter().find(|t| t.path == path).unwrap();
                println!(
                    "resolved `{path}` -> {} in `{}`; {} open target(s) left",
                    target.status,
                    ev.id,
                    ev.open_targets().len()
                );
            }
            ParityCmd::Auto { id, max_files } => {
                match id.as_deref() {
                    Some(id) => {
                        let (ev, verified, open) = harna_core::enforcement::parity::auto(
                            &root, &store, id, max_files,
                        )?;
                        println!(
                            "parity `{}`: {} auto-verified, {} still open (generation unchanged)",
                            ev.id, verified, open
                        );
                    }
                    None => {
                        for line in harna_core::enforcement::parity::auto_all(&root, &store, max_files)? {
                            println!("{line}");
                        }
                    }
                }
            }
            ParityCmd::Bulk { id, status, note, paths, all_open } => {
                // Where the paths come from: --all_open, a file, or stdin.
                let mut entries: Vec<(String, String, String)> = Vec::new();
                if all_open {
                    let events = harna_core::enforcement::parity::list_events(&store)?;
                    let ev = events.iter().find(|e| e.id == id).ok_or_else(|| {
                        anyhow::anyhow!("parity event `{id}` not found")
                    })?;
                    for t in ev.open_targets() {
                        entries.push((t.path.clone(), status.clone(), note.clone().unwrap_or_default()));
                    }
                    if entries.is_empty() {
                        anyhow::bail!("event `{id}` has no open targets");
                    }
                } else {
                    let text = match paths {
                        Some(f) => std::fs::read_to_string(f)?,
                        None => {
                            use std::io::Read;
                            let mut s = String::new();
                            std::io::stdin().read_to_string(&mut s)?;
                            s
                        }
                    };
                    for line in text.lines() {
                        let path = line.trim();
                        if path.is_empty() {
                            continue;
                        }
                        entries.push((path.into(), status.clone(), note.clone().unwrap_or_default()));
                    }
                }
                let (ev, missing) = harna_core::enforcement::parity::resolve_bulk(
                    &store,
                    &id,
                    &entries,
                    "user via harna CLI",
                )?;
                println!(
                    "bulk {status} on `{}`: {} resolved, {} open left",
                    ev.id,
                    entries.len() - missing.len(),
                    ev.open_targets().len()
                );
                for m in missing {
                    eprintln!("warning: no target `{m}` in event `{id}`");
                }
            }
        },
    }
    Ok(())
}
