//! `edit` — typed manifest edits for what scaffold and `harna move` do not
//! cover (0.3.42, from the font-manager test): renaming a container, and
//! setting a layer's `depends_on`/role or a container's path/gateway/role.
//! The agent there hand-edited `manifest.toml` for every one of these; a
//! hand edit is where the manifest breaks (see lesson `regex-edit-manifest`).
//!
//! Every edit is checked against the manifest before it is saved: unknown
//! names, duplicate names, unknown roles and self/unknown `depends_on`
//! targets are refused with the reason. Each returns the change lines it made.

use anyhow::{bail, Context};

use crate::schema::model::manifest::{is_known_role, role_catalog, Container, Layer, Manifest};
use crate::schema::store::Store;

/// A container field update; `None` leaves a field as it is, `Some("")`
/// clears an optional one.
#[derive(Debug, Default, Clone)]
pub struct ContainerSet {
    pub path: Option<String>,
    pub gateway: Option<String>,
    pub role: Option<String>,
    pub description: Option<String>,
    /// Owned capabilities/markers (replaces the list; empty clears).
    pub owns: Option<Vec<String>>,
    /// Escape hatches out of them (replaces the list; empty clears).
    pub escapes: Option<Vec<String>>,
}

/// A layer field update (same convention as [`ContainerSet`]).
#[derive(Debug, Default, Clone)]
pub struct LayerSet {
    pub depends_on: Option<Vec<String>>,
    pub role: Option<String>,
    pub description: Option<String>,
}

/// Rename a container and every reference to it (its layer's `sequence`).
pub fn rename_container(m: &mut Manifest, old: &str, new: &str) -> anyhow::Result<Vec<String>> {
    let new = new.trim();
    if new.is_empty() {
        bail!("the new container name is empty");
    }
    if find_container(m, new).is_some() {
        bail!("a container named `{new}` already exists — names are unique across the manifest");
    }
    let (li, ci) = find_container(m, old).with_context(|| unknown("container", old))?;
    let layer = &mut m.layers[li];
    layer.containers[ci].name = new.to_string();
    let mut changes = vec![format!(
        "container `{old}` -> `{new}` (layer {})",
        layer.name
    )];
    for s in layer.sequence.iter_mut().filter(|s| s.as_str() == old) {
        *s = new.to_string();
        changes.push(format!("layer {} sequence relinked", layer.name));
    }
    Ok(changes)
}

/// Register an existing file as a component of `container` (0.3.45): the
/// step `adopt`'s refresh proposes, without a hand edit. The name defaults
/// to the file stem in kebab case (made unique with the parent folder on a
/// clash); the tests are the ones that exist; the container's sequence,
/// when it keeps one, gains the name.
pub fn register_component(
    root: &std::path::Path,
    m: &mut Manifest,
    container: &str,
    path: &str,
    name: Option<&str>,
    summary: Option<&str>,
) -> anyhow::Result<Vec<String>> {
    let path = path.trim().replace('\\', "/");
    if !root.join(&path).is_file() {
        bail!("`{path}` is not a file in the repository");
    }
    let taken: Vec<String> = m
        .layers
        .iter()
        .flat_map(|l| &l.containers)
        .flat_map(|c| &c.components)
        .flat_map(|c| std::iter::once(c).chain(c.children.iter()))
        .map(|c| c.name.clone())
        .collect();
    if m.layers
        .iter()
        .flat_map(|l| &l.containers)
        .flat_map(|c| &c.components)
        .any(|c| c.path == path)
    {
        bail!("`{path}` is already registered");
    }
    let stem = path
        .rsplit('/')
        .next()
        .unwrap_or(&path)
        .trim_end_matches(".rs")
        .replace('_', "-");
    let name = match name {
        Some(n) => n.trim().to_string(),
        None if !taken.contains(&stem) => stem,
        None => {
            let parent = path.rsplit('/').nth(1).unwrap_or("").replace('_', "-");
            format!("{parent}-{stem}")
        }
    };
    if taken.contains(&name) {
        bail!("a component named `{name}` already exists — names are unique across the manifest; pass --name");
    }
    let (li, ci) = find_container(m, container).with_context(|| unknown("container", container))?;
    let c: &mut Container = &mut m.layers[li].containers[ci];
    let tests: Vec<String> = super::linkage::tests_of(root, &path).into_iter().collect();
    c.components
        .push(crate::schema::model::manifest::Component {
            name: name.clone(),
            kind: crate::schema::model::manifest::ComponentKind::Primitive,
            path: path.clone(),
            summary: summary.unwrap_or_default().trim().to_string(),
            tests: tests.clone(),
            provides: Vec::new(),
            children: Vec::new(),
        });
    if !c.sequence.is_empty() {
        c.sequence.push(name.clone());
    }
    let mut changes = vec![format!(
        "component `{name}` registered in `{container}`: {path}"
    )];
    if !tests.is_empty() {
        changes.push(format!("  tests: {}", tests.join(", ")));
    }
    if summary.is_none() {
        changes.push("  no summary yet — add one so place/plan can explain it".into());
    }
    Ok(changes)
}

