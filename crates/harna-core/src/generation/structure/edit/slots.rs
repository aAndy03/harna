//! `slots` (0.4.1): planned components — the shape of the future, declared
//! before its code. "The cache will live here, behind this gateway": a slot
//! has a name, a path and a purpose; validate reads its missing file as
//! intent, `place` steers matching work into it, `harna scaffold` with its
//! name fills it at its path.

use anyhow::{bail, Context};

use super::{find_container, unknown};
use crate::schema::model::manifest::{Component, ComponentKind, Manifest};

/// Declare a slot in `container`. The path is where its code will live.
pub fn add_slot(
    m: &mut Manifest,
    container: &str,
    name: &str,
    path: &str,
    summary: &str,
) -> anyhow::Result<Vec<String>> {
    let name = name.trim();
    let path = path.trim().replace('\\', "/");
    if name.is_empty() || path.is_empty() {
        bail!("a slot needs a name and the path its code will live at");
    }
    let taken = m
        .layers
        .iter()
        .flat_map(|l| &l.containers)
        .flat_map(|c| &c.components)
        .any(|c| c.name == name);
    if taken {
        bail!("a component named `{name}` already exists — names are unique across the manifest");
    }
    let (li, ci) = find_container(m, container).with_context(|| unknown("container", container))?;
    m.layers[li].containers[ci].components.push(Component {
        name: name.to_string(),
        kind: ComponentKind::Primitive,
        path: path.clone(),
        summary: summary.trim().to_string(),
        tests: Vec::new(),
        provides: Vec::new(),
        children: Vec::new(),
        planned: true,
    });
    Ok(vec![format!(
        "slot `{name}` planned in `{container}` at {path} — validate treats it as intent; `harna scaffold --container {container} --name {name}` fills it"
    )])
}

/// Mark a slot built (its code exists now).
pub fn fill_slot(
    root: &std::path::Path,
    m: &mut Manifest,
    name: &str,
) -> anyhow::Result<Vec<String>> {
    for c in m.layers.iter_mut().flat_map(|l| l.containers.iter_mut()) {
        if let Some(comp) = c.components.iter_mut().find(|k| k.name == name) {
            if !comp.planned {
                bail!("`{name}` is not a planned slot");
            }
            if !comp.path.is_empty() && !root.join(&comp.path).exists() {
                bail!(
                    "slot `{name}` has no code at {} yet — scaffold or write it first",
                    comp.path
                );
            }
            comp.planned = false;
            return Ok(vec![format!(
                "slot `{name}` built — now an ordinary component of `{}`",
                c.name
            )]);
        }
    }
    bail!("no component `{name}` in the manifest")
}

/// Every open slot: (layer, container, slot).
pub fn slots(m: &Manifest) -> Vec<(&str, &str, &Component)> {
    m.layers
        .iter()
        .flat_map(|l| {
            l.containers.iter().flat_map(move |c| {
                c.components
                    .iter()
                    .filter(|k| k.planned)
                    .map(move |k| (l.name.as_str(), c.name.as_str(), k))
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
