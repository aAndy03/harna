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
    let path = m.layers[li].containers[ci]
        .path
        .trim_end_matches('/')
        .to_string();
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
        matches!(rel, "mod.rs" | "lib.rs") || rel == format!("{leaf}.rs")
    };
    let mut names: Vec<String> = inside
        .iter()
        .filter(|md| is_root(&md.path))
        .flat_map(|md| md.reexports.iter().cloned())
        .filter(|n| n != "*")
        .collect();
    let mut source = "re-exports of the container's root module".to_string();
    if names.is_empty() {
        names = inside
            .iter()
            .flat_map(|md| md.inventory.iter())
            .filter(|i| {
                i.vis == "pub"
                    && matches!(
                        i.kind.as_str(),
                        "fn" | "async-fn" | "struct" | "enum" | "trait" | "type" | "const"
                    )
            })
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