/// Add or remove registered tests (0.3.49, ledger field test: integration
/// tests in `crates/x/tests/` cover many components, and the only way to
/// say so was a hand edit of ~28 components). `components` empty = every
/// component of the container (children included). Added paths must exist.
pub fn set_tests(
    root: &std::path::Path,
    m: &mut Manifest,
    container: &str,
    components: &[String],
    add: &[String],
    remove: &[String],
) -> anyhow::Result<Vec<String>> {
    let add: Vec<String> = add.iter().map(|p| p.trim().replace('\\', "/")).collect();
    let remove: Vec<String> = remove.iter().map(|p| p.trim().replace('\\', "/")).collect();
    for p in &add {
        if !root.join(p).exists() {
            bail!("`{p}` is not in the repository");
        }
    }
    let (li, ci) = find_container(m, container).with_context(|| unknown("container", container))?;
    let c: &mut Container = &mut m.layers[li].containers[ci];
    let mut changes = Vec::new();
    let mut matched = Vec::new();
    let mut stack: Vec<&mut crate::schema::model::manifest::Component> =
        c.components.iter_mut().collect();
    while let Some(comp) = stack.pop() {
        if components.is_empty() || components.contains(&comp.name) {
            matched.push(comp.name.clone());
            let before = comp.tests.clone();
            comp.tests.retain(|t| !remove.contains(t));
            for p in &add {
                if !comp.tests.contains(p) {
                    comp.tests.push(p.clone());
                }
            }
            if comp.tests != before {
                changes.push(format!(
                    "{}: tests = [{}]",
                    comp.name,
                    comp.tests.join(", ")
                ));
            }
        }
        stack.extend(comp.children.iter_mut());
    }
    if let Some(missing) = components.iter().find(|n| !matched.contains(n)) {
        bail!("no component `{missing}` in container `{container}`");
    }
    changes.sort();
    Ok(changes)
}

pub fn set_container(
    m: &mut Manifest,
    name: &str,
    set: &ContainerSet,
) -> anyhow::Result<Vec<String>> {
    let (li, ci) = find_container(m, name).with_context(|| unknown("container", name))?;
    check_role(set.role.as_deref())?;
    let c: &mut Container = &mut m.layers[li].containers[ci];
    let mut changes = Vec::new();
    if let Some(p) = &set.path {
        let p = p.trim().trim_end_matches('/').replace('\\', "/");
        changes.push(format!("container {name}: path `{}` -> `{p}`", c.path));
        c.path = p;
    }
    if let Some(g) = &set.gateway {
        changes.push(field_change(name, "gateway", c.gateway.as_deref(), g));
        c.gateway = optional(g);
    }
    if let Some(r) = &set.role {
        changes.push(field_change(name, "role", c.role.as_deref(), r));
        c.role = optional(r);
    }
    if let Some(d) = &set.description {
        c.description = d.trim().to_string();
        changes.push(format!("container {name}: description set"));
    }
    if let Some(o) = &set.owns {
        changes.push(format!(
            "container {name}: owns [{}] -> [{}]",
            c.owns.join(", "),
            o.join(", ")
        ));
        c.owns = o.clone();
    }
    if let Some(e) = &set.escapes {
        changes.push(format!(
            "container {name}: escapes [{}] -> [{}]",
            c.escapes.join(", "),
            e.join(", ")
        ));
        c.escapes = e.clone();
    }
    Ok(changes)
}

