//! The architecture manifest: layers -> containers -> components.
//!
//! Components are `primitive` (leaf building blocks) or `compound`
//! (hierarchical compositions of other components). Compound components
//! nest via `children`, keeping the hierarchy explicit but standalone.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::schema::map::Digest;

/// Top-level manifest document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Data schema version; written by the tool, migrated by `harna migrate`.
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
    /// Test kinds (0.4.7): where each kind of test lives and which layers
    /// it may reach beyond its home's. Declaring any engages `test-reach`;
    /// none declared, tests stay outside layering as before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test_kinds: Vec<TestKind>,
}

impl Manifest {
    /// The lowest schema that holds this manifest (see `SCHEMA_BASELINE`).
    pub fn required_schema(&self) -> u32 {
        fn planned(cs: &[Component]) -> bool {
            cs.iter().any(|c| c.planned || planned(&c.children))
        }
        let v2 = !self.test_kinds.is_empty()
            || self
                .layers
                .iter()
                .any(|l| l.isolated || l.containers.iter().any(|c| planned(&c.components)));
        if v2 {
            2
        } else {
            crate::SCHEMA_BASELINE
        }
    }
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            schema_version: crate::SCHEMA_BASELINE,
            layers: Vec::new(),
            test_kinds: Vec::new(),
        }
    }
}

/// One kind of test (unit, integration, ui, snapshot — the project's names).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TestKind {
    pub name: String,
    /// Path globs that hold this kind (`crates/*/tests/**`, `**/*_tests.rs`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// Layers it may reach beyond its home layer's own reach; `*` = any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reach: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// A marker the file must contain (0.5.8): a regex such as
    /// `#[gpui_kit::test]` makes UI tests a kind of their own even
    /// where they sit beside other tests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contains: Option<String>,
}

impl TestKind {
    /// Whether the path lies in the kind's globs (the marker unread).
    pub fn covers(&self, path: &str) -> bool {
        self.paths
            .iter()
            .any(|p| crate::schema::model::scope::glob_match(p, path))
    }

    /// Whether the file is of this kind: in its globs and, when the kind
    /// names a marker, containing it (a broken marker matches nothing).
    pub fn covers_file(&self, path: &str, src: &str) -> bool {
        self.covers(path)
            && self
                .contains
                .as_deref()
                .is_none_or(|m| regex::Regex::new(m).is_ok_and(|re| re.is_match(src)))
    }
}

pub fn current_schema_version() -> u32 {
    crate::SCHEMA_BASELINE
}

/// A horizontal architectural layer (e.g. "data-access", "domain").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The layer's role in the role model (0.3.25 layers as data): one of
    /// [`ROLES`] — base | engine | gateway | orchestration-sync |
    /// orchestration-async | transit | interface | context. Containers
    /// inherit it unless they declare their own `role`. Adoption stays
    /// data-first: an undeclared role owes nothing (no key, no gate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Explicit boot/init order of the layer's containers. Empty = unordered.
    /// Declared explicitly rather than numeric — inserting never renumbers
    /// siblings; reordering is a one-line diff.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sequence: Vec<String>,
    /// Layers this layer may reach into (peer imports forbidden; downward
    /// declared only). Used by `harna deny` to derive compile-time bans.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    /// Sibling isolation (0.4.2): the layer's containers are peers that may
    /// not import each other — features beside features, engines beside
    /// engines. They meet through a lower layer (a shared capability) or a
    /// declared corridor. gpui-kit's guide: a feature "must not reach into
    /// a sibling feature's internals".
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub isolated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub containers: Vec<Container>,
}

