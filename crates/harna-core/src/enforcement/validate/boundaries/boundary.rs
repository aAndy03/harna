//! Component `boundary`: cross-layer import detection on the real import
//! graph — no name-prefix heuristics.
//!
//! Each internal import is resolved (shared `digest::resolve_dep`) to its
//! target crate + module stem and matched against manifest containers; the
//! importer's allowed set is its own layers plus declared `depends_on`.

use std::path::Path;

use super::{Severity, Violation};

pub fn check(
    root: &Path,
    manifest: &crate::schema::model::manifest::Manifest,
    srcs: &[(String, String)],
    out: &mut Vec<Violation>,
) {
    check_homed(root, manifest, srcs, false, out)
}

/// `crate_home` (0.4.9, test-reach): a file outside every container path
/// (`crates/api/tests/e2e.rs`) is homed to the containers of its own crate
/// instead of skipped — a crate's tests live beside its `src`, not in it.
pub fn check_homed(
    root: &Path,
    manifest: &crate::schema::model::manifest::Manifest,
    srcs: &[(String, String)],
    crate_home: bool,
    out: &mut Vec<Violation>,
) {
    let labels = container_labels(root, manifest);
    if labels.is_empty() {
        return; // no /src-shaped container paths: nothing governable
    }
    let crate_names = crate::search::digest::workspace_crate_names(root);
    let index = crate::search::digest::crates::CrateIndex::load(root);
    let layer_depends: std::collections::HashMap<&str, Vec<&str>> = manifest
        .layers
        .iter()
        .map(|l| {
            (
                l.name.as_str(),
                l.depends_on.iter().map(|s| s.as_str()).collect(),
            )
        })
        .collect();

    let isolated: Vec<&str> = manifest
        .layers
        .iter()
        .filter(|l| l.isolated)
        .map(|l| l.name.as_str())
        .collect();

    let lx = LabelIndex::new(&labels);
    for (file, src) in srcs {
        let mut home: Vec<&ContainerLabel> = lx.homes(file);
        if home.is_empty() && crate_home {
            if let Some(dir) = index.crate_of(file).0 {
                home = lx.of_crate(None, &dir).to_vec();
            }
        }
        if home.is_empty() {
            continue; // ungoverned file
        }
        let home_layers: Vec<&str> = home.iter().map(|l| l.layer.as_str()).collect();
        let home_crate_dir = home[0].crate_dir.as_str();
        let importer_stem = index.crate_of(file).1;

        // Sibling edges use the live source's declared mods, exactly like
        // the scan does — otherwise `validate` and the map disagree about
        // bare submodule references.
        let deps = crate::search::digest::extract_deps(
            src,
            &crate_names,
            &crate::search::digest::declared_mods(src),
        );
        for dep in deps {
            let Some(target) =
                crate::search::digest::resolve_dep(&dep, &importer_stem, &crate_names)
            else {
                continue;
            };
            let target_labels: Vec<&ContainerLabel> = lx
                .of_crate(target.external.as_deref(), home_crate_dir)
                .iter()
                .copied()
                .filter(|l| stem_in_container(&target.stem, &l.rel_prefix))
                .collect();
            if target_labels.is_empty() {
                continue; // target outside the manifest's reach
            }
            // Sibling isolation (0.4.2): a peer container of an isolated
            // layer is out of reach even though its layer is the home's own.
            if let Some(sib) = target_labels.iter().find(|t| {
                isolated.contains(&t.layer.as_str())
                    && home
                        .iter()
                        .all(|h| h.layer == t.layer && h.container_path != t.container_path)
            }) {
                out.push(Violation {
                    check: "sibling-isolation".into(),
                    file: file.clone(),
                    line: find_line(src, &dep),
                    message: format!(
                        "import `{dep}` reaches sibling container `{}` of isolated layer `{}`",
                        sib.container_path, sib.layer
                    ),
                    hint: "siblings meet through a lower layer (move the shared piece into a shared capability) or a declared corridor".into(),
                    severity: crate::enforcement::checks::severity_of("sibling-isolation"),
                });
                continue;
            }
            let mut allowed = home_layers.clone();
            for hl in &home_layers {
                if let Some(deps) = layer_depends.get(*hl) {
                    allowed.extend(deps.iter().copied());
                }
            }
            let reached: Vec<&str> = target_labels
                .iter()
                .map(|l| l.layer.as_str())
                .filter(|tl| !allowed.contains(tl))
                .collect();
            if reached.is_empty() {
                continue;
            }
            out.push(Violation {
                check: "layer-boundary".into(),
                file: file.clone(),
                line: find_line(src, &dep),
                message: format!(
                    "cross-layer import `{dep}` reaches layer(s) {} not in {}'s allowed set",
                    reached.join(", "),
                    home_layers.join("/")
                ),
                hint: "declare depends_on on the importing layer, or route via a transiter".into(),
                severity: Severity::Error,
            });
        }
    }
}