pub fn set_layer(m: &mut Manifest, name: &str, set: &LayerSet) -> anyhow::Result<Vec<String>> {
    let li = m
        .layers
        .iter()
        .position(|l| l.name == name)
        .with_context(|| unknown("layer", name))?;
    check_role(set.role.as_deref())?;
    if let Some(deps) = &set.depends_on {
        for d in deps {
            if d == name {
                bail!("layer `{name}` cannot depend on itself");
            }
            if !m.layers.iter().any(|l| &l.name == d) {
                bail!("{}", unknown("layer", d));
            }
        }
    }
    let l: &mut Layer = &mut m.layers[li];
    let mut changes = Vec::new();
    if let Some(deps) = &set.depends_on {
        changes.push(format!(
            "layer {name}: depends_on [{}] -> [{}]",
            l.depends_on.join(", "),
            deps.join(", ")
        ));
        l.depends_on = deps.clone();
    }
    if let Some(r) = &set.role {
        changes.push(field_change(name, "role", l.role.as_deref(), r));
        l.role = optional(r);
    }
    if let Some(d) = &set.description {
        l.description = d.trim().to_string();
        changes.push(format!("layer {name}: description set"));
    }
    Ok(changes)
}

/// Load, edit, save — the CLI/MCP entry point. Nothing is written when the
/// edit is refused or changes nothing.
pub fn apply(
    store: &Store,
    edit: impl FnOnce(&mut Manifest) -> anyhow::Result<Vec<String>>,
) -> anyhow::Result<String> {
    let mut m = store.load_manifest()?;
    let changes = edit(&mut m)?;
    if changes.is_empty() {
        return Ok("nothing to change (pass at least one field)\n".into());
    }
    store.save_manifest(&m)?;
    let mut out = changes.join("\n");
    out.push_str("\nmanifest saved — run `harna validate` to confirm the new shape holds\n");
    Ok(out)
}

/// A comma list as the CLI passes it; empty means "clear".
pub fn split_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn find_container(m: &Manifest, name: &str) -> Option<(usize, usize)> {
    m.layers.iter().enumerate().find_map(|(li, l)| {
        l.containers
            .iter()
            .position(|c| c.name == name)
            .map(|ci| (li, ci))
    })
}

fn check_role(role: Option<&str>) -> anyhow::Result<()> {
    match role.map(str::trim) {
        Some(r) if !r.is_empty() && !is_known_role(r) => {
            bail!("unknown role `{r}` — known roles:\n{}", role_catalog())
        }
        _ => Ok(()),
    }
}

fn optional(v: &str) -> Option<String> {
    let v = v.trim();
    (!v.is_empty()).then(|| v.to_string())
}

fn field_change(owner: &str, field: &str, old: Option<&str>, new: &str) -> String {
    let new = new.trim();
    let new = if new.is_empty() { "(cleared)" } else { new };
    format!("{owner}: {field} `{}` -> `{new}`", old.unwrap_or("-"))
}

fn unknown(kind: &str, name: &str) -> String {
    format!("no {kind} named `{name}` in the manifest (`harna onboarding` lists them)")
}

#[cfg(test)]
mod tests;
