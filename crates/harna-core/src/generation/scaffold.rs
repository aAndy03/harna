//! Scaffolding: generate primitive/compound components from templates so
//! structure is born correct, and register it in the manifest in the same
//! transaction. The agent never hand-builds structure — it calls this.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::schema::manifest::{Component, ComponentKind, Container, Layer, Manifest};
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
    store.init()?;
    let mut manifest = store.load_manifest().unwrap_or_default();

    // Reject duplicates before taking any mutable borrow of the manifest.
    if find_component(&manifest, &req.name).is_some() {
        anyhow::bail!("component `{}` already exists in the manifest", req.name);
    }

    let layer = find_or_create_layer(&mut manifest, &req.layer);
    let container = find_or_create_container(layer, &req.container);

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
    let message = if seq_was_empty && seq_len == 1 {
        format!(
            "created {} and registered {} in the manifest (boot sequence initialized)",
            files.join(", "),
            components.join(", ")
        )
    } else {
        format!(
            "created {} and registered {} in the manifest",
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
    };
    let mut result = run(root, store, &req)?;
    // Mark the generated file with the transiter role.
    result.components.clear();
    result.components.push(req.name);
    Ok(result)
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
    }
}
