//! harna-mcp: an MCP stdio server exposing harna's memory to any agent.
//!
//! Tools:
//! - harna_onboarding   — the ~1k-token digest a fresh session starts with
//! - harna_rules        — rules for (or all) scope(s), token-efficient
//! - harna_add_rule     — persist a new rule (survives compaction)
//! - harna_log_decision — persist a decision to .harna/decisions/
//! - harna_scan         — rebuild the compressed code map
//!
//! Wire into any MCP client by running the `harna-mcp` binary over stdio.

use std::path::PathBuf;

use rmcp::{
    ServerHandler, handler::server::router::tool::ToolRouter,
    handler::server::tool::Parameters, tool, tool_handler, tool_router,
};
use std::future::Future;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use harna_core::search::digest;
use harna_core::composition::onboarding;
use harna_core::schema::rules::Rule;
use harna_core::schema::store::Store;

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct RulesRequest {
    /// Optional scope path to filter rules by, e.g. "src/engine".
    pub scope: Option<String>,
    /// Optional environment the session is working in ("dev", "prod", ...).
    /// Env-tagged rules are hidden unless their env matches.
    pub env: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct AddRuleRequest {
    /// Short stable id, e.g. "no-hardcoded-urls".
    pub id: String,
    /// The rule in imperative form.
    pub text: String,
    /// Why the rule exists.
    pub rationale: String,
    /// Scope paths this rule applies to; empty means global.
    #[serde(default)]
    pub scope: Vec<String>,
    /// Restrict this rule to one environment ("dev", "prod", ...); omit for all envs.
    pub env: Option<String>,
    /// Bind the rule to an executable check id (violations cite the rule).
    pub check: Option<String>,
    /// Override the bound check's severity ("error" | "info"); requires check.
    pub severity: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct RuleLifecycleRequest {
    /// Rule id.
    pub id: String,
    /// "deprecate" (visibly dead; add the reason) | "activate" (restore) | "remove".
    pub action: String,
    /// Why the rule is being retired (appended to its rationale).
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DecisionRequest {
    /// Short title of the decision.
    pub title: String,
    /// The decision body: context, choice, consequences.
    pub body: String,
    /// Optional: filename (in .harna/decisions/) of the decision this one
    /// supersedes. The old decision stays visible but marked dead.
    pub supersedes: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ScanRequest {
    /// Re-parse every respected file instead of reusing unchanged modules.
    pub full: Option<bool>,
    /// Max files to scan; defaults to 10_000.
    pub max_files: Option<u32>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ValidateRequest {
    /// Max files to check; defaults to 10_000.
    pub max_files: Option<u32>,
    /// Only check .rs files with uncommitted git changes. Use this after edits
    /// and in pre-commit hooks; omit for a full-repo check.
    pub changed_only: Option<bool>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DiffRequest {
    /// Git ref whose .harna/digest.json is the baseline. Defaults to HEAD.
    pub from: Option<String>,
    /// Max files to scan; defaults to 10_000.
    pub max_files: Option<u32>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DependentsRequest {
    /// Digest module path, crate-relative stem, or file name.
    pub module: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct PlaceRequest {
    /// The idea in plain words, e.g. "timezone picker data".
    pub idea: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct HandoffRequest {
    /// What this burst was doing — set it to record (overwrites the previous
    /// handoff). Omit (with `clear` also unset) to read the current handoff.
    pub summary: Option<String>,
    /// What the next session should pick up, in order.
    pub next_steps: Option<Vec<String>>,
    /// The burst is resumed: remove the recorded handoff.
    pub clear: Option<bool>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ParityShowRequest {
    /// Optional event id; omit to show all events.
    pub id: Option<String>,
    /// Only events with unresolved targets (debt view).
    pub open_only: Option<bool>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ParityOpenRequest {
    /// Event id, e.g. "change-store-api". Re-using an id = revision (invalidates old acks).
    pub id: String,
    /// Event kind (rule-change, manual, digest-diff...).
    pub kind: Option<String>,
    /// Git ref whose committed digest is the baseline. Defaults to HEAD.
    pub from: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ParityResolveRequest {
    /// Event id.
    pub id: String,
    /// Module path of the target (as listed in the event).
    pub path: String,
    /// "ack" (change landed here) | "noop" (nothing needed, say why).
    pub status: String,
    /// Why / what was done.
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ParityAutoRequest {
    /// Event id to re-verify; omit to sweep every event.
    pub id: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct EmptyRequest {}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DoctorRequest {}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ScaffoldRequest {
    /// Layer name (created if missing), e.g. "data-access".
    pub layer: String,
    /// Container name within the layer (created if missing).
    pub container: String,
    /// Component name in kebab-case, e.g. "user-repo".
    pub name: String,
    /// "primitive" = leaf; "compound" = folder with mod.rs + starter child.
    pub kind: String,
    /// Explicit path; defaults to src/<layer>/<container>/<name>.
    pub path: Option<String>,
    /// One-line description embedded in the generated files.
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct TransiterRequest {
    /// Layer the connection starts from.
    pub from: String,
    /// Layer the connection reaches.
    pub to: String,
    /// Optional custom name; defaults to transiter-<from>-to-<to>.
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HarnaTools {
    root: PathBuf,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl HarnaTools {
    pub fn new() -> anyhow::Result<Self> {
        // The repo root defaults to the launch directory; override with HARNA_ROOT.
        let root = match std::env::var("HARNA_ROOT") {
            Ok(v) => PathBuf::from(v),
            Err(_) => std::env::current_dir()?,
        };
        Ok(Self { root, tool_router: Self::tool_router() })
    }

    fn store(&self) -> anyhow::Result<Store> {
        Store::open(&self.root)
    }

    /// The onboarding digest a fresh agent session should start with.
    #[tool(description = "Get the onboarding digest: architecture overview, all active rules, and codebase map for this repository. Call this at the start of every session.")]
    async fn harna_onboarding(&self) -> Result<String, rmcp::ErrorData> {
        self.call(|store| {
            let onb = onboarding::build(&store)?;
            Ok(onb.text)
        })
    }

    /// Rules for one scope, or all rules when no scope is given.
    #[tool(description = "Get coding rules, optionally scoped to a path (e.g. 'src/engine'). Rules include rationale and origin so you understand them, not just obey them.")]
    async fn harna_rules(
        &self,
        Parameters(RulesRequest { scope, env }): Parameters<RulesRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            let rules = store.load_rules()?;
            Ok(rules.render_for_agent(scope.as_deref(), env.as_deref()))
        })
    }

    /// Persist a new rule; every future session inherits it.
    #[tool(description = "Record a coding rule with its rationale so future agent sessions inherit it automatically. Call when the user states a preference or a convention is agreed. Optionally bind the rule to an executable check id (violations of that check cite the rule) and set its severity (error|info), overriding the check's default.")]
    async fn harna_add_rule(
        &self,
        Parameters(AddRuleRequest { id, text, rationale, scope, env, check, severity }): Parameters<AddRuleRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            let mut rules = store.load_rules()?;
            if rules.rules.iter().any(|r| r.id == id) {
                anyhow::bail!("rule id '{id}' already exists");
            }
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
                    anyhow::bail!("severity overrides a bound check — pass check first");
                }
                if harna_core::enforcement::checks::parse_severity(s).is_none() {
                    anyhow::bail!(
                        "unknown severity `{s}` — valid: {}",
                        harna_core::enforcement::checks::VALID_SEVERITIES
                    );
                }
            }
            rules.rules.push(Rule {
                id: id.clone(),
                text,
                rationale,
                scope,
                tags: vec![],
                origin: format!("user via harna MCP, {}", today()),
                examples: vec![],
                env,
                check,
                severity,
                status: None,
            });
            store.save_rules(&rules)?;
            Ok(format!("rule '{id}' saved; total rules: {}", rules.rules.len()))
        })
    }

    /// Rule lifecycle: deprecate, activate, or remove a rule.
    #[tool(description = "Change a rule's lifecycle: 'deprecate' marks it visibly dead in rules.toml (agents stop following it; include the reason); 'activate' restores a deprecated rule; 'remove' deletes it outright (git history keeps the record).")]
    async fn harna_rule_lifecycle(
        &self,
        Parameters(RuleLifecycleRequest { id, action, reason }): Parameters<RuleLifecycleRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            let mut rules = store.load_rules()?;
            match action.as_str() {
                "deprecate" => {
                    rules.deprecate(&id, reason.as_deref().unwrap_or(""))?;
                    store.save_rules(&rules)?;
                    Ok(format!("rule '{id}' deprecated (visible in the file, no longer followed)"))
                }
                "activate" => {
                    rules.activate(&id)?;
                    store.save_rules(&rules)?;
                    Ok(format!("rule '{id}' re-activated"))
                }
                "remove" => {
                    rules.remove(&id)?;
                    store.save_rules(&rules)?;
                    Ok(format!("rule '{id}' removed (history lives in git)"))
                }
                other => anyhow::bail!("unknown action '{other}': deprecate | activate | remove"),
            }
        })
    }

    /// Persist a decision to .harna/decisions/ as ADR-style markdown.
    #[tool(description = "Record an architectural decision to .harna/decisions/ so future sessions know why something was built this way. Optionally, supersedes names an older decision file this one replaces — the old one stays referenced but visibly dead.")]
    async fn harna_log_decision(
        &self,
        Parameters(DecisionRequest { title, body, supersedes }): Parameters<DecisionRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            let path = store.save_decision(&title, &body, supersedes.as_deref())?;
            Ok(format!("decision saved to {}", path.display()))
        })
    }

    /// Rebuild the compressed code map; agents query this instead of rescanning.
    #[tool(description = "Rebuild the compressed codebase digest (signatures + inventory) into .harna/digest.json. Incremental: unchanged modules are reused from the previous map; set full=true to disable. Respects .gitignore and git excludes. Run after structurally significant changes.")]
    async fn harna_scan(
        &self,
        Parameters(ScanRequest { max_files, full }): Parameters<ScanRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            let previous = store.load_digest().ok();
            let (d, reused, rescanned) = digest::build_incremental(
                &self.root,
                max_files.unwrap_or(10_000) as usize,
                previous.as_ref(),
                full.unwrap_or(false),
            )?;
            store.save_digest(&d)?;
            Ok(format!(
                "digest: {} modules, {} signatures (reused {}, scanned {})",
                d.module_count(),
                d.signature_count(),
                reused,
                rescanned
            ))
        })
    }

    /// Check the codebase for conformance violations.
    #[tool(description = "Validate the codebase for conformance violations: hardcoded URLs, cross-layer imports, and manifest paths that no longer exist. Set changed_only=true after making code changes (checks only files with uncommitted git changes); omit it for a full-repo check.")]
    async fn harna_validate(
        &self,
        Parameters(ValidateRequest { max_files, changed_only }): Parameters<ValidateRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            let files = if changed_only.unwrap_or(false) {
                Some(harna_core::enforcement::validate::changed_rust_files(&self.root)?)
            } else {
                None
            };
            let violations = harna_core::enforcement::validate::run_on(&self.root, &store, max_files.unwrap_or(10_000) as usize, files)?;
            if violations.is_empty() {
                return Ok("no violations found — codebase conforms".to_string());
            }
            Ok(violations.iter().map(|v| v.render()).collect::<Vec<_>>().join("\n"))
        })
    }

    /// Upgrade .harna/ data files to this tool's schema version.
    #[tool(description = "Migrate .harna/ data files to the current schema version after updating harna. Backs up old files to .harna/backup/ and logs the migration as a decision. Safe to call repeatedly.")]
    async fn harna_migrate(&self) -> Result<String, rmcp::ErrorData> {
        self.call(|store| {
            let reports = harna_core::schema::migrate::run(&self.root, &store)?;
            Ok(reports.iter().map(|r| r.render()).collect::<Vec<_>>().join("\n"))
        })
    }

    /// Compare the working tree's code map against a committed digest.
    #[tool(description = "Structural diff of the code map against a git ref: which modules, pub signatures, and internal deps were added or removed. Call after a change burst to see exactly what moved before deciding what else must follow it.")]
    async fn harna_diff(
        &self,
        Parameters(DiffRequest { from, max_files }): Parameters<DiffRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |_store| {
            let fresh = digest::build(&self.root, max_files.unwrap_or(10_000) as usize)?;
            let baseline =
                harna_core::search::diff::digest_at_ref(&self.root, from.as_deref().unwrap_or("HEAD"))?;
            let mut d = harna_core::search::diff::diff(&baseline, &fresh);
            d.from = from.unwrap_or_else(|| "HEAD".into());
            d.to = "working-tree".into();
            Ok(d.render())
        })
    }

    /// Show the parity ledger (change-propagation debts).
    #[tool(description = "Show parity events: structural changes whose fan-out targets must be acknowledged (ack), recorded as nothing-needed (noop), or auto-verified before commits pass conformance. Open debts gate 'harna validate'. Set open_only=true for the debt view only.")]
    async fn harna_parity(
        &self,
        Parameters(ParityShowRequest { id, open_only }): Parameters<ParityShowRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            harna_core::enforcement::parity::show(&store, id.as_deref(), open_only.unwrap_or(false))
        })
    }

    /// Open a parity event: compute the fan-out of the current structural diff.
    #[tool(description = "Open a parity event for the current structural changes (diff vs a git ref): harna computes which modules must follow the change — changed modules and their importers. Re-using an id revises the event and invalidates old acceptances. Call this before committing a structural change.")]
    async fn harna_parity_open(
        &self,
        Parameters(ParityOpenRequest { id, kind, from }): Parameters<ParityOpenRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            let ev = harna_core::enforcement::parity::open(
                &self.root,
                &store,
                &id,
                kind.as_deref().unwrap_or("digest-diff"),
                "",
                from.as_deref().unwrap_or("HEAD"),
                10_000,
            )?;
            let open = ev.open_targets().len();
            Ok(format!(
                "parity `{}` gen{}: {} open / {} targets — list them with harna_parity",
                ev.id, ev.generation, open, ev.targets.len()
            ))
        })
    }

    /// Resolve one parity target.
    #[tool(description = "Resolve a parity target: status 'ack' when the propagated change landed in that module; 'noop' when nothing was needed (include why — the note is the record). Open parity debts block validation.")]
    async fn harna_parity_resolve(
        &self,
        Parameters(ParityResolveRequest { id, path, status, note }): Parameters<ParityResolveRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            let ev = harna_core::enforcement::parity::resolve(
                &store,
                &id,
                &path,
                &status,
                note.as_deref().unwrap_or(""),
                "agent via harna MCP",
            )?;
            Ok(format!(
                "resolved `{path}` -> {} in `{}`; {} open target(s) left",
                status,
                ev.id,
                ev.open_targets().len()
            ))
        })
    }

    /// Auto-verify open parity targets against the event's baseline.
    #[tool(description = "Re-verify open targets against their baseline WITHOUT a new generation: surfaces unchanged vs the baseline become auto-verified and stop being debts; changed or deleted modules stay open. Give an id to verify one event, or omit it to sweep every event.")]
    async fn harna_parity_auto(
        &self,
        Parameters(ParityAutoRequest { id }): Parameters<ParityAutoRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| match id.as_deref() {
            Some(id) => {
                let (ev, verified, open) =
                    harna_core::enforcement::parity::auto(&self.root, &store, id, 10_000)?;
                Ok(format!(
                    "parity `{}`: {} auto-verified, {} still open (generation unchanged)",
                    ev.id, verified, open
                ))
            }
            None => Ok(harna_core::enforcement::parity::auto_all(&self.root, &store, 10_000)?
                .join("\n")),
        })
    }

    /// PLAN step: full pre-code briefing for an idea, before any code exists.
    #[tool(description = "Full pre-code briefing for an idea BEFORE writing code — reads as a decision proposal: where similar things live (code map) and which containers to extend, the decision context (supersession-aware: superseded decisions are named visibly dead), the rules that will constrain the change (with their bound check ids), the dependency fan-out to expect (dependents query), and the test obligations (registered tests, no-test-update advisory) plus the data-source convention. One tool call; the PLAN step of the flow: plan -> place -> code -> check -> align -> commit. CLI: harna plan <idea> (harna place is the quick placement map only).")]
    async fn harna_place(
        &self,
        Parameters(PlaceRequest { idea }): Parameters<PlaceRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| harna_core::composition::plan::build(&store, &idea))
    }

    /// Burst journal: record state before compaction; resume from it later.
    #[tool(description = "Record the current-burst state BEFORE compaction or running out of context: summary plus optional next_steps. harna_onboarding and harna handoff carry it into every future session — the 'where was I?' answer survives restarts. clear=true removes it once the burst is resumed. Without summary/clear this reads the current handoff.")]
    async fn harna_handoff(
        &self,
        Parameters(HandoffRequest { summary, next_steps, clear }): Parameters<HandoffRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            if clear.unwrap_or(false) {
                return Ok(if harna_core::schema::handoff::clear(&store)? {
                    "handoff cleared (burst resumed)".into()
                } else {
                    "no handoff on disk".into()
                });
            }
            match summary {
                Some(s) => {
                    let open_parity =
                        harna_core::enforcement::parity::open_debt_count(&store).unwrap_or(0);
                    let path = harna_core::schema::handoff::save(
                        &store,
                        &s,
                        &next_steps.unwrap_or_default(),
                        open_parity,
                    )?;
                    Ok(format!("handoff recorded: {}", path.display()))
                }
                None => Ok(match harna_core::schema::handoff::load(&store) {
                    Some(h) => harna_core::schema::handoff::render(&h),
                    None => {
                        "no handoff recorded — none needed if the burst is one plain commit".into()
                    }
                }),
            }
        })
    }

    /// Reverse-edge query: who imports this module (ring 1) and who follows.
    #[tool(description = "Dependents of a module, from the code map's import graph: direct importers (ring 1, with the dep that creates the edge) and expected knock-on (ring 2), plus the manifest containers hosting them. Ask BEFORE making a change; the answer is the expected fan-out shape to compare against harna_parity_open afterwards.")]
    async fn harna_dependents(
        &self,
        Parameters(DependentsRequest { module }): Parameters<DependentsRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            Ok(harna_core::search::dependents::build(&store, &module)?.render())
        })
    }

    /// Generate deny.toml (compile-time layering) from the manifest.
    #[tool(description = "Render the manifest's layer topology into deny.toml for cargo-deny: each workspace crate may only be depended on by crates whose layer is the same or a declared dependency. Run before CI so 'cargo deny check bans' turns layer violations into build errors.")]
    async fn harna_deny(
        &self,
        Parameters(EmptyRequest { .. }): Parameters<EmptyRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            let manifest = store.load_manifest()?;
            let plan = harna_core::generation::deny::plan(&self.root, &manifest)?;
            let path = harna_core::generation::deny::generate(&self.root, &manifest)?;
            let mut out = format!("deny.toml written: {}\n", path.display());
            for r in &plan.rules {
                let deps = if r.wrappers.is_empty() {
                    "nobody".to_string()
                } else {
                    r.wrappers.join(", ")
                };
                out.push_str(&format!("  {} may be depended on by: {}\n", r.crate_name, deps));
            }
            Ok(out)
        })
    }

    /// Installation self-audit.
    #[tool(description = "harna doctor: is the harna installation healthy in this repo? Checks schema currency (offers migrate), data-file parseability, digest freshness, rules, git presence, the commit gate and agent wiring. FAIL items block work; act on them first.")]
    async fn harna_doctor(
        &self,
        Parameters(DoctorRequest {}): Parameters<DoctorRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(|store| {
            let report = harna_core::composition::doctor::build(&store, &self.root)?;
            Ok(report.render())
        })
    }

    /// Scaffold a new component from a template and register it in the manifest.
    #[tool(description = "Scaffold a primitive or compound backend component from a template, and register it in .harna/manifest.toml. Always prefer scaffolding over hand-creating structure.")]
    async fn harna_scaffold(
        &self,
        Parameters(ScaffoldRequest { layer, container, name, kind, path, description }): Parameters<ScaffoldRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            let kind = match kind.as_str() {
                "primitive" => harna_core::generation::scaffold::ScaffoldKind::Primitive,
                "compound" => harna_core::generation::scaffold::ScaffoldKind::Compound,
                "data" => harna_core::generation::scaffold::ScaffoldKind::Data,
                other => anyhow::bail!("unknown kind '{other}': use 'primitive', 'compound' or 'data'"),
            };
            let req = harna_core::generation::scaffold::ScaffoldRequest {
                layer, container, name, kind, path,
                description: description.unwrap_or_default(),
            };
            let res = harna_core::generation::scaffold::run(&self.root, &store, &req)?;
            Ok(res.message)
        })
    }

    /// Scaffold a transiter: a primitive dedicated to connecting two engines.
    #[tool(description = "Scaffold a transiter: a dedicated primitive that maintains the connection between two engines/layers. Engines must never call each other directly.")]
    async fn harna_scaffold_transiter(
        &self,
        Parameters(TransiterRequest { from, to, name }): Parameters<TransiterRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            let res = harna_core::generation::scaffold::scaffold_transiter(&self.root, &store, &from, &to, name)?;
            Ok(res.message)
        })
    }

    fn call<F>(&self, f: F) -> Result<String, rmcp::ErrorData>
    where
        F: FnOnce(Store) -> anyhow::Result<String>,
    {
        let store = self.store().map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        f(store).map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))
    }
}

