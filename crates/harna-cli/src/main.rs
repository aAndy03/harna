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

#[derive(Subcommand)]
enum TemplatesCmd {
    /// Add or replace a template (same id overwrites).
    Add {
        /// Stable id, e.g. schema-shard-change.
        id: String,
        /// Module-path prefix this change type touches (empty = everywhere).
        #[arg(long, default_value = "")]
        scope: String,
        /// The fan-out ceiling an event over this scope shouldn't walk past.
        #[arg(long)]
        max_targets: usize,
        /// Why this shape is declared.
        #[arg(long)]
        rationale: Option<String>,
    },
    /// Remove a template by id.
    Remove { id: String },
    /// List the declared templates.
    List,
}

#[derive(Subcommand)]
enum NotesCmd {
    /// A review checklist: an event's fan-out (or the diff from a ref),
    /// the scoped rules and the registered tests over the changed paths.
    Review {
        /// Parity event id; omit to review against `--from`'s diff.
        id: Option<String>,
        /// Git ref to diff against (default HEAD) when no event is given.
        #[arg(long)]
        from: Option<String>,
    },
    /// A release/PR note draft: decisions + parity events since a ref.
    Release {
        /// Git ref the draft covers up to (default HEAD).
        #[arg(long, default_value = "HEAD")]
        from: String,
    },
}

#[derive(Subcommand)]
enum TermsCmd {
    /// Add or replace a term (same word overwrites).
    Add {
        /// harna's canonical word, e.g. engine.
        word: String,
        /// The meaning as harna defines it.
        #[arg(long)]
        means: String,
        /// Agent vocabulary mapping to this word (repeatable).
        #[arg(long = "alias")]
        aliases: Vec<String>,
    },
    /// Remove a term by word.
    Remove { word: String },
    /// List the glossary.
    List,
}

#[derive(Subcommand)]
enum NonGoalCmd {
    /// Add or replace a refusal (same id overwrites).
    Add {
        /// Stable id, e.g. no-remote-state.
        id: String,
        /// What is refused, in imperative form.
        #[arg(long)]
        refusal: String,
        /// Why the refusal was made — the supersession briefing's evidence.
        #[arg(long)]
        reason: String,
    },
    /// Remove a refusal by id (record why in the decision log).
    Remove { id: String },
    /// List the refusals in force.
    List,
}

#[derive(Subcommand)]
enum LessonCmd {
    /// Add or replace a tried-and-failed record (same id overwrites).
    Add {
        /// Stable id, e.g. ps-roundtrip-source-files.
        id: String,
        /// The claim — what was attempted, in the direction of the failure.
        #[arg(long)]
        claim: String,
        /// The verdict — why it did not work out.
        #[arg(long)]
        verdict: String,
        /// Evidence pointers: decisions, commits, events (repeatable).
        #[arg(long = "evidence")]
        evidence: Vec<String>,
    },
    /// Remove a recorded dead end by id.
    Remove { id: String },
    /// List the recorded dead ends.
    List,
}

#[derive(Subcommand)]
enum RoadmapCmd {
    /// Add or replace a milestone (same id overwrites).
    Add {
        /// Stable id, e.g. 0.3.28 or phase-h.
        id: String,
        /// What the milestone delivers, in one line.
        #[arg(long)]
        title: String,
        /// One of declared|current|done|dropped (default declared).
        #[arg(long, default_value = "declared")]
        state: String,
        /// Optional longer goal statement.
        #[arg(long)]
        notes: Option<String>,
    },
    /// Move a milestone's state (the whole point of the roadmap: NOW moves).
    Set {
        /// Milestone id.
        id: String,
        /// One of declared|current|done|dropped.
        state: String,
    },
    /// Remove a milestone by id (say why in the decision log if it's a drop).
    Remove { id: String },
    /// List the declared path.
    List,
}