/// A container groups related components inside a layer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Container {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    /// One-line purpose (rendered by generated views).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Explicit order of the container's components (same logic as layer.sequence).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sequence: Vec<String>,
    /// Declared public symbols (0.3.4 encapsulation): the container's export
    /// surface. When non-empty, importers OUTSIDE this container may only
    /// consume these symbols — one check from two sides (declared here,
    /// observed on the import graph). Empty = not declared yet (no gate).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provides: Vec<String>,
    /// Container's structural role (0.3.25 layers as data): overrides the
    /// layer's role when declared. See [`ROLES`] for the vocabulary and
    /// `effective_role` for the inheritance rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Registered gateway (0.3.10 boundary constructs): the boundary
    /// facilities this container exclusively touches, comma-separated with
    /// an optional `:write` mode qualifier — `gateway = "fs:write"` means
    /// only this container may WRITE the facility; reading stays open.
    /// Facilities: `fs` (local disk IO), `net` (network + HTTP + DB
    /// clients), `process` (subprocesses), `base` (imports into the
    /// manifest's rank-0 layers). Declaring one engages the
    /// `gateway-ownership` check for that facility — adoption is data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    /// Declared transit corridors (0.3.10): the layers this container
    /// mediates between (`connects = ["api", "billing"]`). Declaring them
    /// makes the container a transiter: its modules must stay thin
    /// (`transiter-thinness`), and direct cross-layer imports between the
    /// connected layers bypass it visibly (`no-sideways`). Adoption is
    /// data: without declared corridors nothing is gated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connects: Vec<String>,
    /// What only this container may construct or declare (0.3.45, deno
    /// study B1/B5). A type name (`CheckedPath`) is a capability: building
    /// it (`CheckedPath {` / `CheckedPath::new(`) elsewhere is a violation,
    /// holding or passing it is not — deno's permission layer works this
    /// way. A marker (`#[op2`, `extension!`) may appear nowhere else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owns: Vec<String>,
    /// Sanctioned escape hatches out of an owned capability
    /// (`CheckedPath::unsafe_new`): every call is counted (check
    /// `escape-hatch`), so a baseline can ratchet them down.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub escapes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<Component>,
}

/// A backend component: primitive leaf or compound composition.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub kind: ComponentKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    /// One-line purpose: rendered by generated views and read back by
    /// briefings. Formalized when `harna render` needed the prose viewers
    /// had been hand-maintaining next to the data.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// Tests that cover this component, registered as manifest data instead
    /// of convention. Inline tests register the component's own path;
    /// external test files register themselves. The `validate/tests` check
    /// holds changed components to their registered tests.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<String>,
    /// Declared public symbols this component contributes to its container's
    /// surface (adds to the container's own `provides` when the check runs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provides: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Component>,
    /// A planned slot (0.4.1): declared before its code exists — "the cache
    /// will live here, behind this gateway". Its missing file is intent,
    /// not drift; `harna scaffold` with its name fills it at its path.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub planned: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentKind {
    #[default]
    Primitive,
    Compound,
}

/// The role vocabulary (0.3.25 layers as data): each role's one-line
/// meaning, as declared on `[[layers]]` (`role = "..."`) or on a container
/// (which overrides its layer). Roles are data, not code: a role the checks
/// don't know is narrated, never invented (same discipline as gateway
/// facilities — the check registry speaks only what this table declares).
pub const ROLES: &[(&str, &str)] = &[
    (
        "base",
        "owns the repository's primary facility and the data everything else reads through",
    ),
    (
        "engine",
        "sync computation — no async fns; any gateway it carries is deliberate",
    ),
    (
        "gateway",
        "exists to own boundary facilities on behalf of the other layers",
    ),
    (
        "orchestration-sync",
        "drives engines in ordinary (sync) flows",
    ),
    (
        "orchestration-async",
        "drives engines in async task flows — the non-interface home for async fns",
    ),
    (
        "transit",
        "mediates between layers and stays thin (`connects` corridors)",
    ),
    (
        "interface",
        "faces humans/agents — CLI, MCP; carries the async runtime boundary",
    ),
    (
        "context",
        "composes project knowledge into agent-facing briefings",
    ),
];

/// Is `role` a declared vocabulary entry?
pub fn is_known_role(role: &str) -> bool {
    ROLES.iter().any(|(id, _)| *id == role)
}

/// The roles that may carry `async fn`s: async orchestration is their
/// business, and interfaces ride the async runtime (servers, handlers).
/// Every other role — engines above all — stays sync (`role-async` issue).
pub fn async_allowed(role: &str) -> bool {
    matches!(role, "orchestration-async" | "interface")
}

/// A container's effective role: its own declaration wins, else the
/// layer's. Undeclared (None) owes nothing — adoption is data.
pub fn effective_role<'a>(layer: &'a Layer, container: &'a Container) -> Option<&'a str> {
    container.role.as_deref().or(layer.role.as_deref())
}

