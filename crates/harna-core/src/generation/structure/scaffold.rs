//! Scaffolding: generate primitive/compound components from templates so
//! structure is born correct, and register it in the manifest in the same
//! transaction. The agent never hand-builds structure — it calls this.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::schema::model::manifest::{
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
    /// [`crate::schema::model::manifest::ROLES`] vocabulary — stamped on the
    /// container (created or existing). None = leave the role as-is.
    #[serde(default)]
    pub role: Option<String>,
    /// The doc header only, no starter stub (0.4.10).
    #[serde(default)]
    pub empty: bool,
    /// Create the declared container's crate first: "lib" or "bin" (0.4.10).
    #[serde(default)]
    pub new_crate: Option<String>,
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

/// The directory a new container is born in, when no --path says it
/// (0.3.50, household field test: in a Cargo workspace the old default
/// `src/<layer>/<container>` created a stray tree at the repo root).
/// - beside the layer's other containers, in their crate's source root;
/// - in a workspace with no root package: refused, naming the --path to
///   pass once (later components of the container follow it);
/// - otherwise the single-crate convention `src/<layer>/<container>`.
fn default_container_dir(
    root: &Path,
    manifest: &crate::schema::model::manifest::Manifest,
    req: &ScaffoldRequest,
) -> anyhow::Result<String> {
    use super::linkage::module_name;
    let index = crate::search::digest::crates::CrateIndex::load(root);
    // The container is declared (a profile's shape) but its crate does not
    // exist yet (0.4.9 field test: the error said "no container in a
    // crate yet" about a container the manifest declared).
    if let Some(c) = manifest
        .layers
        .iter()
        .flat_map(|l| &l.containers)
        .find(|c| c.name == req.container && !c.path.is_empty())
    {
        if req.path.is_none()
            && root.join("Cargo.toml").exists()
            && index.locate_dir(&c.path).is_none()
        {
            anyhow::bail!(
                "container `{}` is declared at `{}`, but no crate holds that path yet — pass --new-crate lib (or bin) and scaffold creates it (Cargo.toml, workspace member, src/lib.rs or main.rs), or create it by hand",
                c.name,
                c.path
            );
        }
    }
    let sibling_root = manifest
        .layers
        .iter()
        .filter(|l| l.name == req.layer)
        .flat_map(|l| &l.containers)
        .filter(|c| !c.path.is_empty())
        .find_map(|c| {
            index
                .locate_dir(&c.path)
                .map(|(info, _)| info.src_root.clone())
        });
    if let Some(src) = sibling_root {
        let src = if src == "." {
            String::new()
        } else {
            format!("{src}/")
        };
        return Ok(format!("{src}{}", module_name(&req.container)));
    }
    let root_package = index.crates.iter().any(|c| c.dir == ".");
    if req.path.is_none() && !index.crates.is_empty() && !root_package {
        // never another layer's crate as the example (0.4.9 field test)
        let example = "crates/<crate>/src";
        anyhow::bail!(
            "layer `{}` has no container in a crate yet and this is a Cargo workspace — pass --path once for the first component (e.g. --path {example}/{}/{}); the container's later components follow it",
            req.layer,
            module_name(&req.container),
            module_name(&req.name)
        );
    }
    Ok(format!(
        "src/{}/{}",
        module_name(&req.layer),
        module_name(&req.container)
    ))
}

/// A module named like its parent (`budget/budget.rs`) is clippy's
/// `module_inception` (0.3.49, ledger field test: scaffold produced it
/// and clippy -D warnings failed). Refuse before writing.
fn refuse_inception(base: &str) -> anyhow::Result<()> {
    let base = base.trim_end_matches('/').trim_end_matches(".rs");
    let Some((dir, leaf)) = base.rsplit_once('/') else {
        return Ok(());
    };
    let parent = dir.rsplit('/').next().unwrap_or(dir);
    if !leaf.is_empty() && leaf == parent {
        anyhow::bail!(
            "`{base}` would be module `{parent}::{leaf}` — clippy's module_inception; name the component for its concern (e.g. `{leaf}-model`), or put the code in the container's own file"
        );
    }
    Ok(())
}

