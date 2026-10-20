//! `surface` (0.4.5): a container's declared surface, drafted from what
//! the code already says. The household field test wrote 32 names into
//! `--provides` that its `mod.rs` already re-exported with `pub use`.
//! The draft reads the map: the re-exports of the container's root
//! modules (`mod.rs`, `lib.rs`, or the file named after the container),
//! else the public items its modules declare. A draft is offered, never
//! applied silently — the user accepts it (`--apply`) or edits it.

use anyhow::{bail, Context};

use super::{find_container, unknown};
use crate::schema::model::manifest::Manifest;
use crate::search::digest::Digest;

/// (source of the draft, names) for `container`.
pub fn draft(m: &Manifest, d: &Digest, container: &str) -> anyhow::Result<(String, Vec<String>)> {
    let (li, ci) = find_container(m, container).with_context(|| unknown("container", container))?;
    // `./x`, `x\y` and `x/` name one container (0.4.9)
    let path = crate::schema::model::scope::normalize_scope(&m.layers[li].containers[ci].path);
    if path.is_empty() {
        bail!("container `{container}` has no path — a surface is drafted from its code");
    }
    let inside: Vec<_> = d
        .modules
        .iter()
        .filter(|md| md.path.starts_with(&format!("{path}/")))
        .collect();
    let leaf = path.rsplit('/').next().unwrap_or(&path);
    let is_root = |p: &str| {
        let rel = &p[path.len() + 1..];
        matches!(rel, "mod.rs" | "lib.rs")
            || rel == format!("{leaf}.rs")
            // a TS container's entry (0.6.2)
            || rel.strip_prefix("index.").is_some_and(|e| crate::search::digest::ts::EXTENSIONS.contains(&e))
    };
    let public = |i: &&crate::search::digest::Item| {
        i.vis == "pub"
            && matches!(
                i.kind.as_str(),
                "fn" | "async-fn" | "struct" | "enum" | "trait" | "type" | "const" | "mod"
                    // TS/JS kinds (0.9.2, field test: interfaces and types
                    // were dropped from the drafted surface)
                    | "interface" | "class" | "function" | "let" | "var" | "namespace"
            )
    };
    // The root's re-exports AND the items the root itself declares
    // (0.4.9): a `lib.rs` holding `pub fn` beside its `pub use` lines
    // would otherwise lose those names to declared-surface.
    let mut names: Vec<String> = inside
        .iter()
        .filter(|md| is_root(&md.path))
        .flat_map(|md| {
            md.reexports
                .iter()
                .cloned()
                .chain(md.inventory.iter().filter(public).map(|i| i.name.clone()))
        })
        .filter(|n| n != "*")
        .collect();
    // a TS barrel (`index.ts`: `export * from './money'`) exposes what the
    // files it re-exports declare (0.9.2)
    for md in inside
        .iter()
        .filter(|md| is_root(&md.path) && md.lang != "rust")
    {
        for dep in &md.deps {
            let Some(Some(target)) = crate::search::digest::file_edge(dep) else {
                continue;
            };
            if let Some(t) = inside.iter().find(|x| x.path == target) {
                names.extend(t.inventory.iter().filter(public).map(|i| i.name.clone()));
            }
        }
    }
    let mut source =
        "the container's root module (re-exports and its own public items)".to_string();
    if names.is_empty() {
        names = inside
            .iter()
            .flat_map(|md| md.inventory.iter())
            .filter(public)
            .map(|i| i.name.clone())
            .collect();
        source = "public items declared in the container".into();
    }
    names.sort();
    names.dedup();
    Ok((source, names))
}

/// Write the drafted names as the container's `provides`.
pub fn apply(m: &mut Manifest, container: &str, names: &[String]) -> anyhow::Result<Vec<String>> {
    let (li, ci) = find_container(m, container).with_context(|| unknown("container", container))?;
    if names.is_empty() {
        bail!("the draft for `{container}` is empty — applying it would clear `provides`; run `harna scan` or check the container's path");
    }
    let c = &mut m.layers[li].containers[ci];
    let before = c.provides.len();
    c.provides = names.to_vec();
    Ok(vec![format!(
        "container `{container}`: provides {} -> {} name(s) (declared-surface now holds importers to them)",
        before,
        names.len()
    )])
}

#[cfg(test)]
mod tests;
