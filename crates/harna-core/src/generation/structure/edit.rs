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
    /// The declared surface (0.3.50): the names other containers may
    /// import; everything else stays private (replaces; empty clears).
    pub provides: Option<Vec<String>>,
    /// Who answers for it (0.7.0; replaces; empty clears).
    pub owners: Option<Vec<String>>,
}

/// A layer field update (same convention as [`ContainerSet`]).
#[derive(Debug, Default, Clone)]
pub struct LayerSet {
    pub depends_on: Option<Vec<String>>,
    pub role: Option<String>,
    pub description: Option<String>,
    /// Sibling isolation (0.4.2): containers of the layer may not import
    /// each other.
    pub isolated: Option<bool>,
    /// Who answers for it (0.7.0; replaces; empty clears).
    pub owners: Option<Vec<String>>,
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
            planned: false,
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

/// Take a container out of the manifest (0.3.50, household field test: a
/// stray scaffold could only be undone by hand-editing the manifest). The
/// files stay — deleting code is the user's act, not the map's; its layer's
/// sequence forgets it.
pub fn remove_container(m: &mut Manifest, name: &str) -> anyhow::Result<Vec<String>> {
    let (li, ci) = find_container(m, name).with_context(|| unknown("container", name))?;
    let layer = &mut m.layers[li];
    let c = layer.containers.remove(ci);
    layer.sequence.retain(|s| s != name);
    Ok(vec![format!(
        "container `{name}` removed from layer `{}` ({} component(s) unregistered; files under `{}` untouched)",
        layer.name,
        c.components.len(),
        if c.path.is_empty() { "-" } else { &c.path }
    )])
}

/// Take a layer out of the manifest, with its containers; layers that
/// declared it in `depends_on` drop it. Files stay.
/// Declare a container (0.6.0): a name, the layer it sits in (created
/// when new), the folder it governs — any language, no files written.
/// TypeScript containers had no way in: scaffold writes Rust.
pub fn add_container(
    m: &mut Manifest,
    layer: &str,
    name: &str,
    path: &str,
    description: Option<&str>,
) -> anyhow::Result<Vec<String>> {
    if name.trim().is_empty() || layer.trim().is_empty() {
        anyhow::bail!("a container needs a name and a layer");
    }
    if crate::schema::model::scope::normalize_scope(path).is_empty() {
        anyhow::bail!("container `{name}` needs a folder, not the repository root");
    }
    if m.layers
        .iter()
        .flat_map(|l| &l.containers)
        .any(|c| c.name == name)
    {
        anyhow::bail!(
            "container `{name}` already exists (harna container set {name} ... edits it)"
        );
    }
    let path = crate::schema::model::scope::normalize_scope(path);
    let mut out = Vec::new();
    if !m.layers.iter().any(|l| l.name == layer) {
        m.layers.push(crate::schema::model::manifest::Layer {
            name: layer.into(),
            ..Default::default()
        });
        out.push(format!(
            "layer `{layer}` created (no depends_on yet: harna layer set {layer} --depends-on ...)"
        ));
    }
    let l = m
        .layers
        .iter_mut()
        .find(|l| l.name == layer)
        .expect("just ensured");
    l.containers
        .push(crate::schema::model::manifest::Container {
            name: name.into(),
            path: path.clone(),
            description: description.unwrap_or("").into(),
            ..Default::default()
        });
    out.push(format!(
        "container `{name}` declared in `{layer}` at `{path}` (files untouched)"
    ));
    Ok(out)
}

/// Take a component (and its children) out of the manifest; its files
/// stay (0.5.9 field test: a deleted file left a blocking manifest-path
/// error and nothing to unregister it with).
pub fn unregister_component(m: &mut Manifest, name: &str) -> anyhow::Result<Vec<String>> {
    fn take(cs: &mut Vec<crate::schema::model::manifest::Component>, name: &str) -> Option<String> {
        if let Some(at) = cs.iter().position(|c| c.name == name) {
            return Some(cs.remove(at).path);
        }
        cs.iter_mut().find_map(|c| take(&mut c.children, name))
    }
    for c in m.layers.iter_mut().flat_map(|l| l.containers.iter_mut()) {
        if let Some(path) = take(&mut c.components, name) {
            c.sequence.retain(|s| s != name);
            return Ok(vec![format!(
                "component `{name}` unregistered from `{}` ({path} untouched on disk)",
                c.name
            )]);
        }
    }
    Err(anyhow::anyhow!("{}", unknown("component", name)))
}

pub fn remove_layer(m: &mut Manifest, name: &str) -> anyhow::Result<Vec<String>> {
    let li = m
        .layers
        .iter()
        .position(|l| l.name == name)
        .with_context(|| unknown("layer", name))?;
    let l = m.layers.remove(li);
    let mut changes = vec![format!(
        "layer `{name}` removed ({} container(s) unregistered; files untouched)",
        l.containers.len()
    )];
    for other in &mut m.layers {
        if other.depends_on.iter().any(|d| d == name) {
            other.depends_on.retain(|d| d != name);
            changes.push(format!(
                "layer `{}`: depends_on no longer lists `{name}`",
                other.name
            ));
        }
    }
    // test kinds reach layers too (0.4.9)
    for k in &mut m.test_kinds {
        if k.reach.iter().any(|r| r == name) {
            k.reach.retain(|r| r != name);
            changes.push(format!(
                "test kind `{}`: reach no longer lists `{name}`",
                k.name
            ));
        }
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
        if !p.trim().is_empty() && crate::schema::model::scope::normalize_scope(p).is_empty() {
            anyhow::bail!(
                "container `{name}`: the repository root is not a container path (name its folder)"
            );
        }
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
    if let Some(p) = &set.provides {
        changes.push(format!(
            "container {name}: provides [{}] -> [{}]",
            c.provides.join(", "),
            p.join(", ")
        ));
        c.provides = p.clone();
    }
    if let Some(o) = &set.owners {
        if let Some(bad) = o
            .iter()
            .find(|h| h.trim().is_empty() || h.chars().any(char::is_whitespace))
        {
            anyhow::bail!("owner `{bad}` is not a handle — use @team, @user or a mail, no spaces");
        }
        changes.push(format!(
            "container {name}: owners [{}] -> [{}]",
            c.owners.join(", "),
            o.join(", ")
        ));
        c.owners = o.clone();
    }
    Ok(changes)
}

pub fn set_layer(m: &mut Manifest, name: &str, set: &LayerSet) -> anyhow::Result<Vec<String>> {
    let li = m
        .layers
        .iter()
        .position(|l| l.name == name)
        // a layer is born with its first container (1.0.1, proof: agents
        // tried `layer set` first and met only "no layer named")
        .with_context(|| {
            format!(
                "{} — a layer is born with its first container: `harna container add <name> --layer {name} --path <dir>` (or `harna scaffold`)",
                unknown("layer", name)
            )
        })?;
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
    if let Some(i) = set.isolated {
        if l.isolated != i {
            l.isolated = i;
            changes.push(format!(
                "layer {name}: isolated -> {i}{}",
                if i {
                    " (its containers may not import each other)"
                } else {
                    ""
                }
            ));
        }
    }
    if let Some(d) = &set.description {
        l.description = d.trim().to_string();
        changes.push(format!("layer {name}: description set"));
    }
    if let Some(o) = &set.owners {
        if let Some(bad) = o
            .iter()
            .find(|h| h.trim().is_empty() || h.chars().any(char::is_whitespace))
        {
            anyhow::bail!("owner `{bad}` is not a handle — use @team, @user or a mail, no spaces");
        }
        changes.push(format!(
            "layer {name}: owners [{}] -> [{}]",
            l.owners.join(", "),
            o.join(", ")
        ));
        l.owners = o.clone();
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

pub mod contract;
pub mod services;
pub mod slots;
pub mod surface;
pub mod test_kinds;

#[cfg(test)]
mod tests;