/// Scaffold a component and register it in the manifest.
pub fn run(root: &Path, store: &Store, req: &ScaffoldRequest) -> anyhow::Result<ScaffoldResult> {
    if req.name.is_empty() {
        anyhow::bail!("component name must not be empty");
    }
    if let Some(role) = &req.role {
        if !is_known_role(role) {
            anyhow::bail!(
                "unknown role `{role}` — the vocabulary is data: {}",
                role_catalog()
            );
        }
    }
    store.init()?;
    let born_crate = match &req.new_crate {
        Some(kind) => {
            // the crate's place: the declared container's path, else the
            // --path the new component is given (0.5.9 field test: the old
            // hint named a `container set` that cannot create a container)
            let m = store.load_manifest()?;
            let at = m
                .layers
                .iter()
                .flat_map(|l| &l.containers)
                .find(|c| c.name == req.container && !c.path.is_empty())
                .map(|c| c.path.clone())
                .or_else(|| req.path.clone())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "--new-crate needs to know where the crate goes: declare container `{}` with a path, or pass --path crates/<crate>/src/<name>",
                        req.container
                    )
                })?;
            let dir = skeleton::crate_dir_of(&at).ok_or_else(|| {
                anyhow::anyhow!("path `{at}` has no `src` segment — no crate to create")
            })?;
            skeleton::create(root, &dir, kind)?
        }
        None => Vec::new(),
    };
    // init guarantees a manifest exists, so a load failure is a broken or
    // newer-schema file — never default it: saving would overwrite the
    // user's whole architecture with just this one component.
    let mut manifest = store
        .load_manifest()
        .map_err(|e| e.context("scaffold refuses to rewrite an unreadable manifest"))?;

    // A planned slot of this name is filled (0.4.1): its declared path is
    // where the code goes, and the slot gives way to the real component.
    // A registered component whose file is gone is restored the same way
    // (0.4.3: validate said "run harna scaffold", scaffold said "exists").
    let slot_path = match find_component(&manifest, &req.name) {
        Some(c) if c.planned => Some(c.path.clone()),
        Some(c) if !c.path.is_empty() && !root.join(&c.path).exists() => Some(c.path.clone()),
        _ => None,
    };
    let filled;
    // What the slot or registered entry already carried survives the fill
    // (0.4.9): its place, summary, tests and declared surface.
    let mut prior: Option<Component> = None;
    let req = if let Some(p) = slot_path {
        let mut home = (req.layer.clone(), req.container.clone());
        for l in manifest.layers.iter_mut() {
            for c in l.containers.iter_mut() {
                if let Some(at) = c.components.iter().position(|k| k.name == req.name) {
                    prior = Some(c.components.remove(at));
                    home = (l.name.clone(), c.name.clone());
                }
            }
        }
        let base = match req.kind {
            ScaffoldKind::Compound => p.trim_end_matches("/mod.rs").to_string(),
            _ => p.trim_end_matches(".rs").to_string(),
        };
        filled = ScaffoldRequest {
            path: req.path.clone().or(Some(base)),
            layer: home.0,
            container: home.1,
            description: if req.description.is_empty() {
                prior
                    .as_ref()
                    .map(|c| c.summary.clone())
                    .unwrap_or_default()
            } else {
                req.description.clone()
            },
            ..req.clone()
        };
        &filled
    } else {
        req
    };

    // Reject duplicates before taking any mutable borrow of the manifest.
    if find_component(&manifest, &req.name).is_some() {
        anyhow::bail!(
            "component `{}` already exists in the manifest — component names are unique across the whole manifest, not per container; pick a distinct one (e.g. `{}-{}`)",
            req.name,
            req.container,
            req.name
        );
    }

    // Where a new container's files go, decided before the manifest is
    // borrowed for edits (0.3.50: the Cargo layout, not a root `src/`).
    let default_dir = default_container_dir(root, &manifest, req)?;

    let layer = find_or_create_layer(&mut manifest, &req.layer);
    let container = find_or_create_container(layer, &req.container);
    if let Some(role) = &req.role {
        container.role = Some(role.clone());
    }

    // Default path: <container.path>/<name>[...]. Generated path segments
    // are Rust module names (0.3.38): `user-repo` is born `user_repo.rs`,
    // so the file can be declared as `mod user_repo;` and compile.
    use super::linkage::module_name;
    let container_dir = if container.path.is_empty() {
        default_dir
    } else {
        container.path.clone()
    };
    let base = req
        .path
        .clone()
        .unwrap_or_else(|| format!("{}/{}", container_dir, module_name(&req.name)));
    check_inside_repo(&base)?;
    if !matches!(req.kind, ScaffoldKind::Data) {
        refuse_inception(&base)?;
    }

    // A new container is born with its path (0.3.42, font-manager: every
    // scaffolded container had `path = ""`, so the gateway and boundary
    // checks — which govern files by container path — saw nothing). The
    // default directory, or the explicit path's parent directory.
    let path_note = if container.path.is_empty() {
        let dir = match &req.path {
            None => container_dir.clone(),
            Some(_) => base
                .trim_end_matches('/')
                .rsplit_once('/')
                .map(|(d, _)| d.to_string())
                .unwrap_or_default(),
        };
        if dir.is_empty() {
            String::new()
        } else {
            container.path = dir.clone();
            format!(" (container {} path: {dir})", req.container)
        }
    } else {
        String::new()
    };

    let mut components = Vec::new();
    let mut files = Vec::new();

    let wire_target: Option<String> = match req.kind {
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
            // the header names where the file really lives (0.3.49: it
            // named the default directory even under --path)
            let home = file_rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            fs::write(
                &abs,
                if req.empty {
                    empty_template(&req.name, &req.description, home)
                } else {
                    primitive_template(&req.name, &req.description, home)
                },
            )?;
            files.push(file_rel.clone());
            components.push(req.name.clone());
            container.components.push(Component {
                planned: false,
                name: req.name.clone(),
                kind: ComponentKind::Primitive,
                path: file_rel.clone(),
                summary: req.description.clone(),
                // No stub test is registered as coverage (0.3.50, household
                // field test: the fake test counted, then went stale); the
                // real tests are registered when written (`container tests`).
                tests: Vec::new(),
                provides: Vec::new(),
                children: Vec::new(),
            });
            Some(file_rel.clone())
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
            let child_file = format!("{}/{}.rs", dir_rel, module_name(&child_name));
            fs::write(
                root.join(&child_file),
                if req.empty {
                    empty_template(&child_name, &req.description, &dir_rel)
                } else {
                    primitive_template(&child_name, &req.description, &dir_rel)
                },
            )?;
            fs::write(
                root.join(format!("{}/mod.rs", dir_rel)),
                compound_mod_template(&req.name, &child_name, &req.description),
            )?;
            files.push(format!("{}/mod.rs", dir_rel));
            files.push(child_file.clone());
            container.components.push(Component {
                planned: false,
                name: req.name.clone(),
                kind: ComponentKind::Compound,
                path: format!("{}/mod.rs", dir_rel),
                summary: req.description.clone(),
                tests: Vec::new(),
                provides: Vec::new(),
                children: vec![Component {
                    planned: false,
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
            Some(format!("{}/mod.rs", dir_rel))
        }
        ScaffoldKind::Data => {
            // Constants-are-data: the value-set lives in a .toml; code reads
            // it through one loader. The toml is the single source of truth.
            let data_file = format!("{base}.toml");
            let loader_file = format!("{base}_loader.rs");
            let loader_name = format!("{}-loader", req.name);
            let abs_toml = root.join(&data_file);
            let abs_loader = root.join(&loader_file);
            if abs_toml.exists() || abs_loader.exists() {
                anyhow::bail!(
                    "refusing to overwrite existing file: {}",
                    if abs_toml.exists() {
                        &data_file
                    } else {
                        &loader_file
                    }
                );
            }
            if let Some(parent) = abs_toml.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&abs_toml, data_toml_template(&req.name, &req.description))?;
            fs::write(
                &abs_loader,
                data_loader_template(&req.name, &loader_name, &data_file),
            )?;
            files.push(data_file.clone());
            files.push(loader_file.clone());
            components.push(req.name.clone());
            components.push(loader_name.clone());
            container.components.push(Component {
                planned: false,
                name: req.name.clone(),
                kind: ComponentKind::Primitive,
                path: data_file.clone(),
                summary: req.description.clone(),
                tests: Vec::new(),
                provides: Vec::new(),
                children: vec![Component {
                    planned: false,
                    name: loader_name,
                    kind: ComponentKind::Primitive,
                    path: loader_file.clone(),
                    summary: req.description.clone(),
                    tests: Vec::new(),
                    provides: Vec::new(),
                    children: Vec::new(),
                }],
            });
            Some(loader_file.clone())
        }
    };

    // Boot order is opt-in (0.3.50): a container that declares a sequence
    // gets new components appended to it; one that never did is not given
    // one (the ledger field test's 19 auto-filled sequences read as
    // ceremony nothing depended on).
    if !container.sequence.is_empty() {
        for name in &components {
            if !container.sequence.contains(name) {
                container.sequence.push(name.clone());
            }
        }
    }
    if let Some(old) = &prior {
        if let Some(c) = container.components.iter_mut().find(|c| c.name == req.name) {
            if c.tests.is_empty() {
                c.tests = old.tests.clone();
            }
            if c.provides.is_empty() {
                c.provides = old.provides.clone();
            }
            if c.summary.is_empty() {
                c.summary = old.summary.clone();
            }
        }
    }
    // The borrow ends here; nothing to drop — a reference drop is a no-op.
    let _ = container;

    store.save_manifest(&manifest)?;

    // Linkage (0.3.38): the new module is declared up its parent chain to
    // the crate root, so it compiles the moment it exists.
    let wiring = match &wire_target {
        Some(t) => super::linkage::wire_module(root, t)?,
        None => super::linkage::Wiring::default(),
    };

    // The role stamp goes into the message: the declaration is a decision,
    // and decisions are read aloud (adopted roles are visible at a glance).
    let role_note = req
        .role
        .as_deref()
        .map(|r| format!(" (container role: {r})"))
        .unwrap_or_default();
    let message = format!(
        "created {} and registered {} in the manifest{role_note}",
        files.join(", "),
        components.join(", ")
    );
    let message = format!("{message}{path_note}; {}", wiring.render());
    let message = if born_crate.is_empty() {
        message
    } else {
        format!("new crate: {}; {message}", born_crate.join(", "))
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
        empty: false,
        new_crate: None,
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
pub(super) fn check_inside_repo(rel: &str) -> anyhow::Result<()> {
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
        anyhow::bail!(
            "scaffold path `{rel}` must be repo-relative with no `..` (it stays inside the repo)"
        );
    }
    Ok(())
}

fn find_or_create_layer<'m>(manifest: &'m mut Manifest, name: &str) -> &'m mut Layer {
    if let Some(pos) = manifest.layers.iter().position(|l| l.name == name) {
        return &mut manifest.layers[pos];
    }
    manifest.layers.push(Layer {
        owners: Vec::new(),
        isolated: false,
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
        owners: Vec::new(),
        escapes: Vec::new(),
        owns: Vec::new(),
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

mod skeleton;
mod templates;
use templates::*;

#[cfg(test)]
mod tests;
