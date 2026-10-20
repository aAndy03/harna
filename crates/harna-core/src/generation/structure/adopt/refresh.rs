//! `adopt refresh` (0.3.43): the draft for a repo that already has a
//! manifest. Code keeps arriving after adoption; `harna adopt` used to
//! refuse there ("edit the manifest"), so newer modules drifted out of the
//! map one hand edit at a time. The refresh compares the tree with the
//! manifest and prints what changed, still a proposal and never written:
//!
//! - modules inside a container that no component registers, as
//!   paste-ready component stanzas under their container;
//! - modules outside every container (the map does not reach them);
//! - components whose declared path is gone (`harna move` relinks);
//! - containers without a `path` (path-governed checks cannot see them).
//!
//! Crate glue (`lib.rs`, `main.rs`, `mod.rs`, `build.rs`) and whole test
//! files are structure, not components, and are never proposed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Context as _;

use crate::schema::model::manifest::{Component, Manifest};
use crate::schema::model::scope::path_in_container;
use crate::search::digest;

/// What the tree has that the manifest does not, and the reverse.
#[derive(Debug, Clone, Default)]
pub struct Refresh {
    /// (container name, container path) -> proposed components.
    pub unregistered: BTreeMap<(String, String), Vec<Component>>,
    /// Module paths no container path covers.
    pub outside: Vec<String>,
    /// (component name, declared path) missing on disk.
    pub stale: Vec<(String, String)>,
    /// (container name, layer name) with an empty path.
    pub pathless: Vec<(String, String)>,
}

impl Refresh {
    pub fn is_current(&self) -> bool {
        self.unregistered.is_empty()
            && self.outside.is_empty()
            && self.stale.is_empty()
            && self.pathless.is_empty()
    }

    /// One line for stderr / the head of the MCP answer.
    pub fn summary(&self) -> String {
        format!(
            "refresh: {} unregistered module(s), {} outside every container, {} stale component path(s), {} container(s) without a path",
            self.unregistered.values().map(Vec::len).sum::<usize>(),
            self.outside.len(),
            self.stale.len(),
            self.pathless.len()
        )
    }

    /// The report: paste-ready stanzas first, then what needs a decision.
    pub fn render(&self) -> String {
        if self.is_current() {
            return "the manifest covers the tree — nothing to refresh\n".into();
        }
        let mut out = String::new();
        for ((name, path), comps) in &self.unregistered {
            out.push_str(&format!(
                "# unregistered in container `{name}` ({path}) — paste under it, then replace the draft summaries\n"
            ));
            for c in comps {
                out.push_str(&stanza(c));
            }
        }
        if !self.outside.is_empty() {
            out.push_str("# outside every container — the map does not reach these; scaffold a container or `harna container set <name> --path <dir>`\n");
            for p in &self.outside {
                out.push_str(&format!("#   {p}\n"));
            }
        }
        if !self.stale.is_empty() {
            out.push_str("# stale — declared, missing on disk; `harna move` relinks a move, or drop the component\n");
            for (name, path) in &self.stale {
                out.push_str(&format!("#   component `{name}`: {path}\n"));
            }
        }
        if !self.pathless.is_empty() {
            out.push_str(
                "# containers without a path — path-governed checks cannot see their files\n",
            );
            for (name, layer) in &self.pathless {
                out.push_str(&format!(
                    "#   `{name}` (layer {layer}): harna container set {name} --path <dir>\n"
                ));
            }
        }
        out
    }
}

/// Compare a fresh scan of the tree with `manifest`.
pub fn refresh(root: &Path, manifest: &Manifest, max_files: usize) -> anyhow::Result<Refresh> {
    let (map, _, _) =
        digest::build_incremental(root, max_files, None, true).context("scanning the tree")?;
    let mut registered: BTreeSet<&str> = BTreeSet::new();
    let mut names: Vec<String> = Vec::new();
    let mut r = Refresh::default();
    for l in &manifest.layers {
        for c in &l.containers {
            if c.path.is_empty() {
                r.pathless.push((c.name.clone(), l.name.clone()));
            }
            walk(&c.components, &mut |comp| {
                registered.insert(comp.path.as_str());
                names.push(comp.name.clone());
                // a planned slot's missing file is intent, not staleness (0.4.9)
                if !comp.planned && !comp.path.is_empty() && !root.join(&comp.path).exists() {
                    r.stale.push((comp.name.clone(), comp.path.clone()));
                }
            });
        }
    }
    // Longest container path wins when containers nest (the one prefix
    // rule `module_homes` applies everywhere).
    let mut hosts: Vec<(&str, &str)> = manifest
        .layers
        .iter()
        .flat_map(|l| l.containers.iter())
        .filter(|c| !c.path.is_empty())
        .map(|c| (c.path.as_str(), c.name.as_str()))
        .collect();
    hosts.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(b.0)));

    for m in &map.modules {
        if registered.contains(m.path.as_str()) || is_glue(&m.path) {
            continue;
        }
        match hosts.iter().find(|(p, _)| path_in_container(&m.path, p)) {
            Some((path, name)) => {
                let comp = proposal(root, &m.path, &mut names);
                r.unregistered
                    .entry((name.to_string(), path.to_string()))
                    .or_default()
                    .push(comp);
            }
            None => r.outside.push(m.path.clone()),
        }
    }
    Ok(r)
}

fn walk<'c>(comps: &'c [Component], f: &mut impl FnMut(&'c Component)) {
    for c in comps {
        f(c);
        walk(&c.children, f);
    }
}

/// Crate glue and test files are structure, never components.
fn is_glue(path: &str) -> bool {
    let file = path.rsplit('/').next().unwrap_or(path);
    matches!(file, "lib.rs" | "main.rs" | "mod.rs" | "build.rs") || digest::is_test_path(path)
}

/// A draft component for one module: a manifest-unique kebab name from
/// its stem, its size as the summary marker, the tests that exist.
fn proposal(root: &Path, path: &str, names: &mut Vec<String>) -> Component {
    let stem = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".rs")
        .replace('_', "-");
    let parent = path.rsplit('/').nth(1).unwrap_or("").replace('_', "-");
    let mut name = stem.clone();
    if names.contains(&name) {
        name = format!("{parent}-{stem}");
    }
    let base = name.clone();
    let mut n = 2;
    while names.contains(&name) {
        name = format!("{base}-{n}");
        n += 1;
    }
    names.push(name.clone());
    let lines = std::fs::read_to_string(root.join(path))
        .map(|s| s.lines().count())
        .unwrap_or(0);
    Component {
        planned: false,
        name,
        kind: crate::schema::model::manifest::ComponentKind::Primitive,
        path: path.to_string(),
        summary: format!("draft: {lines} lines"),
        tests: crate::generation::structure::linkage::tests_of(root, path)
            .into_iter()
            .collect(),
        provides: Vec::new(),
        children: Vec::new(),
    }
}

fn stanza(c: &Component) -> String {
    let tests = if c.tests.is_empty() {
        String::new()
    } else {
        format!(
            "tests = [{}]\n",
            c.tests
                .iter()
                .map(|t| format!("\"{t}\""))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!(
        "[[layers.containers.components]]\nname = \"{}\"\nkind = \"primitive\"\npath = \"{}\"\nsummary = \"{}\"\n{tests}\n",
        c.name, c.path, c.summary
    )
}

#[cfg(test)]
mod tests;