fn stem_in_container(stem: &str, rel_prefix: &str) -> bool {
    rel_prefix.is_empty()
        || stem == rel_prefix
        || stem.starts_with(&format!("{rel_prefix}/"))
        || rel_prefix.starts_with(&format!("{stem}/"))
}

/// One check from two sides (0.3.4 encapsulation): the manifest declares
/// what a container `provides`; the live import graph says what importers
/// consume. A cross-container import may only name declared symbols, and a
/// `priv` inventory item is never legal to reach across containers —
/// declared surface or not. Containers that never declared `provides` are
/// only held to the private-symbol rule (adoption stays additive).
pub fn check_declared_surface(
    root: &Path,
    manifest: &crate::schema::model::manifest::Manifest,
    digest: Option<&crate::search::digest::Digest>,
    srcs: &[(String, String)],
    out: &mut Vec<Violation>,
) {
    use crate::search::digest::{resolve_dep, workspace_crate_names};

    let labels = container_labels(root, manifest);
    if labels.is_empty() {
        return;
    }
    let crate_names = workspace_crate_names(root);
    let index = crate::search::digest::crates::CrateIndex::load(root);

    // Private declarations per container (from the committed map): the two-
    // sidedness of the check depends on the digest, so without one we only
    // enforce the declared side.
    let mut priv_by_container: std::collections::HashMap<
        String,
        std::collections::BTreeSet<String>,
    > = Default::default();
    let mut pub_by_container: std::collections::HashMap<
        String,
        std::collections::BTreeSet<String>,
    > = Default::default();
    // labels by folder, found by a walk up each path (0.9.1: every label
    // was tested against every module — 98k x 1 700)
    let by_path = crate::schema::model::scope::index_by_path(
        labels.iter().map(|l| (l.container_path.clone(), l)),
    );
    if let Some(d) = digest {
        for m in &d.modules {
            for l in crate::schema::model::scope::enclosing(&by_path, &m.path)
                .into_iter()
                .copied()
            {
                for item in m
                    .inventory
                    .iter()
                    .filter(|i| i.vis == "priv" && i.kind != "impl")
                {
                    priv_by_container
                        .entry(l.container_path.clone())
                        .or_default()
                        .insert(item.name.clone());
                }
                // Public anywhere in the container (declared or re-exported)
                // means a same-named private item is a collision, not the
                // target (0.3.45: `deno_runtime::exit` is `pub use
                // deno_os::exit`; a private `exit` elsewhere is not it).
                let public = pub_by_container
                    .entry(l.container_path.clone())
                    .or_default();
                public.extend(
                    m.inventory
                        .iter()
                        .filter(|i| i.vis == "pub")
                        .map(|i| i.name.clone()),
                );
                public.extend(m.reexports.iter().cloned());
            }
        }
    }

    // Crate-relative module stems per container: a dep whose stem lands
    // exactly on a module is a whole-module import, not a symbol reference —
    // no symbol can be named, so the surface check cannot misread it.
    let mut module_stems_by_container: std::collections::HashMap<
        String,
        std::collections::BTreeSet<String>,
    > = Default::default();
    // labels by folder, found by a walk up each path (0.9.1: every label
    // was tested against every module — 98k x 1 700)
    let by_path = crate::schema::model::scope::index_by_path(
        labels.iter().map(|l| (l.container_path.clone(), l)),
    );
    if let Some(d) = digest {
        for m in &d.modules {
            for l in crate::schema::model::scope::enclosing(&by_path, &m.path)
                .into_iter()
                .copied()
            {
                module_stems_by_container
                    .entry(l.container_path.clone())
                    .or_default()
                    .insert(m.crate_of().1);
            }
        }
    }

    let lx = LabelIndex::new(&labels);
    for (file, src) in srcs {
        let home: Vec<&ContainerLabel> = lx.homes(file);
        if home.is_empty() {
            continue; // ungoverned file
        }
        let home_container_paths: std::collections::BTreeSet<&str> =
            home.iter().map(|l| l.container_path.as_str()).collect();
        let home_crate_dir = home[0].crate_dir.as_str();
        let importer_stem = index.crate_of(file).1;

        let deps = crate::search::digest::extract_deps(
            src,
            &crate_names,
            &crate::search::digest::declared_mods(src),
        );
        for dep in deps {
            let Some(target) = resolve_dep(&dep, &importer_stem, &crate_names) else {
                continue;
            };
            let target_labels: Vec<&ContainerLabel> = lx
                .of_crate(target.external.as_deref(), home_crate_dir)
                .iter()
                .copied()
                .filter(|l| stem_in_container(&target.stem, &l.rel_prefix))
                .collect();
            // Innermost container wins the provides judgment: a stem may
            // boundary-match nested containers; the longest path decides.
            let Some(tl) = target_labels.iter().max_by_key(|l| l.container_path.len()) else {
                continue;
            };
            if home_container_paths.contains(tl.container_path.as_str()) {
                continue; // same container: encapsulation lets siblings breathe
            }
            // The consumed name is the last path segment the dep spells out.
            let sym = target.stem.rsplit('/').next().unwrap_or("");
            if sym.is_empty() {
                continue;
            }
            // Whole-module import: the dep names a module, not a symbol.
            let stems = module_stems_by_container.get(&tl.container_path);
            if stems.is_some_and(|stems| stems.contains(&target.stem)) {
                continue;
            }
            // The symbol is judged only when the path leading to it is a
            // module of the target (or its crate root). `deno_core::anyhow::
            // Context` runs through a re-exported external crate, not a
            // module of deno_core — it cannot name deno_core's private
            // `Context` (0.3.45, deno study A8: 193 false errors).
            let parent = target.stem.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
            if !parent.is_empty() && !stems.is_some_and(|stems| stems.contains(parent)) {
                continue;
            }
            let provides = &tl.provides;
            if !provides.is_empty() && !provides.iter().any(|p| p == sym) {
                out.push(Violation {
                    check: "declared-surface".into(),
                    file: file.clone(),
                    line: find_line(src, &dep),
                    message: format!(
                        "import `{dep}` reaches container `{}` outside its declared surface (`{sym}` not in provides)",
                        tl.container_path
                    ),
                    hint: format!(
                        "declare `{sym}` in the target container's/components' `provides`, or consume the declared facade"
                    ),
                    severity: Severity::Error,
                });
            }
            let public = pub_by_container.get(&tl.container_path);
            let shadowed = public.is_some_and(|p| p.contains(sym) || p.contains("*"));
            if !shadowed
                && priv_by_container
                    .get(&tl.container_path)
                    .is_some_and(|names| names.contains(sym))
            {
                out.push(Violation {
                    check: "declared-surface".into(),
                    file: file.clone(),
                    line: find_line(src, &dep),
                    message: format!(
                        "import `{dep}` reaches a private symbol `{sym}` across containers (`{}` never declared it)",
                        tl.container_path
                    ),
                    hint: "make the symbol part of the declared surface (provides) or keep it inside its container".into(),
                    severity: Severity::Error,
                });
            }
        }
    }
}

