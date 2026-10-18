//! Scaffolding: generate primitive/compound components from templates so
//! structure is born correct, and register it in the manifest in the same
//! transaction. The agent never hand-builds structure — it calls this.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::schema::manifest::{
    is_known_role, role_catalog, Component, ComponentKind, Container, Layer, Manifest,
};
use crate::schema::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScaffoldKind {
    Primitive,
    Compound,
    /// Data source: value-set literals live in .toml (constants-are-data) and
    /// code imports one loader. `name` becomes `<name>.toml` + `<name>-loader.rs`.
    Data,
}

/// What to scaffold and where it lives in the architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaffoldRequest {
    /// Layer name (created if missing), e.g. "data-access".
    pub layer: String,
    /// Container name within the layer (created if missing), e.g. "users".
    pub container: String,
    /// Component name in kebab-case, e.g. "user-repo".
    pub name: String,
    /// primitive = leaf component; compound = composition of components.
    pub kind: ScaffoldKind,
    /// Optional explicit path; defaults to `<container.path>/<name>` when set,
    /// else `src/<layer>/<container>/<name>`.
    #[serde(default)]
    pub path: Option<String>,
    /// One-line description embedded in the generated files.
    #[serde(default)]
    pub description: String,
    /// Role for the container (0.3.25 layers as data): one of the
    /// [`crate::schema::manifest::ROLES`] vocabulary — stamped on the
    /// container (created or existing). None = leave the role as-is.
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaffoldResult {
    /// Files created or updated.
    pub files: Vec<String>,
    /// Where the component was registered (manifest.toml).
    pub registered_in: String,
    /// Names of the generated components (compound scaffolds also create a child).
    pub components: Vec<String>,
    /// Human/agent-readable one-line summary.
    pub message: String,
}

