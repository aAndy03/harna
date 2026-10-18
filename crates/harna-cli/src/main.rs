use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};

use harna_core::search::digest;
use harna_core::composition::onboarding;
use harna_core::schema::rules::Rule;
use harna_core::schema::store::Store;

mod commands;
use commands::{
    ack::AckCmd, deferred::DeferredCmd, lesson::LessonCmd, non_goal::NonGoalCmd,
    notes::NotesCmd, parity::ParityCmd, roadmap::RoadmapCmd, templates::TemplatesCmd,
    terms::TermsCmd,
};

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
    Init {
        /// Start from another repo's rules + manifest skeleton: path to
        /// its root (reads .harna/) or to a .harna/ directory itself.
        /// Copied verbatim as a starting point to prune.
        #[arg(long)]
        from: Option<PathBuf>,
    },
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
        /// Override the bound check's severity ("error" | "info"); requires --check.
        #[arg(long)]
        severity: Option<String>,
        /// Naming convention as lint-as-data: a regex matched over file
        /// paths and declared names (check `pattern`). Requires no --check.
        #[arg(long)]
        pattern: Option<String>,
        /// Budget for `container-size` / `fan-out-cap` rules: components
        /// per container, imports per component.
        #[arg(long)]
        limit: Option<u32>,
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
        /// The decision file this one supersedes (name in .harna/decisions/).
        /// The old decision stays referenced but visibly dead.
        #[arg(long)]
        supersedes: Option<String>,
    },
    /// Decision log: every decision, superseded ones visibly dead.
    Decisions {
        /// Optional: render the chain around this decision (filename or
        /// unique prefix) — successors first, then its supersedes ancestors.
        chain: Option<String>,
    },
    /// Upgrade .harna/ data files to this tool's schema version (backups kept,
    /// verified readable). Restore rolls the newest backup back over the live file.
    Migrate {
        /// Preview what would move — nothing is written.
        #[arg(long)]
        dry: bool,
    },
    /// Restore the newest backup of a data file (.harna/backup/<name>.vN).
    /// The backup is verified readable before it is swapped in.
    Restore {
        /// Plain data-file name, e.g. manifest.toml.
        name: String,
    },
    /// Archive closed parity events, superseded decisions and old score
    /// snapshots into .harna/archive/ — the live surface stays lean.
    Archive {
        /// Score snapshots kept live (newest kept first; the rest archive).
        #[arg(long, default_value_t = harna_core::composition::score::SNAPSHOT_RETENTION)]
        keep: usize,
    },
    /// Print a component summary from the manifest.
    Manifest,
    /// Emit generated views (ARCHITECTURE.md + SURFACE.md) from the manifest
    /// and the committed map — committed and diffable; never hand-edit what
    /// render owns.
    /// Draft a manifest proposal from the existing tree (printed; you prune).
    Adopt,
    /// Parity fan-out templates: declared shapes for recurring change types.
    Templates {
        #[command(subcommand)]
        cmd: TemplatesCmd,
    },
    /// Render collaboration notes (review checklist / release draft).
    Notes {
        #[command(subcommand)]
        cmd: NotesCmd,
    },
    /// The debt register: deliberate deferrals with a review trigger.
    Deferred {
        #[command(subcommand)]
        cmd: DeferredCmd,
    },
    /// The glossary: agent vocabulary -> harna vocabulary (0.3.26).
    Terms {
        #[command(subcommand)]
        cmd: TermsCmd,
    },
    /// The decline register: deliberate product refusals (0.3.27).
    NonGoal {
        #[command(subcommand)]
        cmd: NonGoalCmd,
    },
    /// The dead-ends register: tried-and-failed records with verdicts (0.3.27).
    Lesson {
        #[command(subcommand)]
        cmd: LessonCmd,
    },
    /// The roadmap: declared milestones, never generated (0.3.28).
    Roadmap {
        #[command(subcommand)]
        cmd: RoadmapCmd,
    },
    /// Print the generated dependency graph (DOT or Mermaid).
    Graph {
        /// Output format: dot | mermaid.
        #[arg(long, default_value = "mermaid")]
        format: String,
    },
    Render {
        /// Verify the committed views are current instead of writing (CI).
        #[arg(long)]
        check: bool,
        /// Render only the public-surface view (SURFACE.md, 0.3.5). The
        /// default regenerates every generated view the map can produce.
        #[arg(long)]
        surface: bool,
    },
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
        /// Environment the session declares ("dev", "prod", ...): env-tagged
        /// rules for other environments stay out of enforcement.
        #[arg(long)]
        env: Option<String>,
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
    /// Where does this symbol live? Definitions + referencing modules.
    Whereis {
        /// Symbol name, optionally module-qualified (e.g. module::Symbol).
        symbol: String,
        #[arg(long)]
        json: bool,
    },
    /// List registered tests mapped to runnable `cargo test` commands (0.3.11).
    Tests {
        /// Only this component (by manifest component name).
        #[arg(long)]
        component: Option<String>,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Measure the import graph (fan-in/fan-out, boundary pressure,
    /// coupling outliers) and print the composite health grade (0.3.12).
    Metrics {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
        /// Persist the current grade as a timestamped snapshot (0.3.13).
        #[arg(long)]
        record: bool,
        /// Show the recorded trend with deltas (regressions flagged).
        #[arg(long)]
        trend: bool,
    },
    /// Print the codebase's own hierarchy from the committed digest.
    Tree,
    /// Installation self-audit: schema, data files, git, hook, agent wiring.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Release gate: validation errors + doctor failures + open parity
    /// debts + map freshness compose into one PASS/FAIL (0.3.29). Exits 2
    /// on FAIL. The test suite is NOT run here (harna never executes
    /// project code) — run it yourself, then release.
    Preflight {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
        /// Max files to check (safety cap).
        #[arg(long, default_value_t = 10_000)]
        max_files: usize,
    },
    /// Truthfulness reconciliation: the mechanically verifiable claims
    /// (package version, changelog head, roadmap NOW/DONEs, parity
    /// silence) cross-checked against the state (0.3.30). DRIFTs name
    /// both sides; exits 2 on drift.
    ValidateIntent {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// The advisory ledger: lock away accepted Info findings with a
    /// reason, re-arm them, or list what is locked (0.3.31). Locked
    /// advisories render as one-line receipts and stay out of
    /// strict-mode gating; errors can never be acknowledged.
    Acknowledge {
        #[command(subcommand)]
        cmd: AckCmd,
    },
    /// Split planner: map an oversized file's top-level items and print
    /// HOW to split it (dispatch-split or prefix-cluster) plus the
    /// manifest sketch (0.3.31). Application stays manual — the cut
    /// lines are visible, the judgment is yours.
    Decompose {
        /// File to plan, repo-relative, e.g. crates/x/src/main.rs.
        file: String,
    },
    /// Placement briefing for an idea, before code exists (PLAN step).
    Place {
        /// The idea in plain words, e.g. "timezone picker data".
        idea: String,
    },
    /// Full pre-code briefing: placement + decision context (supersession-
    /// aware) + rule bindings + dependency fan-out preview + test obligations.
    Plan {
        /// The idea in plain words, e.g. "timezone picker data".
        idea: String,
    },
    /// The burst journal: record the current-burst state before compaction.
    /// No arguments shows the current handoff; the next session resumes from
    /// onboarding/status (which carry it).
    Handoff {
        /// What this burst was doing (overwrites any previous handoff).
        #[arg(long)]
        summary: Option<String>,
        /// Declared next step, in order; repeatable with --next.
        #[arg(long = "next")]
        next: Vec<String>,
        /// The burst is resumed: remove the recorded handoff.
        #[arg(long)]
        clear: bool,
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
        /// Role for the container (base/engine/gateway/orchestration-async/
        /// orchestration-sync/transit/interface/context) — stamped on it.
        #[arg(long)]
        role: Option<String>,
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
    /// Rebuild .harna/digest/ shards by scanning the repo.
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
        /// Git ref whose committed digest is the baseline (shards or legacy monolith).
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
        Command::Init { from } => {
            store.init()?;
            let mut profile_note = String::new();
            if let Some(src) = from {
                let applied = store.apply_profile(&src)?;
                if applied.manifest {
                    profile_note.push_str(&format!(
                        "\nmanifest skeleton copied from {} — its paths point at the OTHER tree: prune and re-point them (edit the file or `harna scaffold`)",
                        src.display()
                    ));
                }
                if applied.rules {
                    profile_note.push_str(&format!(
                        "\nrules copied from {} — scopes that miss this tree simply apply nowhere",
                        src.display()
                    ));
                }
                if !applied.manifest && !applied.rules {
                    anyhow::bail!("no manifest.toml / rules.toml found under {}", src.display());
                }
            }
            let agents = store.ensure_agents_md()?;
            println!("initialized {}", store.manifest_path().parent().unwrap().display());
            if agents {
                println!("AGENTS.md written at repo root — agents now know the harna contract");
            } else {
                println!("AGENTS.md already covers harna — left untreated");
            }
            if !profile_note.is_empty() {
                println!("{profile_note}");
            }
            println!("next: `harna scan`, add rules, and wire the harna MCP server");
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
        Command::AddRule { id, text, rationale, scopes, env, check, severity, pattern, limit } => {
            let mut rules = store.load_rules()?;
            if rules.rules.iter().any(|r| r.id == id) {
                anyhow::bail!("rule id '{id}' already exists");
            }
            // Both edges read the same check registry (checks.toml via
            // enforcement::checks) — no third spelling of the semantics.
            if let Some(c) = &check {
                if !harna_core::enforcement::checks::known(c) {
                    anyhow::bail!(
                        "unknown check `{c}` — known: {}",
                        harna_core::enforcement::checks::known_ids_listed()
                    );
                }
            }
            if let Some(s) = &severity {
                if check.is_none() {
                    anyhow::bail!("--severity overrides a bound check — add --check first");
                }
                if harna_core::enforcement::checks::parse_severity(s).is_none() {
                    anyhow::bail!(
                        "unknown severity `{s}` — valid: {}",
                        harna_core::enforcement::checks::VALID_SEVERITIES
                    );
                }
            }
            if let Some(p) = &pattern {
                if let Some(check) = check.as_ref() {
                    if check != "pattern" {
                        anyhow::bail!(
                            "--pattern implies the `pattern` check — the explicit --check `{check}` disagrees"
                        );
                    }
                }
                if let Err(e) = harna_core::enforcement::checks::compile_pattern(p) {
                    anyhow::bail!("--pattern is not a valid regex (`{p}`): {e}");
                }
            }
            if limit.is_some()
                && !matches!(check.as_deref(), Some("container-size") | Some("fan-out-cap"))
            {
                anyhow::bail!(
                    "--limit only names budgets — bind with --check container-size or --check fan-out-cap"
                );
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
                severity,
                pattern,
                limit,
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
        Command::LogDecision { title, body, supersedes } => {
            let path = store.save_decision(&title, &body, supersedes.as_deref())?;
            println!("decision saved: {}", path.display());
        }
        Command::Decisions { chain } => {
            let decisions = harna_core::schema::decisions::list(&store.decisions_dir())?;
            match chain {
                Some(q) => {
                    println!("{}", harna_core::schema::decisions::chain(&decisions, q.trim())?);
                }
                None => {
                    println!("{}", harna_core::schema::decisions::render(&decisions));
                }
            }
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
        Command::Migrate { dry } => {
            let reports = harna_core::schema::migrate::run(&root, &store, dry)?;
            for r in &reports {
                println!("{}", r.render());
            }
        }
        Command::Restore { name } => {
            let restored = harna_core::schema::migrate::restore(&store, &name)?;
            println!("{restored}");
        }
        Command::Archive { keep } => {
            // The three ledgers file their own dead weight; this prints it.
            let closed = harna_core::enforcement::parity::archive_closed(&store)?;
            let superseded = harna_core::schema::decisions::archive_superseded(
                &store.decisions_dir(),
                &store.archive_dir(),
            )?;
            let old = harna_core::composition::score::archive_old_snapshots(&store, keep)?;
            println!(
                "archived: {} closed parity event(s), {} superseded decision(s), {} old score snapshot(s)",
                closed.len(),
                superseded.len(),
                old.len()
            );
            for id in &closed {
                println!("  parity/{id}.toml");
            }
            for f in &superseded {
                println!("  decisions/{f}");
            }
            for n in &old {
                println!("  scores/{n}.toml");
            }
            if closed.is_empty() && superseded.is_empty() && old.is_empty() {
                println!("nothing to archive — the live surface is already lean");
            }
        }
        Command::Templates { cmd } => commands::templates::run(&store, &root, cmd)?,
        Command::Notes { cmd } => commands::notes::run(&store, &root, cmd)?,
        Command::Deferred { cmd } => commands::deferred::run(&store, &root, cmd)?,
        Command::Terms { cmd } => commands::terms::run(&store, &root, cmd)?,
        Command::NonGoal { cmd } => commands::non_goal::run(&store, &root, cmd)?,
        Command::Lesson { cmd } => commands::lesson::run(&store, &root, cmd)?,
        Command::Roadmap { cmd } => commands::roadmap::run(&store, &root, cmd)?,
        Command::Adopt => {
            // A missing .harna/ is not an error, it is the fresh start
            // adopt exists for; a present manifest that is non-empty is
            // the refusal. A malformed manifest all the way to the CLI.
            if root.join(".harna/manifest.toml").is_file() {
                let existing = store.load_manifest()?;
                if !existing.is_empty() {
                    anyhow::bail!(
                        "this repository already has a manifest ({} layers) — `harna adopt` drafts fresh starts; edit the manifest or use `harna scaffold` instead",
                        existing.layers.len()
                    );
                }
            }
            let d = harna_core::generation::adopt::draft(&root, 10_000)?;
            // Summary to stderr: redirecting stdout must yield clean,
            // paste-ready manifest TOML — a draft is data, not a report.
            eprintln!(
                "draft: {} module(s) across {} layer(s)/{} container(s); {} cross-crate edge(s) proposed as depends_on",
                d.module_count, d.layer_count, d.container_count, d.cross_crate_edges
            );
            println!("{}", harna_core::generation::adopt::render(&d)?);
        }
        Command::Graph { format } => {
            let digest = store.load_digest()?;
            let manifest = store.load_manifest()?;
            let view = harna_core::generation::graph::build(&root, &manifest, &digest);
            let text = match format.as_str() {
                "dot" => harna_core::generation::graph::dot(&view),
                "mermaid" => harna_core::generation::graph::mermaid(&view),
                other => anyhow::bail!("unknown graph format `{other}` — use dot or mermaid"),
            };
            // The summary goes to stderr: redirecting stdout must yield a
            // clean, import-ready file — an export is data, not a report.
            eprintln!(
                "{} nodes · {} edges",
                view.nodes.len(),
                view.edges.len()
            );
            println!("{text}");
        }
        Command::Render { check, surface } => {
            let manifest = store.load_manifest()?;
            if manifest.is_empty() {
                anyhow::bail!("manifest has no layers — nothing to render yet");
            }
            if surface {
                // The surface view reads the committed map (inventory) too.
                let digest = store.load_digest()?;
                let stale = harna_core::generation::render::surface_is_stale(&root, &manifest, &digest)?;
                if stale {
                    if check {
                        anyhow::bail!(
                            "SURFACE.md is stale — run `harna render --surface` and commit the view"
                        );
                    }
                    let path = harna_core::generation::render::write_surface(&root, &manifest, &digest)?;
                    println!("rendered {}", path.display());
                } else {
                    if check {
                        println!("surface view is current");
                    } else {
                        println!("SURFACE.md already current (nothing to write)");
                    }
                }
                return Ok(());
            }
            let digest_ok = store.load_digest().ok();
            let arch_stale = harna_core::generation::render::is_stale(&root, &manifest)?;
            let surface_state = digest_ok.as_ref().map(|d| {
                harna_core::generation::render::surface_is_stale(&root, &manifest, d).unwrap_or(true)
            });
            let stale = arch_stale || surface_state.unwrap_or(false);
            if stale {
                if check {
                    anyhow::bail!(
                        "generated views are stale — run `harna render` and commit the views"
                    );
                }
                let path = harna_core::generation::render::write(&root, &manifest)?;
                println!("rendered {}", path.display());
                if let (Some(_d), Some(true)) = (digest_ok.as_ref(), surface_state) {
                    let path = harna_core::generation::render::write_surface(&root, &manifest, digest_ok.as_ref().unwrap())?;
                    println!("rendered {}", path.display());
                }
            } else if check {
                println!("views are current");
            } else {
                println!("generated views already current (nothing to write)");
                if surface_state.is_none() {
                    eprintln!(
                        "note: no committed code map — the surface view (SURFACE.md) needs one; run `harna scan`"
                    );
                }
            }
        }
        Command::Validate { changed, max_files, strict, json, env } => {
            let files = if changed {
                let changed_files = harna_core::enforcement::validate::changed_rust_files(&root)?;
                // No changed .rs files still runs the manifest-level gates
                // (parity debts, manifest paths, decisions, roadmap, data
                // health): a manifest-only edit must not pass unchecked.
                // Progress goes to stderr so `--json` stays parseable.
                if changed_files.is_empty() {
                    eprintln!("no changed .rs files — running manifest-level checks only");
                } else {
                    eprintln!("checking {} changed file(s)…", changed_files.len());
                }
                Some(changed_files)
            } else {
                None
            };
            let violations =
                harna_core::enforcement::validate::run_on(&root, &store, max_files, files, env.as_deref())?;
            // Advisory ledger (0.3.31): locked-away Infos ride as
            // receipts, never as blocks — and stay out of strict gating.
            let ledger = store.load_acknowledged().unwrap_or_default();
            let (violations, acked) =
                harna_core::enforcement::validate::acknowledge::partition(&ledger, violations);
            let errors = violations.iter().filter(|v| v.severity == harna_core::enforcement::validate::Severity::Error).count();
            let infos = violations.len() - errors;
            let gate = errors > 0 || (strict && infos > 0);

            if json {
                let report = serde_json::json!({
                    "violations": violations,
                    "acknowledged": acked,
                    "clean": violations.is_empty(),
                    "counts": { "error": errors, "info": infos, "acknowledged": acked.len() },
                    "blocks": gate,
                });
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else if violations.is_empty() && acked.is_empty() {
                println!("no violations found");
            } else {
                for v in &violations {
                    println!("{}", v.render());
                }
                for a in &acked {
                    println!("ack [{}] {} — {}", a.violation.check, a.violation.file, a.reason);
                }
                println!("\n{} violation(s) ({e} error, {i} info, {a} acknowledged)", violations.len(), e = errors, i = infos, a = acked.len());
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
        Command::Plan { idea } => {
            print!("{}", harna_core::composition::plan::build(&store, &idea)?);
        }
        Command::Handoff { summary, next, clear } => {
            if clear {
                if harna_core::schema::handoff::clear(&store)? {
                    println!("handoff cleared (burst resumed)");
                } else {
                    println!("no handoff on disk");
                }
                return Ok(());
            }
            match summary {
                Some(s) => {
                    let open_parity = harna_core::enforcement::parity::open_debt_count(&store)
                        .unwrap_or(0);
                    let path = harna_core::schema::handoff::save(&store, &s, &next, open_parity)?;
                    println!("handoff recorded: {}", path.display());
                }
                None => match harna_core::schema::handoff::load(&store) {
                    Some(h) => print!("{}", harna_core::schema::handoff::render(&h)),
                    None => println!("no handoff recorded — none needed if the burst is one plain commit"),
                },
            }
        }
        Command::Doctor { json } => {
            let report = harna_core::composition::doctor::build(&store, &root)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", report.render());
            }
            if report.fail_count() > 0 {
                eprintln!(
                    "harna: {} installation issue(s) block work — fix the FAIL items",
                    report.fail_count()
                );
                {
                    use std::io::Write as _;
                    let _ = std::io::stdout().flush();
                }
                process::exit(1);
            }
        }
        Command::Preflight { json, max_files } => {
            let r = harna_core::composition::preflight::build(&store, &root, max_files)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", r.render());
            }
            if !r.pass {
                // Gate precedent is `validate`'s exit 2: hooks and CI gate on this.
                eprintln!("harna: preflight FAIL — not releasable until every gate holds");
                {
                    use std::io::Write as _;
                    let _ = std::io::stdout().flush();
                }
                process::exit(2);
            }
        }
        Command::ValidateIntent { json } => {
            let r = harna_core::composition::intent::build(&store, &root)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", r.render());
            }
            if !r.pass {
                // Same gate precedent: drift is normal mid-burst, but a
                // claimed coherence with drift is not releasable.
                eprintln!("harna: validate-intent found DRIFT(s) — reconcile before claiming coherence");
                {
                    use std::io::Write as _;
                    let _ = std::io::stdout().flush();
                }
                process::exit(2);
            }
        }
        Command::Acknowledge { cmd } => commands::ack::run(&store, &root, cmd)?,
        Command::Decompose { file } => {
            let src = std::fs::read_to_string(root.join(&file))
                .map_err(|_| anyhow::anyhow!("cannot read `{file}` — repo-relative path from the repo root"))?;
            let p = harna_core::composition::decompose::plan(&file, &src);
            print!("{}", p.render());
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
        Command::Whereis { symbol, json } => {
            let r = harna_core::search::whereis::build(&store, &symbol)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", r.render());
            }
        }
        Command::Tests { component, json } => {
            let manifest = store.load_manifest()?;
            let mut targets =
                harna_core::enforcement::validate::test_coverage::runnable(&root, &manifest);
            if let Some(name) = component {
                if !targets.iter().any(|t| t.component == name) {
                    anyhow::bail!("unknown component `{name}` — no registered tests under that name");
                }
                targets.retain(|t| t.component == name);
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&targets)?);
            } else {
                print!("{}", harna_core::enforcement::validate::test_coverage::render_targets(&targets));
            }
        }
        Command::Metrics { json, record, trend } => {
            if json {
                let r = harna_core::composition::score::full(&store)?;
                println!("{}", serde_json::to_string_pretty(&r)?);
                return Ok(());
            }
            let mut out = String::new();
            if record {
                let s = harna_core::composition::score::build(&store)?;
                let path = harna_core::composition::score::record(&store, &s)?;
                out.push_str(&format!("recorded {}\n", path.display()));
            }
            if trend || !record {
                if trend {
                    let snaps = harna_core::composition::score::list_snapshots(&store)?;
                    let rows = harna_core::composition::score::trend(&snaps);
                    out.push_str(&harna_core::composition::score::render_trend(&rows));
                } else {
                    // One scan, many consumers (0.3.17): metrics, score and
                    // hotspots share the single map read inside `full`.
                    let r = harna_core::composition::score::full(&store)?;
                    out.push_str(&r.metrics.render());
                    out.push_str(&r.score.render());
                    out.push_str(&r.hotspots.render());
                }
            }
            print!("{out}");
        }
        Command::Scaffold { layer, container, name, kind, path, description, role } => {
            let req = harna_core::generation::scaffold::ScaffoldRequest {
                layer,
                container,
                name,
                kind: kind.to_core(),
                path,
                description: description.unwrap_or_default(),
                role,
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
            // Sharded per container (0.3.15): the manifest maps modules home.
            let manifest = store.load_manifest().unwrap_or_default();
            store.save_digest(&manifest, &d)?;
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
            // Hash-verified reuse (0.3.17): the working-tree map rebuilds
            // from the committed map where the body hash proves a file
            // unchanged — same diff, a fraction of the parsing. The
            // extractor gate inside build_incremental keeps old maps from
            // feeding stale edges.
            let previous = store.load_digest().ok();
            let (fresh, _, _) = digest::build_incremental(&root, max_files, previous.as_ref(), false)?;
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
        Command::Parity { cmd } => commands::parity::run(&store, &root, cmd)?,
    }
    Ok(())
}