/// The vocabulary rendered for hints: `base (data owner); engine (sync)…`
pub fn role_catalog() -> String {
    ROLES
        .iter()
        .map(|(id, meaning)| format!("{id}: {meaning}"))
        .collect::<Vec<_>>()
        .join("; ")
}

impl Manifest {
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// External crates owned by a gateway (0.3.48, R2): every
    /// `gateway = "crate:NAME"` part, as (package name, owner container
    /// paths) in manifest order. Read by validate and by `harna deny`.
    pub fn crate_gateways(&self) -> Vec<(String, Vec<String>)> {
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        for c in self.layers.iter().flat_map(|l| &l.containers) {
            let Some(raw) = c.gateway.as_deref() else {
                continue;
            };
            for name in raw.split(',').filter_map(external_crate) {
                match out.iter_mut().find(|(n, _)| *n == name) {
                    Some((_, paths)) => paths.push(c.path.clone()),
                    None => out.push((name, vec![c.path.clone()])),
                }
            }
        }
        out
    }

    /// Total number of components (including nested children).
    pub fn component_count(&self) -> usize {
        self.layers
            .iter()
            .flat_map(|l| l.containers.iter())
            .map(|c| count_components(&c.components))
            .sum()
    }

    pub fn container_count(&self) -> usize {
        self.layers.iter().map(|l| l.containers.len()).sum()
    }
}

fn count_components(comps: &[Component]) -> usize {
    comps
        .iter()
        .map(|c| 1 + count_components(&c.children))
        .sum()
}

/// A digest module's home: the container whose path prefixes the module
/// (longest container path wins when containers nest) and that
/// container's layer. Lives in memory (0.3.15) — it joins two memory
/// types (Manifest + Digest), so the store can shard by container, search
/// can measure by home, and checks can gate by home, all sharing the one
/// resolution instead of copying it.
pub struct ModuleHome<'m> {
    pub layer: &'m str,
    pub container_path: &'m str,
}

/// digest module path -> its home. Modules outside every container stay
/// unhomed (the map does not reach them — same adoption discipline as
/// the gateway check's ungoverned carve-out).
pub fn module_homes<'m>(
    manifest: &'m Manifest,
    digest: &'m Digest,
) -> HashMap<&'m str, ModuleHome<'m>> {
    let mut hosts: Vec<(&str, &str)> = manifest
        .layers
        .iter()
        .flat_map(|l| {
            l.containers
                .iter()
                .map(|c| (c.path.as_str(), l.name.as_str()))
        })
        .collect();
    hosts.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(b.0)));
    let mut map = HashMap::new();
    for m in &digest.modules {
        if let Some((path, layer)) = hosts
            .iter()
            .find(|(path, _)| crate::schema::model::scope::path_in_container(&m.path, path))
        {
            map.insert(
                m.path.as_str(),
                ModuleHome {
                    layer,
                    container_path: path,
                },
            );
        }
    }
    map
}

/// digest module path -> owning layer name. The longest container path
/// wins when containers nest (the same prefix rule applied everywhere the
/// manifest prefixes the map). Derived from `module_homes`.
pub fn module_layers<'m>(manifest: &'m Manifest, digest: &'m Digest) -> HashMap<&'m str, &'m str> {
    module_homes(manifest, digest)
        .into_iter()
        .map(|(k, home)| (k, home.layer))
        .collect()
}