/// Scaffold a component and register it in the manifest.
pub fn run(root: &Path, store: &Store, req: &ScaffoldRequest) -> anyhow::Result<ScaffoldResult> {
    if req.name.is_empty() {
        anyhow::bail!("component name must not be empty");
    }
    if let Some(role) = &req.role {
        if !is_known_role(role) {
            anyhow::bail!("unknown role `{role}` — the vocabulary is data: {}", role_catalog());
        }
    }
    store.init()?;
    // init guarantees a manifest exists, so a load failure is a broken or
    // newer-schema file — never default it: saving would overwrite the
    // user's whole architecture with just this one component.
    let mut manifest = store
        .load_manifest()
        .map_err(|e| e.context("scaffold refuses to rewrite an unreadable manifest"))?;

    // Reject duplicates before taking any mutable borrow of the manifest.
    if find_component(&manifest, &req.name).is_some() {
        anyhow::bail!("component `{}` already exists in the manifest", req.name);
    }

    let layer = find_or_create_layer(&mut manifest, &req.layer);
    let container = find_or_create_container(layer, &req.container);
    if let Some(role) = &req.role {
        container.role = Some(role.clone());
    }

    // Default path: <container.path>/<name>[...]
    let container_dir = if container.path.is_empty() {
        format!("src/{}/{}", req.layer, req.container)
    } else {
        container.path.clone()
    };
    let base = req
        .path
        .clone()
        .unwrap_or_else(|| format!("{}/{}", container_dir, req.name));
    check_inside_repo(&base)?;

    let mut components = Vec::new();
    let mut files = Vec::new();

    match req.kind {
        ScaffoldKind::Primitive => {
            let file_rel = if base.ends_with(".rs") {
                base.clone()
            } else {
                format!("{}.rs", base)
            };
            let abs = root.join(&file_rel);
            if abs.exists() {
                anyhow::bail!("refusing to overwrite existing file: {}", file_rel);
            }
            if let Some(parent) = abs.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&abs, primitive_template(&req.name, &req.description, &container_dir))?;
            files.push(file_rel.clone());
            components.push(req.name.clone());
            container.components.push(Component {
                name: req.name.clone(),
                kind: ComponentKind::Primitive,
                path: file_rel.clone(),
                summary: req.description.clone(),
                tests: Vec::new(),
                provides: Vec::new(),
                children: Vec::new(),
            });
        }
        ScaffoldKind::Compound => {
            // Compound components are folders: mod.rs plus one starter child.
            let dir_rel = base.clone();
            let abs = root.join(&dir_rel);
            if abs.exists() {
                anyhow::bail!("refusing to overwrite existing folder: {}", dir_rel);
            }
            fs::create_dir_all(&abs)?;
            let child_name = format!("{}-core", req.name);
            let child_file = format!("{}/{}.rs", dir_rel, child_name);
            fs::write(
                root.join(&child_file),
                primitive_template(&child_name, &req.description, &dir_rel),
            )?;
            fs::write(
                root.join(format!("{}/mod.rs", dir_rel)),
                compound_mod_template(&req.name, &child_name, &req.description),
            )?;
            files.push(format!("{}/mod.rs", dir_rel));
            files.push(child_file.clone());
            container.components.push(Component {
                name: req.name.clone(),
                kind: ComponentKind::Compound,
                path: format!("{}/mod.rs", dir_rel),
                summary: req.description.clone(),
                tests: Vec::new(),
                provides: Vec::new(),
                children: vec![Component {
                    name: child_name.clone(),
                    kind: ComponentKind::Primitive,
                    path: child_file.clone(),
                    summary: req.description.clone(),
                    tests: Vec::new(),
                    provides: Vec::new(),
                    children: Vec::new(),
                }],
            });
            components.push(req.name.clone());
            components.push(child_name);
        }
        ScaffoldKind::Data => {
            // Constants-are-data: the value-set lives in a .toml; code reads
            // it through one loader. The toml is the single source of truth.
            let data_file = format!("{base}.toml");
            let loader_file = format!("{base}-loader.rs");
            let loader_name = format!("{}-loader", req.name);
            let abs_toml = root.join(&data_file);
            let abs_loader = root.join(&loader_file);
            if abs_toml.exists() || abs_loader.exists() {
                anyhow::bail!(
                    "refusing to overwrite existing file: {}",
                    if abs_toml.exists() { &data_file } else { &loader_file }
                );
            }
            if let Some(parent) = abs_toml.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&abs_toml, data_toml_template(&req.name, &req.description))?;
            fs::write(&abs_loader, data_loader_template(&req.name, &loader_name, &data_file))?;
            files.push(data_file.clone());
            files.push(loader_file.clone());
            components.push(req.name.clone());
            components.push(loader_name.clone());
            container.components.push(Component {
                name: req.name.clone(),
                kind: ComponentKind::Primitive,
                path: data_file.clone(),
                summary: req.description.clone(),
                tests: Vec::new(),
                provides: Vec::new(),
                children: vec![Component {
                    name: loader_name,
                    kind: ComponentKind::Primitive,
                    path: loader_file.clone(),
                    summary: req.description.clone(),
                    tests: Vec::new(),
                    provides: Vec::new(),
                    children: Vec::new(),
                }],
            });
        }
    }

    // Boot-order bookkeeping: every generated component joins the container's
    // sequence (ordering decisions stay visible in the manifest).
    let seq_was_empty = container.sequence.is_empty();
    for name in &components {
        if !container.sequence.contains(name) {
            container.sequence.push(name.clone());
        }
    }
    let seq_len = container.sequence.len();
    // The borrow ends here; nothing to drop — a reference drop is a no-op.
    let _ = container;

    store.save_manifest(&manifest)?;

    // The role stamp goes into the message: the declaration is a decision,
    // and decisions are read aloud (adopted roles are visible at a glance).
    let role_note = req
        .role
        .as_deref()
        .map(|r| format!(" (container role: {r})"))
        .unwrap_or_default();
    let message = if seq_was_empty && seq_len == 1 {
        format!(
            "created {} and registered {} in the manifest (boot sequence initialized){role_note}",
            files.join(", "),
            components.join(", ")
        )
    } else {
        format!(
            "created {} and registered {} in the manifest{role_note}",
            files.join(", "),
            components.join(", ")
        )
    };
    Ok(ScaffoldResult {
        message,
        files,
        registered_in: store.manifest_path().to_string_lossy().replace('\\', "/"),
        components,
    })
}