fn today() -> String {
    std::env::var("HARNA_DATE").unwrap_or_else(|_| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| format!("unix-{}", d.as_secs() / 86_400))
            .unwrap_or_else(|_| "unknown-date".into())
    })
}

#[tool_handler]
impl ServerHandler for HarnaTools {
    fn list_resources(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<
        Output = Result<rmcp::model::ListResourcesResult, rmcp::ErrorData>,
    > + Send + '_ {
        let resources = vec![
            rmcp::model::Resource {
                raw: rmcp::model::RawResource {
                    uri: "harna://manifest.toml".into(),
                    name: "manifest".into(),
                    description: Some(
                        "architecture: layers -> containers -> components (read-only)"
                            .into(),
                    ),
                    mime_type: Some("application/toml".into()),
                    size: None,
                },
                annotations: None,
            },
            rmcp::model::Resource {
                raw: rmcp::model::RawResource {
                    uri: "harna://rules.toml".into(),
                    name: "rules".into(),
                    description: Some(
                        "coding rules with rationale, scope, origin (read-only)".into(),
                    ),
                    mime_type: Some("application/toml".into()),
                    size: None,
                },
                annotations: None,
            },
            rmcp::model::Resource {
                raw: rmcp::model::RawResource {
                    uri: "harna://digest.json".into(),
                    name: "digest".into(),
                    description: Some(
                        "compressed code map: signatures, inventory, deps (read-only)"
                            .into(),
                    ),
                    mime_type: Some("application/json".into()),
                    size: None,
                },
                annotations: None,
            },
        ];
        std::future::ready(Ok(rmcp::model::ListResourcesResult::with_all_items(resources)))
    }

    fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParam,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<
        Output = Result<rmcp::model::ReadResourceResult, rmcp::ErrorData>,
    > + Send + '_ {
        let file = match request.uri.as_str() {
            "harna://manifest.toml" => Some("manifest.toml"),
            "harna://rules.toml" => Some("rules.toml"),
            "harna://digest.json" => Some("digest.json"),
            _ => None,
        };
        let result = match file {
            Some(f) => {
                let path = self.root.join(".harna").join(f);
                std::fs::read_to_string(&path).map(|text| {
                    rmcp::model::ReadResourceResult {
                        contents: vec![rmcp::model::ResourceContents::text(text, request.uri.clone())],
                    }
                })
            }
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("unknown harna resource: {}", request.uri),
            )),
        };
        std::future::ready(result.map_err(|e| rmcp::ErrorData {
            code: rmcp::model::ErrorCode::INVALID_PARAMS,
            message: e.to_string().into(),
            data: None,
        }))
    }
}
