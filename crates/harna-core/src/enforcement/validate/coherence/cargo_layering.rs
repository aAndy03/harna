//! `cargo-layering` (0.3.45, deno study B4): the layer DAG on Cargo's own
//! dependency edges. deno writes its stack (`cli → runtime → ext → libs`)
//! as prose and nothing enforces it; `layer-boundary` catches an import
//! once it exists, this catches the `Cargo.toml` line that makes it
//! possible — exact, cheap, before any code.
//!
//! A crate's layers are those of the containers it hosts. Each workspace
//! dependency (normal `[dependencies]`; dev- and build-dependencies may
//! cross for tests and codegen) must land in those layers or in a layer
//! they declare in `depends_on`. The Cargo reading is the crate index's
//! (`CrateIndex::cargo_deps`) — the same one adopt drafts `depends_on`
//! from, so a fresh draft passes this check.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::Violation;
use crate::schema::model::manifest::Manifest;
use crate::search::digest::crates::CrateIndex;

pub fn check(root: &Path, manifest: &Manifest, out: &mut Vec<Violation>) {
    let index = CrateIndex::load(root);
    if index.crates.is_empty() {
        return;
    }
    let mut layers_of: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    for l in &manifest.layers {
        for c in l.containers.iter().filter(|c| !c.path.is_empty()) {
            if let Some((k, _)) = index.locate_dir(&c.path) {
                layers_of.entry(k.dir.clone()).or_default().insert(&l.name);
            }
        }
    }
    let depends: BTreeMap<&str, Vec<&str>> = manifest
        .layers
        .iter()
        .map(|l| {
            (
                l.name.as_str(),
                l.depends_on.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    let package = |dir: &str| {
        index
            .crates
            .iter()
            .find(|c| c.dir == dir)
            .map(|c| c.package.clone())
            .unwrap_or_else(|| dir.to_string())
    };

    for (dir, own) in &layers_of {
        let allowed: BTreeSet<&str> = own
            .iter()
            .flat_map(|l| std::iter::once(*l).chain(depends.get(l).into_iter().flatten().copied()))
            .collect();
        for target in index.cargo_deps(root, dir) {
            let Some(their) = layers_of.get(&target) else {
                continue; // a crate the manifest does not reach (yet)
            };
            let outside: Vec<&str> = their
                .iter()
                .filter(|l| !allowed.contains(*l))
                .copied()
                .collect();
            if outside.is_empty() {
                continue;
            }
            out.push(Violation {
                check: "cargo-layering".into(),
                file: if dir == "." {
                    "Cargo.toml".into()
                } else {
                    format!("{dir}/Cargo.toml")
                },
                line: 0,
                severity: crate::enforcement::checks::severity_of("cargo-layering"),
                message: format!(
                    "dependency `{}` reaches layer(s) {} that {} do(es) not declare in depends_on",
                    package(&target),
                    outside.join(", "),
                    own.iter().copied().collect::<Vec<_>>().join(", ")
                ),
                hint: "declare the reach (`harna layer set <layer> --depends-on …`) or drop the dependency".into(),
            });
        }
    }
}

#[cfg(test)]
mod tests;