fn find_line(src: &str, needle: &str) -> usize {
    src.lines()
        .enumerate()
        .find(|(_, l)| l.contains(needle))
        .map(|(i, _)| i + 1)
        .unwrap_or(0)
}

struct ContainerLabel {
    layer: String,
    /// Directory of the owning crate, e.g. "crates/harna-core" ("." for the root).
    crate_dir: String,
    /// Package name of that crate.
    crate_pkg: String,
    /// Crate-relative prefix of the container path, e.g. "schema".
    rel_prefix: String,
    container_path: String,
    /// Declared export surface: the container's own plus its components'.
    provides: Vec<String>,
}

/// Labels found without scanning them all (0.9.1): by folder (a walk up a
/// file's path), by crate dir and by package (import spelling).
struct LabelIndex<'a> {
    by_path: std::collections::HashMap<String, Vec<&'a ContainerLabel>>,
    by_dir: std::collections::HashMap<&'a str, Vec<&'a ContainerLabel>>,
    by_pkg: std::collections::HashMap<String, Vec<&'a ContainerLabel>>,
}

impl<'a> LabelIndex<'a> {
    fn new(labels: &'a [ContainerLabel]) -> Self {
        let mut by_dir: std::collections::HashMap<&str, Vec<&ContainerLabel>> = Default::default();
        let mut by_pkg: std::collections::HashMap<String, Vec<&ContainerLabel>> =
            Default::default();
        for l in labels {
            by_dir.entry(l.crate_dir.as_str()).or_default().push(l);
            by_pkg
                .entry(l.crate_pkg.replace('-', "_"))
                .or_default()
                .push(l);
        }
        LabelIndex {
            by_path: crate::schema::model::scope::index_by_path(
                labels.iter().map(|l| (l.container_path.clone(), l)),
            ),
            by_dir,
            by_pkg,
        }
    }