/// Scaffold a `transiter` — a specialized primitive that exists only to
/// connect two or more engines. Named and tagged so tools can find them.
pub fn scaffold_transiter(
    root: &Path,
    store: &Store,
    from_layer: &str,
    to_layer: &str,
    name: Option<String>,
) -> anyhow::Result<ScaffoldResult> {
    let name = name.unwrap_or_else(|| format!("transiter-{}-to-{}", from_layer, to_layer));
    let req = ScaffoldRequest {
        layer: "transit".into(),
        container: format!("{}-{}", from_layer, to_layer),
        name,
        kind: ScaffoldKind::Primitive,
        path: None,
        description: format!(
            "transiter maintaining the connection between `{from_layer}` and `{to_layer}`"
        ),
        // The transit role rides the declaration: the corridor's container
        // is born with its place in the role model (0.3.25).
        role: Some("transit".into()),
    };
    let mut result = run(root, store, &req)?;
    // Stamp the corridor declaration on the new container: a transiter is
    // not a plain primitive — `connects` records exactly which two layers
    // it bridges (`transiter-thinness` and `no-sideways` read this).
    // Hand-written `connects` entries are never clobbered.
    let mut manifest = store.load_manifest()?;
    if let Some(corridor) = manifest
        .layers
        .iter_mut()
        .find(|l| l.name == "transit")
        .and_then(|l| l.containers.iter_mut().find(|c| c.name == req.container))
    {
        if corridor.connects.is_empty() {
            corridor.connects = vec![from_layer.to_string(), to_layer.to_string()];
            store.save_manifest(&manifest)?;
        }
    }
    // Mark the generated file with the transiter role.
    result.components.clear();
    result.components.push(req.name);
    Ok(result)
}

/// Every generated path is repo-relative and stays inside the repo: `path`,
/// `layer`, `container` and `name` all reach the filesystem through it, so
/// `../../x` or an absolute `C:/x` must be refused before anything is
/// written (or registered in the manifest).
fn check_inside_repo(rel: &str) -> anyhow::Result<()> {
    use std::path::Component as C;
    let p = Path::new(rel);
    let inside = !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.starts_with('\\')
        // A drive prefix (`C:`) is absolute on Windows and a stray name
        // elsewhere; `:` is invalid in Windows paths either way.
        && !rel.contains(':')
        && p.components().all(|c| matches!(c, C::Normal(_) | C::CurDir));
    if !inside {
        anyhow::bail!("scaffold path `{rel}` must be repo-relative with no `..` (it stays inside the repo)");
    }
    Ok(())
}

fn find_or_create_layer<'m>(manifest: &'m mut Manifest, name: &str) -> &'m mut Layer {
    if let Some(pos) = manifest.layers.iter().position(|l| l.name == name) {
        return &mut manifest.layers[pos];
    }
    manifest.layers.push(Layer {
        name: name.to_string(),
        description: String::new(),
        sequence: Vec::new(),
        depends_on: Vec::new(),
        containers: Vec::new(),
        role: None,
    });
    manifest.layers.last_mut().expect("just pushed")
}

fn find_or_create_container<'m>(layer: &'m mut Layer, name: &str) -> &'m mut Container {
    if let Some(pos) = layer.containers.iter().position(|c| c.name == name) {
        return &mut layer.containers[pos];
    }
    layer.containers.push(Container {
        name: name.to_string(),
        path: String::new(),
        description: String::new(),
        sequence: Vec::new(),
        provides: Vec::new(),
        role: None,
        gateway: None,
        connects: Vec::new(),
        components: Vec::new(),
    });
    layer.containers.last_mut().expect("just pushed")
}

fn find_component<'m>(manifest: &'m Manifest, name: &str) -> Option<&'m Component> {
    for layer in &manifest.layers {
        for container in &layer.containers {
            for comp in &container.components {
                if comp.name == name {
                    return Some(comp);
                }
                if let Some(child) = find_component_rec(&comp.children, name) {
                    return Some(child);
                }
            }
        }
    }
    None
}

fn find_component_rec<'m>(comps: &'m [Component], name: &str) -> Option<&'m Component> {
    for c in comps {
        if c.name == name {
            return Some(c);
        }
        if let Some(found) = find_component_rec(&c.children, name) {
            return Some(found);
        }
    }
    None
}