/// One gateway part naming an external crate: `crate:reqwest` -> `reqwest`.
pub fn external_crate(part: &str) -> Option<String> {
    let (word, name) = part.trim().split_once(':')?;
    let name = name.trim();
    (word.trim() == "crate" && !name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        let toml_src = r#"
[[layers]]
name = "data-access"

[[layers.containers]]
name = "users"
path = "src/data/users"

[[layers.containers.components]]
name = "user-repo"
kind = "primitive"
path = "src/data/users/repo.rs"
summary = "user persistence"

[[layers.containers.components]]
name = "user-store"
kind = "compound"
path = "src/data/users/store"

[[layers.containers.components.children]]
name = "connection-pool"
kind = "primitive"
"#;
        toml::from_str(toml_src).unwrap()
    }

    #[test]
    fn roles_are_additive_with_serde_defaults() {
        // A pre-0.3.25 manifest (no role lines) loads clean; declared roles
        // survive a roundtrip untouched.
        let with_roles = r#"
[[layers]]
name = "engines"
role = "engine"

[[layers.containers]]
name = "billing"
path = "src/engines/billing"
role = "gateway"
"#;
        let m: Manifest = toml::from_str(with_roles).unwrap();
        assert_eq!(m.layers[0].role.as_deref(), Some("engine"));
        assert_eq!(m.layers[0].containers[0].role.as_deref(), Some("gateway"));

        // Container overrides layer; container inherits when undeclared.
        assert_eq!(
            effective_role(&m.layers[0], &m.layers[0].containers[0]),
            Some("gateway")
        );
        assert_eq!(
            effective_role(
                &m.layers[0],
                &Container {
                    name: "plain".into(),
                    ..Default::default()
                }
            ),
            Some("engine")
        );

        // The vocabulary is coherent: known roles resolve, unknown don't,
        // async is allowed exactly in orchestration-async and interface.
        assert!(is_known_role("engine") && is_known_role("orchestration-async"));
        assert!(!is_known_role("library"));
        for (id, _) in ROLES {
            let allowed = async_allowed(id);
            assert_eq!(
                allowed,
                matches!(*id, "orchestration-async" | "interface"),
                "{id}"
            );
        }
        assert!(!role_catalog().is_empty());
    }

    #[test]
    fn roundtrip() {
        let m = sample();
        let s = toml::to_string(&m).unwrap();
        let back: Manifest = toml::from_str(&s).unwrap();
        assert_eq!(m.component_count(), back.component_count());
    }

    #[test]
    fn counts_nested_children() {
        assert_eq!(sample().component_count(), 3);
        assert_eq!(sample().container_count(), 1);
    }

    #[test]
    fn tests_field_is_additive_with_serde_default() {
        // Old manifests (no `tests` entries) deserialize clean.
        let m = sample();
        assert!(m.layers[0].containers[0].components[0].tests.is_empty());

        let with_tests = r#"
[[layers]]
name = "data-access"

[[layers.containers]]
name = "users"
path = "src/data/users"

[[layers.containers.components]]
name = "user-repo"
kind = "primitive"
path = "src/data/users/repo.rs"
summary = "user persistence"
tests = ["tests/repo_test.rs"]
"#;
        let m: Manifest = toml::from_str(with_tests).unwrap();
        let comp = &m.layers[0].containers[0].components[0];
        assert_eq!(comp.tests, vec!["tests/repo_test.rs"]);
        assert_eq!(comp.summary, "user persistence");
        // Serde defaults mean a roundtrip never drops registered data.
        let back: Manifest = toml::from_str(&toml::to_string(&m).unwrap()).unwrap();
        assert_eq!(back.layers[0].containers[0].components[0].tests.len(), 1);
        assert_eq!(
            back.layers[0].containers[0].components[0].summary,
            "user persistence"
        );
    }

    #[test]
    fn gateway_and_connects_are_additive_with_serde_defaults() {
        // A pre-0.3.10 manifest (no gateway/connects) loads clean; declared
        // values survive a roundtrip untouched.
        let with_data = r#"
[[layers]]
name = "data-access"

[[layers.containers]]
name = "users"
path = "src/data/users"
gateway = "fs:write"
connects = ["api", "billing"]
"#;
        let m: Manifest = toml::from_str(with_data).unwrap();
        let c = &m.layers[0].containers[0];
        assert_eq!(c.gateway.as_deref(), Some("fs:write"));
        assert_eq!(c.connects, vec!["api", "billing"]);
        let back: Manifest = toml::from_str(&toml::to_string(&m).unwrap()).unwrap();
        let c = &back.layers[0].containers[0];
        assert_eq!(c.gateway.as_deref(), Some("fs:write"));
        assert_eq!(c.connects, vec!["api", "billing"]);

        // Empty declarations serialize as absent: the committed manifest
        // stays uncluttered for containers that declare nothing.
        let rendered = toml::to_string(&sample()).unwrap();
        assert!(!rendered.contains("gateway"));
        assert!(!rendered.contains("connects"));
    }
}