    /// The labels whose container holds `file`.
    fn homes(&self, file: &str) -> Vec<&'a ContainerLabel> {
        crate::schema::model::scope::enclosing(&self.by_path, file)
            .into_iter()
            .copied()
            .collect()
    }

    /// The labels of the crate a dep target lands in.
    fn of_crate(&self, external: Option<&str>, home_dir: &str) -> &[&'a ContainerLabel] {
        let found = match external {
            Some(name) => self.by_pkg.get(&name.replace('-', "_")),
            None => self.by_dir.get(home_dir),
        };
        found.map(Vec::as_slice).unwrap_or(&[])
    }
}

fn container_labels(
    root: &Path,
    manifest: &crate::schema::model::manifest::Manifest,
) -> Vec<ContainerLabel> {
    let mut labels = Vec::new();
    let index = crate::search::digest::crates::CrateIndex::load(root);
    for layer in &manifest.layers {
        for c in &layer.containers {
            if c.path.is_empty() {
                continue;
            }
            // 0.3.44: the crate as Cargo declares it, then the `src/` shape.
            // the package from Cargo's own index when it knows the crate
            // (0.9.1: each container's Cargo.toml was read and parsed again)
            let (crate_dir, rel_prefix, known_pkg) = match index.locate_dir(&c.path) {
                Some((k, rel)) => (k.dir.clone(), rel, Some(k.package.clone())),
                None => match c.path.split_once("/src/") {
                    Some((before, tail)) => (before.to_string(), tail.to_string(), None),
                    None if c.path.starts_with("src/") => {
                        (".".to_string(), c.path[4..].to_string(), None)
                    }
                    _ => continue,
                },
            };
            let pkg = match known_pkg {
                Some(p) => p,
                None => {
                    let cargo = root.join(&crate_dir).join("Cargo.toml");
                    let Ok(src) = std::fs::read_to_string(&cargo) else {
                        continue;
                    };
                    let Ok(value) = toml::from_str::<toml::Value>(&src) else {
                        continue;
                    };
                    let Some(pkg) = value
                        .get("package")
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                    else {
                        continue;
                    };
                    pkg.to_string()
                }
            };
            labels.push(ContainerLabel {
                layer: layer.name.clone(),
                crate_dir,
                crate_pkg: pkg,
                rel_prefix: rel_prefix.trim_end_matches('/').to_string(),
                // Normalized once: `src/api/` and `src/api` are one container.
                container_path: crate::schema::model::scope::normalize_scope(&c.path),
                provides: {
                    let mut p = c.provides.clone();
                    fn collect(
                        comps: &[crate::schema::model::manifest::Component],
                        p: &mut Vec<String>,
                    ) {
                        for comp in comps {
                            p.extend(comp.provides.iter().cloned());
                            collect(&comp.children, p);
                        }
                    }
                    collect(&c.components, &mut p);
                    p.sort();
                    p.dedup();
                    p
                },
            });
        }
    }
    labels
}

pub mod ts;

#[cfg(test)]
mod tests;
