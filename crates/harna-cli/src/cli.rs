//! The command line surface: every subcommand, its arguments and help
//! text (clap). `main.rs` dispatches; this file only declares.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::commands::manifest::{ContainerCmd, LayerCmd, SlotCmd, TestKindCmd};
use crate::commands::registers::{
    command::CommandCmd,
    ledgers::{ack::AckCmd, baseline::BaselineCmd, deferred::DeferredCmd},
    lesson::LessonCmd,
    non_goal::NonGoalCmd,
    quality::QualityCmd,
    roadmap::RoadmapCmd,
    templates::TemplatesCmd,
    terms::TermsCmd,
};
use crate::commands::{hooks::HooksCmd, notes::NotesCmd, parity::ParityCmd};

/// CLI-side mirror of `scaffold::ScaffoldKind` so harna-core stays lean.
#[derive(Clone, Copy, clap::ValueEnum)]
pub(crate) enum KindArg {
    Primitive,
    Compound,
    Data,
}

impl KindArg {
    pub(crate) fn to_core(self) -> harna_core::generation::structure::scaffold::ScaffoldKind {
        match self {
            KindArg::Primitive => {
                harna_core::generation::structure::scaffold::ScaffoldKind::Primitive
            }
            KindArg::Compound => {
                harna_core::generation::structure::scaffold::ScaffoldKind::Compound
            }
            KindArg::Data => harna_core::generation::structure::scaffold::ScaffoldKind::Data,
        }
    }
}