fn pascal(name: &str) -> String {
    name.split(['-', '_'])
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut cs = s.chars();
            match cs.next() {
                Some(first) => first.to_uppercase().collect::<String>() + cs.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn primitive_template(name: &str, description: &str, scope: &str) -> String {
    let struct_name = pascal(name);
    format!(
        "//! `{name}` — primitive backend component.
//!
//! {desc}
//! Scope: `{scope}`. Rules that apply here live in `.harna/rules.toml`
//! (query them via the harna tools instead of assuming).

#[derive(Debug, Clone)]
pub struct {struct_name} {{
    // TODO: fields
}}

impl {struct_name} {{
    /// TODO: implement the one responsibility of this primitive.
    pub fn new() -> Self {{
        Self {{ }}
    }}
}}

impl Default for {struct_name} {{
    fn default() -> Self {{
        Self::new()
    }}
}}

#[cfg(test)]
mod tests {{
    use super::*;

    #[test]
    fn constructs() {{
        let _ = {struct_name}::new();
    }}
}}
",
        name = name,
        desc = if description.is_empty() { "TODO: describe the single responsibility." } else { description },
        scope = scope,
        struct_name = struct_name,
    )
}

fn compound_mod_template(name: &str, child_name: &str, description: &str) -> String {
    let child_mod = child_name.replace('-', "_");
    format!(
        "//! `{name}` — compound backend component.
//!
//! {desc}
//! Composition of primitive/compound children; children are declared here
//! and registered in `.harna/manifest.toml`.

pub mod {child_mod};
",
        name = name,
        desc = if description.is_empty() { "TODO: describe what this composition provides." } else { description },
        child_mod = child_mod,
    )
}

/// The data file is the single source of truth for a value-set
/// (constants-are-data). Granularity note included by design: value-sets
/// may be split into further data files as they diverge.
fn data_toml_template(name: &str, description: &str) -> String {
    format!(
        "# `{name}` — data source (single point of truth; constants-are-data).\n\
# {desc}\n\
# Keep code reading this file through `{name}-loader.rs`; when the set\n\
# diverges, split into further data files and let loaders compose them.\n\n\
[[entries]]\n\
key = \"example-entry\"\n\
value = \"replace-me\"\n",
        name = name,
        desc = if description.is_empty() { "TODO: what facts does this value-set represent?" } else { description },
    )
}

/// The loader is the one import consumers get; it owns the parsing stack so
/// callers stay data-source-agnostic.
fn data_loader_template(name: &str, loader_name: &str, data_file: &str) -> String {
    let entry_struct = pascal(name);
    format!(
        "//! `{loader_name}` — the single loader for the `{name}` data source ({data_file}).\n\
//!\n\
//! All consumers of `{name}` import this loader and nothing else; the toml\n\
//! file is the point of truth and stays code-free. Recommended wiring:\n\
//!\n\
//! ```ignore\n\
//! use serde::{{Deserialize, Serialize}};\n\
//!\n\
//! #[derive(Debug, Clone, Deserialize)]\n\
//! pub struct {entry_struct} {{\n\
//!     pub key: String,\n\
//!     pub value: String,\n\
//! }}\n\
//!\n\
//! pub fn load_{name}() -> Vec<{entry_struct}> {{\n\
//!     const RAW: &str = include_str!(\"../{data_file}\");\n\
//!     toml::from_str(RAW).expect(\"valid {name}.toml\")\n\
//! }}\n\
//! ```\n\
//!\n\
//! TODO: paste the wiring above into this file and remove this line.\n\
//! Registered in `.harna/manifest.toml` as an owned component of `{name}`.\n",
        loader_name = loader_name,
        name = name,
        data_file = data_file,
        entry_struct = entry_struct,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_dir() -> PathBuf {
        let d = std::env::temp_dir()
            .join(format!("harna-scaffold-{}-{}", std::process::id(), name_of_test()));
        // Fresh dir every run: pid reuse across runs would otherwise resurrect
        // stale manifests (scaffold refuses to overwrite existing files).
        let _ = std::fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn temp() -> PathBuf {
        tmp_dir()
    }

    fn name_of_test() -> String {
        // The test thread's name IS the test id under cargo — a unique,
        // collision-free tag (the wall-clock nanos it replaced were not
        // guaranteed distinct on Windows, so same-tick tests shared a temp
        // repo and infected each other's manifests). Rust separators do not
        // belong in paths on Windows — sanitize them.
        std::thread::current()
            .name()
            .unwrap_or("scaffold")
            .replace("::", "__")
    }

    #[test]
    fn primitive_scaffold_creates_file_and_registers() {
        let root = tmp_dir();
        let store = Store::open(&root).unwrap();
        let req = ScaffoldRequest {
            layer: "data-access".into(),
            container: "users".into(),
            name: "user-repo".into(),
            kind: ScaffoldKind::Primitive,
            path: None,
            description: "persists users".into(),
            role: None,
        };
        let res = run(&root, &store, &req).unwrap();
        assert_eq!(res.components, vec!["user-repo"]);
        let file = root.join("src/data-access/users/user-repo.rs");
        assert!(file.exists(), "file created");

        let m = store.load_manifest().unwrap();
        assert_eq!(m.layers.len(), 1);
        assert_eq!(m.layers[0].containers[0].components[0].name, "user-repo");
        assert_eq!(m.layers[0].containers[0].components[0].kind, ComponentKind::Primitive);
        // The request description is the component's rendered summary — the
        // view prose must exist as data, not vanish in the manifest rewrite.
        assert_eq!(
            m.layers[0].containers[0].components[0].summary,
            "persists users"
        );
    }

    #[test]
    fn paths_escaping_the_repo_are_refused_before_any_write() {
        let root = tmp_dir();
        let store = Store::open(&root).unwrap();
        store.init().unwrap();
        let manifest_before = std::fs::read_to_string(store.manifest_path()).unwrap();
        let req = |path: Option<&str>, container: &str, name: &str| ScaffoldRequest {
            layer: "engine".into(),
            container: container.into(),
            name: name.into(),
            kind: ScaffoldKind::Primitive,
            path: path.map(Into::into),
            description: "d".into(),
            role: None,
        };
        for bad in [
            req(Some("../../escaped"), "c", "n"),
            req(Some("/abs/escaped"), "c", "n"),
            req(Some("C:/abs/escaped"), "c", "n"),
            req(None, "../..", "n"),
            req(None, "c", "../../escaped"),
        ] {
            assert!(run(&root, &store, &bad).is_err(), "{:?} / {} / {}", bad.path, bad.container, bad.name);
        }
        assert!(scaffold_transiter(&root, &store, "a", "b", Some("../../../t".into())).is_err());
        assert_eq!(std::fs::read_to_string(store.manifest_path()).unwrap(), manifest_before);
        assert!(!root.parent().unwrap().join("escaped.rs").exists());
        // Plain nested relative paths stay allowed.
        assert!(run(&root, &store, &req(Some("src/ok/inner"), "c", "fine")).is_ok());
    }

    #[test]
    fn broken_manifest_is_refused_not_overwritten() {
        let root = tmp_dir();
        let store = Store::open(&root).unwrap();
        store.init().unwrap();
        let broken = "schema_version = 1\n[[layers]\nname = \"half-edited\"\n";
        std::fs::write(store.manifest_path(), broken).unwrap();
        let req = ScaffoldRequest {
            layer: "engine".into(),
            container: "billing".into(),
            name: "ledger".into(),
            kind: ScaffoldKind::Primitive,
            path: None,
            description: "d".into(),
            role: None,
        };
        assert!(run(&root, &store, &req).is_err());
        assert_eq!(
            std::fs::read_to_string(store.manifest_path()).unwrap(),
            broken,
            "the user's manifest keeps its bytes"
        );
        assert!(!root.join("src/engine/billing/ledger.rs").exists());
    }

    #[test]
    fn compound_scaffold_creates_folder_with_child() {
        let root = tmp_dir();
        let store = Store::open(&root).unwrap();
        let req = ScaffoldRequest {
            layer: "engine".into(),
            container: "billing".into(),
            name: "billing-engine".into(),
            kind: ScaffoldKind::Compound,
            path: None,
            description: String::new(),
            role: None,
        };
        let res = run(&root, &store, &req).unwrap();
        assert_eq!(res.components.len(), 2); // compound + starter child
        assert!(root.join("src/engine/billing/billing-engine/mod.rs").exists());
        assert!(root.join("src/engine/billing/billing-engine/billing-engine-core.rs").exists());

        let m = store.load_manifest().unwrap();
        let comp = &m.layers[0].containers[0].components[0];
        assert_eq!(comp.kind, ComponentKind::Compound);
        assert_eq!(comp.children.len(), 1);
        assert_eq!(comp.children[0].name, "billing-engine-core");
    }

    #[test]
    fn a_declared_role_stamps_the_container_and_is_validated() {
        let root = tmp_dir();
        let store = Store::open(&root).unwrap();
        let req = ScaffoldRequest {
            layer: "engines".into(),
            container: "billing".into(),
            name: "billing-motor".into(),
            kind: ScaffoldKind::Primitive,
            path: None,
            description: "sync billing".into(),
            role: Some("engine".into()),
        };
        let res = run(&root, &store, &req).unwrap();
        assert!(res.message.contains("container role: engine"), "the stamp is read aloud");
        let m = store.load_manifest().unwrap();
        assert_eq!(
            m.layers[0].containers[0].role.as_deref(),
            Some("engine"),
            "the request's role is manifest data"
        );
        // An unknown role is refused with the vocabulary named (data, not
        // invented).
        let bad = ScaffoldRequest {
            role: Some("library".into()),
            ..req.clone()
        };
        let err = run(&root, &store, &bad).unwrap_err().to_string();
        assert!(err.contains("unknown role `library`"), "was: {err}");
        assert!(err.contains("orchestration-async"), "the vocabulary is in the hint");
    }

    #[test]
    fn duplicate_component_name_is_rejected() {
        let root = tmp_dir();
        let store = Store::open(&root).unwrap();
        let req = |path: Option<String>| ScaffoldRequest {
            layer: "data-access".into(),
            container: "users".into(),
            name: "user-repo".into(),
            kind: ScaffoldKind::Primitive,
            path: path.clone(),
            description: String::new(),
            role: None,
        };
        run(&root, &store, &req(None)).unwrap();
        assert!(run(&root, &store, &req(None)).is_err());
    }

    #[test]
    fn data_scaffold_creates_source_and_loader_pair() {
        let tmp = temp();
        let store = Store::open(&tmp).unwrap();
        let req = ScaffoldRequest {
            layer: "api".into(),
            container: "timezones".into(),
            name: "timezones".into(),
            kind: ScaffoldKind::Data,
            path: None,
            description: "every timezone the calendar supports".into(),
            role: None,
        };
        let res = run(&tmp, &store, &req).unwrap();
        assert_eq!(res.components, vec!["timezones", "timezones-loader"]);

        let toml_text = std::fs::read_to_string(tmp.join("src/api/timezones/timezones.toml")).unwrap();
        assert!(toml_text.contains("[[entries]]"));
        let loader = std::fs::read_to_string(tmp.join("src/api/timezones/timezones-loader.rs")).unwrap();
        assert!(loader.contains("include_str!"));
        assert!(loader.contains("constants-are-data") == false, "principle lives in rules, not code");

        // Manifest registers both, loader owned by the data component.
        let m = store.load_manifest().unwrap();
        let comp = &m.layers[0].containers[0].components[0];
        assert_eq!(comp.name, "timezones");
        assert_eq!(comp.children[0].name, "timezones-loader");
        // Boot order includes both.
        assert!(m.layers[0].containers[0].sequence.contains(&"timezones-loader".into()));
    }

    #[test]
    fn transiter_goes_to_transit_layer() {
        let root = tmp_dir();
        let store = Store::open(&root).unwrap();
        let res = scaffold_transiter(&root, &store, "api", "billing", None).unwrap();
        assert!(res.components[0].starts_with("transiter-api-to-billing"));
        let m = store.load_manifest().unwrap();
        assert_eq!(m.layers[0].name, "transit");
        // The generated container declares the corridor it bridges.
        let corridor = &m.layers[0].containers[0];
        assert_eq!(corridor.connects, vec!["api".to_string(), "billing".to_string()]);
    }

    #[test]
    fn transiter_connects_stamp_never_clobbers_hand_edits() {
        let root = tmp_dir();
        let store = Store::open(&root).unwrap();
        // Scaffold once, then hand-edit `connects` to reflect a deliberate
        // third leg; a second scaffold over the same container keeps it.
        scaffold_transiter(&root, &store, "api", "billing", None).unwrap();
        let mut m = store.load_manifest().unwrap();
        m.layers[0].containers[0].connects = vec!["api".into(), "billing".into(), "auth".into()];
        store.save_manifest(&m).unwrap();
        scaffold_transiter(&root, &store, "api", "billing", Some("transiter-again".into())).unwrap();
        let m = store.load_manifest().unwrap();
        let corridor = &m.layers[0].containers[0];
        assert_eq!(corridor.connects.len(), 3, "hand-edited corridor stays");
        assert_eq!(
            corridor.connects[2],
            "auth".to_string(),
            "the hand-written entry survives the scaffold"
        );
        // And the second scaffolded component still registered itself.
        assert!(corridor.components.iter().any(|c| c.name == "transiter-again"));
    }
}