#[derive(Subcommand)]
enum DeferredCmd {
    /// Add or replace an entry (same id overwrites).
    Add {
        /// Stable id, e.g. fixture-builder-dedup.
        id: String,
        /// What collecting this deferral would touch (component name,
        /// module or container path).
        #[arg(long)]
        component: String,
        /// Why this is deliberately deferred, not an accident.
        #[arg(long)]
        reason: String,
        /// When to revisit: an ISO date (2027-01-15) or a version the
        /// tool must reach (0.4.0).
        #[arg(long)]
        review: String,
    },
    /// Remove an entry (it was collected, folded into a version, or the
    /// ask it guarded against never came).
    Remove { id: String },
    /// List the open deferrals, overdue ones flagged.
    List,
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
        Command::Templates { cmd } => match cmd {
            TemplatesCmd::Add { id, scope, max_targets, rationale } => {
                let mut t = store.load_templates()?;
                t.templates.retain(|x| x.id != id);
                t.templates.push(harna_core::schema::parity_data::ParityTemplate {
                    id: id.clone(),
                    scope: scope.clone(),
                    max_targets,
                    rationale: rationale.unwrap_or_default(),
                });
                t.templates.sort_by(|a, b| a.id.cmp(&b.id));
                store.save_templates(&t)?;
                let shown_scope = if scope.is_empty() { "*".to_string() } else { scope };
                println!("template `{id}` declared — matching events over `{shown_scope}` notice at {max_targets}");
            }
            TemplatesCmd::Remove { id } => {
                let mut t = store.load_templates()?;
                let before = t.templates.len();
                t.templates.retain(|x| x.id != id);
                store.save_templates(&t)?;
                if t.templates.len() == before {
                    anyhow::bail!("no template `{id}` declared");
                }
                println!("template `{id}` removed");
            }
            TemplatesCmd::List => {
                let t = store.load_templates()?;
                if t.templates.is_empty() {
                    println!("no templates declared — `harna templates add <id> --scope <path> --max-targets <n>` declares a recurring change type's fan-out shape");
                }
                for x in &t.templates {
                    println!(
                        "- [{}] scope `{}` ceilings {} target(s){}",
                        x.id,
                        if x.scope.is_empty() { "*".to_string() } else { x.scope.clone() },
                        x.max_targets,
                        if x.rationale.is_empty() { String::new() } else { format!(" — why: {}", x.rationale) }
                    );
                }
            }
        },
        Command::Notes { cmd } => match cmd {
            NotesCmd::Review { id, from } => {
                if id.is_none() && from.is_none() {
                    anyhow::bail!("give a parity event id, or --from <ref> to review a range");
                }
                let text = harna_core::composition::notes::review(&store, id.as_deref(), from.as_deref())?;
                println!("{text}");
            }
            NotesCmd::Release { from } => {
                let text = harna_core::composition::notes::release(&store, &from)?;
                println!("{text}");
            }
        },
        Command::Deferred { cmd } => match cmd {
            DeferredCmd::Add { id, component, reason, review } => {
                let kind_hint = match harna_core::schema::deferred_data::review_kind(&review) {
                    harna_core::schema::deferred_data::ReviewKind::IsoDate => "that date".to_string(),
                    harna_core::schema::deferred_data::ReviewKind::Version => {
                        format!("version {review}")
                    }
                    harna_core::schema::deferred_data::ReviewKind::Milestone => format!(
                        "milestone `{review}` is done (declare it first: `harna roadmap add {review} --title \"..\"`; undeclared ids are checks' ghosts)"
                    ),
                };
                let mut r = store.load_deferred()?;
                r.upsert(harna_core::schema::deferred_data::DeferredEntry {
                    id: id.clone(),
                    component,
                    reason,
                    review: review.clone(),
                    recorded: chrono::Local::now().format("%Y-%m-%d").to_string(),
                });
                store.save_deferred(&r)?;
                println!(
                    "deferred `{id}` registered — review when {kind_hint} arrives (harna status surfaces it, overdue reviews cost a debt)",
                );
            }
            DeferredCmd::Remove { id } => {
                let mut r = store.load_deferred()?;
                if !r.remove(&id) {
                    anyhow::bail!("no deferred entry `{id}` registered");
                }
                store.save_deferred(&r)?;
                println!("deferred `{id}` removed (the promise is closed — say why in the decision log if it was collected)");
            }
            DeferredCmd::List => {
                let r = store.load_deferred()?;
                if r.entries.is_empty() {
                    println!("debt register empty — no deliberate deferrals declared (decisions may still carry prose deferrals; `harna deferred add` tracks them as reviewed data)");
                }
                let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                let roadmap = store.load_roadmap().unwrap_or_default();
                for e in &r.entries {
                    let kind = harna_core::schema::deferred_data::review_kind(&e.review);
                    let due = match kind {
                        harna_core::schema::deferred_data::ReviewKind::Milestone => {
                            harna_core::schema::deferred_data::milestone_due(
                                &e.review,
                                &|id| roadmap.reached(id),
                            )
                        }
                        _ => harna_core::schema::deferred_data::review_due(
                            &e.review,
                            env!("CARGO_PKG_VERSION"),
                            &today,
                        ),
                    };
                    println!(
                        "- [{}] `{}` review {}{}{}",
                        e.id,
                        e.component,
                        e.review,
                        if due { " — OVERDUE (costs a debt)" } else if kind == harna_core::schema::deferred_data::ReviewKind::Milestone { " (milestone)" } else { "" },
                        if e.recorded.is_empty() { String::new() } else { format!(" (recorded {})", e.recorded) },
                    );
                    println!("  why: {}", e.reason);
                }
            }
        },
        Command::Terms { cmd } => match cmd {
            TermsCmd::Add { word, means, aliases } => {
                let mut t = store.load_terms()?;
                t.terms.retain(|e| !e.word.eq_ignore_ascii_case(&word));
                t.terms.push(harna_core::schema::terms_data::Term {
                    word: word.clone(),
                    aliases: aliases.clone(),
                    means: means.clone(),
                });
                let collisions = t.alias_collisions();
                store.save_terms(&t)?;
                println!("term `{word}` recorded ({} total)", t.terms.len());
                if !collisions.is_empty() {
                    println!(
                        "  WATCH: {} alias collision(s) remain — {collisions:?} (doctor names them; one spelling, one meaning)",
                        collisions.len()
                    );
                }
            }
            TermsCmd::Remove { word } => {
                let mut t = store.load_terms()?;
                let before = t.terms.len();
                t.terms.retain(|e| !e.word.eq_ignore_ascii_case(&word));
                if t.terms.len() == before {
                    anyhow::bail!("no term `{word}` in the glossary");
                }
                store.save_terms(&t)?;
                println!("term `{word}` removed (git history keeps the record)");
            }
            TermsCmd::List => {
                let t = store.load_terms()?;
                if t.terms.is_empty() {
                    println!("glossary empty — harna's vocabulary lives in the tool docs until terms are recorded (`harna terms add <word> --means \"..\" [--alias ..]`)");
                }
                for e in &t.terms {
                    let aliases = if e.aliases.is_empty() {
                        String::new()
                    } else {
                        format!(" (also: {})", e.aliases.join(", "))
                    };
                    println!("  {}{} = {}", e.word, aliases, e.means);
                }
            }
        },
        Command::NonGoal { cmd } => match cmd {
            NonGoalCmd::Add { id, refusal, reason } => {
                let mut g = store.load_non_goals()?;
                g.upsert(harna_core::schema::non_goals_data::NonGoal {
                    id: id.clone(),
                    refusal,
                    reason,
                    recorded: chrono::Local::now().format("%Y-%m-%d").to_string(),
                });
                store.save_non_goals(&g)?;
                println!(
                    "refusal `{id}` in force ({}) — plan/place suppress matching proposals; superseding needs the user's agreement + `harna non-goal remove` + a logged decision",
                    g.goals.len()
                );
            }
            NonGoalCmd::Remove { id } => {
                let mut g = store.load_non_goals()?;
                if !g.remove(&id) {
                    anyhow::bail!("no refusal `{id}` recorded");
                }
                store.save_non_goals(&g)?;
                println!("refusal `{id}` removed — log WHY it was superseded (`harna log-decision`), or the removal itself is the betrayal risk");
            }
            NonGoalCmd::List => {
                let g = store.load_non_goals()?;
                if g.goals.is_empty() {
                    println!("no refusals recorded — non-goals are deliberate product decisions (\"we will not do X\"), kept in data via `harna non-goal add`");
                }
                for e in &g.goals {
                    let date = if e.recorded.is_empty() { String::new() } else { format!(" (recorded {})", e.recorded) };
                    println!("  - [{}] {}{}\n    why: {}", e.id, e.refusal, date, e.reason);
                }
            }
        },
        Command::Lesson { cmd } => match cmd {
            LessonCmd::Add { id, claim, verdict, evidence } => {
                let mut l = store.load_lessons()?;
                l.upsert(harna_core::schema::lessons_data::Lesson {
                    id: id.clone(),
                    claim,
                    verdict,
                    evidence,
                    recorded: chrono::Local::now().format("%Y-%m-%d").to_string(),
                });
                store.save_lessons(&l)?;
                println!(
                    "lesson `{id}` recorded ({}) — new sessions meet this dead end at plan/place before re-suggesting it",
                    l.lessons.len()
                );
            }
            LessonCmd::Remove { id } => {
                let mut l = store.load_lessons()?;
                if !l.remove(&id) {
                    anyhow::bail!("no lesson `{id}` recorded");
                }
                store.save_lessons(&l)?;
                println!("lesson `{id}` removed — say why the verdict no longer holds (decision log), otherwise it was not stale");
            }
            LessonCmd::List => {
                let l = store.load_lessons()?;
                if l.lessons.is_empty() {
                    println!("no dead ends recorded — tried-and-failed directions belong here as data (`harna lesson add`), so failure is owned once and not re-suggested");
                }
                for e in &l.lessons {
                    let date = if e.recorded.is_empty() { String::new() } else { format!(" (recorded {})", e.recorded) };
                    println!("  - [{}] {}{}", e.id, e.claim, date);
                    println!("    verdict: {}", e.verdict);
                    if !e.evidence.is_empty() {
                        println!("    evidence: {}", e.evidence.join("; "));
                    }
                }
            }
        },
        Command::Roadmap { cmd } => match cmd {
            RoadmapCmd::Add { id, title, state, notes } => {
                if !harna_core::schema::roadmap_data::Milestone::STATES.contains(&state.as_str()) {
                    anyhow::bail!(
                        "unknown state `{state}` — one of declared|current|done|dropped"
                    );
                }
                let mut r = store.load_roadmap()?;
                r.upsert(harna_core::schema::roadmap_data::Milestone {
                    id: id.clone(),
                    title,
                    state,
                    notes: notes.unwrap_or_default(),
                });
                store.save_roadmap(&r)?;
                println!(
                    "milestone `{id}` declared ({}) — the roadmap is never generated; coherence is checked, ownership is yours",
                    r.milestones.len()
                );
            }
            RoadmapCmd::Set { id, state } => {
                if !harna_core::schema::roadmap_data::Milestone::STATES.contains(&state.as_str()) {
                    anyhow::bail!(
                        "unknown state `{state}` — one of declared|current|done|dropped"
                    );
                }
                let mut r = store.load_roadmap()?;
                if !r.milestones.iter().any(|m| m.id.eq_ignore_ascii_case(&id)) {
                    anyhow::bail!("no milestone `{id}` declared");
                }
                for m in r.milestones.iter_mut() {
                    if m.id.eq_ignore_ascii_case(&id) {
                        m.state = state.clone();
                    }
                }
                store.save_roadmap(&r)?;
                println!(
                    "milestone `{id}` is now `{state}` — when a milestone lands `done`, milestone-keyed deferred reviews go due"
                );
            }
            RoadmapCmd::Remove { id } => {
                let mut r = store.load_roadmap()?;
                if !r.remove(&id) {
                    anyhow::bail!("no milestone `{id}` declared");
                }
                store.save_roadmap(&r)?;
                println!("milestone `{id}` removed — log why (decision log), or the path misrepresented itself");
            }
            RoadmapCmd::List => {
                let r = store.load_roadmap()?;
                if r.milestones.is_empty() {
                    println!("no milestones declared — a roadmap is declared data, never generated (`harna roadmap add <id> --title \"..\"`)");
                }
                for m in &r.milestones {
                    let notes = if m.notes.is_empty() { String::new() } else { format!("\n    notes: {}", m.notes) };
                    println!("  [{}] {} = {}{}", m.state, m.id, m.title, notes);
                }
            }
        },
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
            let violations =
                harna_core::enforcement::validate::run_on(&root, &store, max_files, files, env.as_deref())?;
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
