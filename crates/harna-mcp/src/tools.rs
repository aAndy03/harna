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
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DecisionRequest {
    /// Short title of the decision.
    pub title: String,
    /// The decision body: context, choice, consequences.
    pub body: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ScanRequest {
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
    #[tool(description = "Record a coding rule with its rationale so future agent sessions inherit it automatically. Call when the user states a preference or a convention is agreed.")]
    async fn harna_add_rule(
        &self,
        Parameters(AddRuleRequest { id, text, rationale, scope, env }): Parameters<AddRuleRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            let mut rules = store.load_rules()?;
            if rules.rules.iter().any(|r| r.id == id) {
                anyhow::bail!("rule id '{id}' already exists");
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
            });
            store.save_rules(&rules)?;
            Ok(format!("rule '{id}' saved; total rules: {}", rules.rules.len()))
        })
    }

    /// Persist a decision to .harna/decisions/ as ADR-style markdown.
    #[tool(description = "Record an architectural decision to .harna/decisions/ so future sessions know why something was built this way.")]
    async fn harna_log_decision(
        &self,
        Parameters(DecisionRequest { title, body }): Parameters<DecisionRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            let path = store.save_decision(&title, &body)?;
            Ok(format!("decision saved to {}", path.display()))
        })
    }

    /// Rebuild the compressed code map; agents query this instead of rescanning.
    #[tool(description = "Rebuild the compressed codebase digest (signatures only) into .harna/digest.json. Run after structurally significant changes.")]
    async fn harna_scan(
        &self,
        Parameters(ScanRequest { max_files }): Parameters<ScanRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            let d = digest::build(&self.root, max_files.unwrap_or(10_000) as usize)?;
            store.save_digest(&d)?;
            Ok(format!(
                "digest: {} modules, {} signatures",
                d.module_count(),
                d.signature_count()
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
                other => anyhow::bail!("unknown kind '{other}': use 'primitive' or 'compound'"),
            };
            let req = harna_core::generation::scaffold::ScaffoldRequest {
                layer, container, name, kind, path,
                description: description.unwrap_or_default(),
            };
            let res = harna_core::generation::scaffold::run(&self.root, &store, &req)?;
            Ok(format!(
                "created {} and registered {} in the manifest",
                res.files.join(", "),
                res.components.join(", ")
            ))
        })
    }

    /// Scaffold a transiter engine connecting two layers.
    #[tool(description = "Scaffold a transiter: a dedicated primitive that maintains the connection between two engines/layers. Engines must never call each other directly.")]
    async fn harna_scaffold_transiter(
        &self,
        Parameters(TransiterRequest { from, to, name }): Parameters<TransiterRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        self.call(move |store| {
            let res = harna_core::generation::scaffold::scaffold_transiter(&self.root, &store, &from, &to, name)?;
            Ok(format!(
                "created {} and registered transiter in the manifest",
                res.files.join(", ")
            ))
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
impl ServerHandler for HarnaTools {}