#[derive(Parser)]
#[command(
    name = "harna",
    version,
    about = "Persistent agent context for Rust codebases — instructions become data in .harna/"
)]
pub(crate) struct Cli {
    /// Repository root (defaults to current directory).
    #[arg(long, global = true)]
    pub(crate) root: Option<PathBuf>,

    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Initialize .harna/ in the repository.
    Init {
        /// Start from another repo's rules + manifest skeleton: path to
        /// its root (reads .harna/) or to a .harna/ directory itself.
        /// Copied verbatim as a starting point to prune.
        #[arg(long)]
        from: Option<PathBuf>,
        /// Opt AGENTS.md in: add harna's managed block (rendered from
        /// .harna/). Hand-written prose is never rewritten; without this
        /// flag init leaves AGENTS.md alone.
        #[arg(long)]
        agents: bool,
        /// Start from a profile: a shipped name (`harna profile list`), a
        /// folder path, or a folder under ~/.harna/profiles (0.4.2).
        #[arg(long, conflicts_with = "from")]
        profile: Option<String>,
        /// With --profile/--from: replace a manifest/rules that already
        /// hold this project's data (otherwise init refuses).
        #[arg(long)]
        force: bool,
    },
    /// Starting shapes as data: list profiles, show what one declares.
    Profile {
        #[command(subcommand)]
        cmd: crate::commands::profile::ProfileCmd,
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
        /// Budget for a budget check (`harna checks` lists which take one):
        /// components, imports, lines, files.
        #[arg(long)]
        limit: Option<u32>,
        /// Argument for a list-taking check (repeatable): banned paths
        /// for banned-api, the files of a sync-group, …
        #[arg(long = "value")]
        values: Vec<String>,
        /// The command that proves the rule, run by harna's CI workflow
        /// (generated: the regenerate-and-diff check).
        #[arg(long)]
        command: Option<String>,
        /// Scope by meaning: a regex over declarations; the rule applies to
        /// every file declaring a match (\bMoney\b), within --scope if set.
        #[arg(long)]
        scope_signature: Option<String>,
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
        #[arg(long, default_value_t = harna_core::composition::gates::score::SNAPSHOT_RETENTION)]
        keep: usize,
    },
    /// harna's toolbox: what each tool does, when to reach for it, how.
    /// Every tool is optional; `place`/`plan` offer the ones that fit.
    Tools,
    /// List the checks a rule can bind (`--check`): id, default severity,
    /// the arguments each reads, and what it enforces.
    Checks,
    /// Print a component summary from the manifest.
    Manifest,
    /// Draft a manifest from the tree, or refresh an existing one (printed; you prune).
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
    /// Long-term intent (0.4.0): what the software must hold over its life
    /// — scale, latency, durability, privacy, connectivity — as data.
    Quality {
        #[command(subcommand)]
        cmd: QualityCmd,
    },
    /// The dead-ends register: tried-and-failed records with verdicts (0.3.27).
    Lesson {
        #[command(subcommand)]
        cmd: LessonCmd,
    },
    /// Edit a layer in the manifest: depends_on, role (0.3.42).
    Layer {
        #[command(subcommand)]
        cmd: LayerCmd,
    },
    /// Test kinds (0.4.7): where each kind of test lives and which layers
    /// it may reach; declaring any holds tests and dev-dependencies to it.
    TestKind {
        #[command(subcommand)]
        cmd: TestKindCmd,
    },
    /// Planned components (0.4.1): declare a slot before its code; scaffold
    /// with its name fills it.
    Slot {
        #[command(subcommand)]
        cmd: SlotCmd,
    },
    /// Edit a container in the manifest: rename, path, gateway, role (0.3.42).
    Container {
        #[command(subcommand)]
        cmd: ContainerCmd,
    },
    /// The ratchet ledger: today's errors frozen per (check, file), allowed to fall, never to grow (0.3.44).
    Baseline {
        #[command(subcommand)]
        cmd: BaselineCmd,
    },
    /// The repo's own commands: what to run, what it proves, where (0.3.46).
    #[command(name = "command")]
    Commands {
        #[command(subcommand)]
        cmd: CommandCmd,
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
    /// Emit generated views (ARCHITECTURE.md + SURFACE.md) from the manifest
    /// and the committed map — committed and diffable; never hand-edit what
    /// render owns. Also fills the harna block of agent instruction files.
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
    /// Install harna's own enforcement — git pre-commit, Claude Code
    /// hooks, CI — so the gates run without anyone remembering (0.3.36).
    Hooks {
        #[command(subcommand)]
        cmd: HooksCmd,
    },
    /// Answer an agent-runtime hook: session-start | post-edit | stop
    /// (what the installed Claude Code hooks call; exit code is the verdict).
    Hook { event: String },
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
        /// Component name in kebab-case, e.g. user-repo. Repeatable: one
        /// call scaffolds several components into the same container.
        #[arg(long = "name", required = true)]
        names: Vec<String>,
        /// primitive = leaf; compound = folder with mod.rs + starter child.
        #[arg(long, value_enum)]
        kind: KindArg,
        /// Explicit path; with several --name, the folder they go in.
        /// Defaults to the container's path (or src/<layer>/<container>) + /<name>.
        #[arg(long)]
        path: Option<String>,
        /// One-line description embedded in the generated files.
        #[arg(long)]
        description: Option<String>,
        /// Role for the container (base/engine/gateway/orchestration-async/
        /// orchestration-sync/transit/interface/context) — stamped on it.
        #[arg(long)]
        role: Option<String>,
        /// Write the doc header only, no starter stub (for code you write yourself).
        #[arg(long)]
        empty: bool,
        /// Create the declared container's crate first: lib or bin
        /// (Cargo.toml, workspace member, src/lib.rs or src/main.rs).
        #[arg(long, value_name = "lib|bin")]
        new_crate: Option<String>,
    },
    /// Move a file or folder and keep it linked (0.3.39): manifest paths,
    /// tests and rule scopes relinked, `mod` declarations moved, `use`
    /// paths rewritten across the workspace.
    Move {
        /// Repo-relative source, e.g. crates/x/src/schema/terms_data.rs.
        from: String,
        /// Repo-relative destination, e.g. crates/x/src/schema/registers/terms.rs.
        to: String,
    },
    /// Scaffold a corridor (transiter): the one named, thin hand-off between
    /// two layers — a command bus, an event channel, an API facade.
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
