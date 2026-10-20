//! The argument shapes of the tools defined in `tools.rs` and
//! `tools/flow.rs` (each becomes a tool's input schema).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct RulesRequest {
    /// Optional scope path to filter rules by, e.g. "src/engine".
    pub scope: Option<String>,
    /// Session environment ("dev", "prod"…); hides other envs' rules.
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
    /// Only in this environment ("dev", "prod"…); omit for all.
    pub env: Option<String>,
    /// Check id to bind (violations cite the rule).
    pub check: Option<String>,
    /// "error" | "info", overriding the bound check's default.
    pub severity: Option<String>,
    /// Regex over file paths and declared names.
    pub pattern: Option<String>,
    /// Budget for a budget check.
    pub limit: Option<u32>,
    /// List args (banned paths => replacement, sync files).
    #[serde(default)]
    pub values: Vec<String>,
    /// CI proof command.
    pub command: Option<String>,
    /// Regex over declarations.
    pub scope_signature: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct RenderRequest {
    /// Verify the committed views are current instead of writing.
    pub check: Option<bool>,
    /// Only SURFACE.md.
    pub surface: Option<bool>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct WhereisRequest {
    /// Symbol name, optionally module-qualified ("module::Symbol"); a trailing "()" is tolerated.
    pub symbol: String,
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
    /// File name in .harna/decisions/ of the decision this replaces.
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
    /// Only files with uncommitted changes (use after edits).
    pub changed_only: Option<bool>,
    /// Session environment; other envs' rules are not enforced.
    pub env: Option<String>,
    /// Info findings block too (default: only errors block).
    pub strict: Option<bool>,
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
    /// What this burst did (records; overwrites). Omit to read.
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
    /// Event id, e.g. "change-store-api"; reuse = revise.
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
    /// Layer (created if missing).
    pub layer: String,
    /// Container (created if missing).
    pub container: String,
    /// Component name in kebab-case, e.g. "user-repo".
    pub name: String,
    /// "primitive" (file) | "compound" (folder) | "data" (toml + loader).
    pub kind: String,
    /// Repo-relative path; default <container path>/<name>.
    pub path: Option<String>,
    /// One line, used as the component summary.
    pub description: Option<String>,
    /// Container role (see the concept map), e.g. "engine".
    pub role: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct MoveRequest {
    /// Repo-relative source file or folder.
    pub from: String,
    /// Repo-relative destination.
    pub to: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ManifestEditRequest {
    pub action: String,
    /// Container or layer.
    pub name: String,
    pub new_name: Option<String>,
    /// test-kind: comma-separated globs (none removes it).
    pub path: Option<String>,
    /// fs | fs:write | net | process | base
    pub gateway: Option<String>,
    pub role: Option<String>,
    /// Comma-separated layers (test-kind: its reach).
    pub depends_on: Option<String>,
    pub owns: Option<String>,
    pub escapes: Option<String>,
    pub provides: Option<String>,
    pub isolated: Option<bool>,
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct TransiterRequest {
    /// From layer.
    pub from: String,
    /// To layer.
    pub to: String,
    /// Custom name.
    pub name: Option<String>,
}

/// MCP tool arguments are always a JSON object — a bare bool/string
/// parameter advertises a non-object schema and no client can call it.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct MigrateRequest {
    /// Preview only.
    pub dry: Option<bool>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct RestoreRequest {
    /// Data-file name: 'manifest.toml' | 'rules.toml' (no path separators).
    pub name: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ArchiveRequest {
    /// Score snapshots kept live (newest kept first; the rest archive).
    pub keep: Option<usize>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GraphRequest {
    /// Output format: "dot" | "mermaid" (default mermaid).
    pub format: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct TemplateAddRequest {
    /// Stable id, e.g. schema-shard-change.
    pub id: String,
    /// Module-path prefix this change type touches (empty = everywhere).
    pub scope: Option<String>,
    /// The fan-out ceiling an event over this scope shouldn't walk past.
    pub max_targets: usize,
    /// Why this shape is declared.
    pub rationale: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DeferredAddRequest {
    /// Stable id, e.g. fixture-builder-dedup.
    pub id: String,
    /// What it would touch: component, module or container path.
    pub component: String,
    /// Why it is deferred.
    pub reason: String,
    /// When to revisit: ISO date or version, e.g. "0.4.0".
    pub review: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct NotesRequest {
    /// "review" | "release".
    pub kind: String,
    /// Parity event id (review); omit to diff from `from`.
    pub id: Option<String>,
    /// Git ref, default HEAD.
    pub from: Option<String>,
}
