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
    let labels = container_labels(root, manifest);
    if labels.is_empty() {
        return; // no /src-shaped container paths: nothing governable
    }
    let crate_names = crate::search::digest::workspace_crate_names(root);
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

    for (file, src) in srcs {
        let home: Vec<&ContainerLabel> = labels
            .iter()
            .filter(|l| crate::schema::model::scope::path_in_container(file, &l.container_path))
            .collect();
        if home.is_empty() {
            continue; // ungoverned file
        }
        let home_layers: Vec<&str> = home.iter().map(|l| l.layer.as_str()).collect();
        let home_crate_dir = home[0].crate_dir.as_str();
        let importer_stem = crate::search::digest::crate_of(file).1;

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
            let target_labels: Vec<&ContainerLabel> = labels
                .iter()
                .filter(|l| match target.external.as_deref() {
                    Some(crate_name) => {
                        crate::search::digest::crate_name_eq(&l.crate_pkg, crate_name)
                    }
                    None => l.crate_dir == home_crate_dir,
                })
                .filter(|l| stem_in_container(&target.stem, &l.rel_prefix))
                .collect();
            if target_labels.is_empty() {
                continue; // target outside the manifest's reach
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
    use crate::search::digest::{crate_name_eq, crate_of, resolve_dep, workspace_crate_names};

    let labels = container_labels(root, manifest);
    if labels.is_empty() {
        return;
    }
    let crate_names = workspace_crate_names(root);

    // Private declarations per container (from the committed map): the two-
    // sidedness of the check depends on the digest, so without one we only
    // enforce the declared side.
    let mut priv_by_container: std::collections::HashMap<
        String,
        std::collections::BTreeSet<String>,
    > = Default::default();
    if let Some(d) = digest {
        for m in &d.modules {
            for l in labels.iter().filter(|l| {
                crate::schema::model::scope::path_in_container(&m.path, &l.container_path)
            }) {
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
    if let Some(d) = digest {
        for m in &d.modules {
            for l in labels.iter().filter(|l| {
                crate::schema::model::scope::path_in_container(&m.path, &l.container_path)
            }) {
                module_stems_by_container
                    .entry(l.container_path.clone())
                    .or_default()
                    .insert(crate_of(&m.path).1);
            }
        }
    }

    for (file, src) in srcs {
        let home: Vec<&ContainerLabel> = labels
            .iter()
            .filter(|l| crate::schema::model::scope::path_in_container(file, &l.container_path))
            .collect();
        if home.is_empty() {
            continue; // ungoverned file
        }
        let home_container_paths: std::collections::BTreeSet<&str> =
            home.iter().map(|l| l.container_path.as_str()).collect();
        let home_crate_dir = home[0].crate_dir.as_str();
        let importer_stem = crate_of(file).1;

        let deps = crate::search::digest::extract_deps(
            src,
            &crate_names,
            &crate::search::digest::declared_mods(src),
        );
        for dep in deps {
            let Some(target) = resolve_dep(&dep, &importer_stem, &crate_names) else {
                continue;
            };
            let target_labels: Vec<&ContainerLabel> = labels
                .iter()
                .filter(|l| match target.external.as_deref() {
                    Some(crate_name) => crate_name_eq(&l.crate_pkg, crate_name),
                    None => l.crate_dir == home_crate_dir,
                })
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
            if module_stems_by_container
                .get(&tl.container_path)
                .is_some_and(|stems| stems.contains(&target.stem))
            {
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
            if priv_by_container
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

fn container_labels(
    root: &Path,
    manifest: &crate::schema::model::manifest::Manifest,
) -> Vec<ContainerLabel> {
    let mut labels = Vec::new();
    for layer in &manifest.layers {
        for c in &layer.containers {
            if c.path.is_empty() {
                continue;
            }
            let (crate_dir, rel_prefix) = match c.path.split_once("/src/") {
                Some((before, tail)) => (before.to_string(), tail.to_string()),
                None if c.path.starts_with("src/") => (".".to_string(), c.path[4..].to_string()),
                _ => continue,
            };
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
            labels.push(ContainerLabel {
                layer: layer.name.clone(),
                crate_dir,
                crate_pkg: pkg.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::{Container, Layer, Manifest};

    fn fixture(tag: &str) -> (std::path::PathBuf, Manifest) {
        let root =
            std::env::temp_dir().join(format!("harna-boundary-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for (dir, pkg) in [(".", "t"), ("crates/t", "t")] {
            let cargo = root.join(dir).join("Cargo.toml");
            std::fs::create_dir_all(cargo.parent().unwrap()).unwrap();
            std::fs::write(
                &cargo,
                format!("[package]\nname = \"{pkg}\"\nversion = \"0.1.0\"\n"),
            )
            .unwrap();
        }
        let manifest = Manifest {
            schema_version: 1,
            layers: vec![
                Layer {
                    name: "api".into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec![],
                    role: None,
                    containers: vec![Container {
                        name: "handlers".into(),
                        path: "crates/t/src/api".into(),
                        description: String::new(),
                        sequence: vec![],
                        provides: vec![],
                        role: None,
                        gateway: None,
                        connects: vec![],
                        components: vec![],
                    }],
                },
                Layer {
                    name: "engine".into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec![],
                    role: None,
                    containers: vec![Container {
                        name: "core".into(),
                        path: "crates/t/src/engine".into(),
                        description: String::new(),
                        sequence: vec![],
                        provides: vec![],
                        role: None,
                        gateway: None,
                        connects: vec![],
                        components: vec![],
                    }],
                },
            ],
        };
        (root, manifest)
    }

    #[test]
    fn flags_undeclared_cross_layer_import() {
        let (root, mut manifest) = fixture("flags");
        std::fs::create_dir_all(root.join("crates/t/src/api")).unwrap();
        std::fs::create_dir_all(root.join("crates/t/src/engine")).unwrap();
        std::fs::write(
            root.join("crates/t/src/api/h.rs"),
            "use crate::engine::billing::charge;\nfn main() {}",
        )
        .unwrap();
        let srcs = vec![(
            "crates/t/src/api/h.rs".to_string(),
            std::fs::read_to_string(root.join("crates/t/src/api/h.rs")).unwrap(),
        )];

        let mut v = Vec::new();
        check(&root, &manifest, &srcs, &mut v);
        assert_eq!(v.len(), 1, "undeclared cross-layer import is an error");
        assert_eq!(v[0].check, "layer-boundary");
        assert_eq!(v[0].line, 1);
        assert!(v[0].message.contains("engine"));

        // Declaring the dependency makes the same import legal.
        manifest.layers[0].depends_on = vec!["engine".into()];
        let mut v2 = Vec::new();
        check(&root, &manifest, &srcs, &mut v2);
        assert!(v2.is_empty());
    }

    #[test]
    fn allows_same_layer_and_ungoverned_stems() {
        let (root, manifest) = fixture("allowed");
        std::fs::create_dir_all(root.join("crates/t/src/api")).unwrap();
        std::fs::create_dir_all(root.join("crates/t/src/engine")).unwrap();

        // Same-container import + import of a crate-root symbol no container
        // governs (stem `t` -> ungoverned -> skipped).
        std::fs::write(
            root.join("crates/t/src/api/h.rs"),
            "use crate::api::helpers;\nuse crate::T;\nfn main() {}",
        )
        .unwrap();
        let srcs = vec![(
            "crates/t/src/api/h.rs".to_string(),
            std::fs::read_to_string(root.join("crates/t/src/api/h.rs")).unwrap(),
        )];

        let mut v = Vec::new();
        check(&root, &manifest, &srcs, &mut v);
        assert!(v.is_empty(), "same-layer and ungoverned imports allowed");
    }

    fn module(path: &str, items: &[(&str, &str, &str, &str)]) -> crate::search::digest::Module {
        crate::search::digest::Module {
            path: path.into(),
            lang: "rust".into(),
            signatures: vec![],
            deps: vec![],
            body_hash: "x".into(),
            inventory: items
                .iter()
                .map(|(vis, kind, name, text)| crate::search::digest::Item {
                    vis: vis.to_string(),
                    kind: kind.to_string(),
                    name: name.to_string(),
                    text: text.to_string(),
                })
                .collect(),
            mods: vec![],
        }
    }

    #[test]
    fn declared_surface_gates_cross_container_symbols_and_priv() -> anyhow::Result<()> {
        let (root, mut manifest) = fixture("surface");
        manifest.layers[0].depends_on = vec!["engine".into()]; // layer gate silent
                                                               // The engine container declares its export surface.
        manifest.layers[1].containers[0].provides = vec!["Billing".into(), "Facade".into()];
        std::fs::create_dir_all(root.join("crates/t/src/api")).unwrap();
        std::fs::create_dir_all(root.join("crates/t/src/engine")).unwrap();
        std::fs::write(
            root.join("crates/t/src/api/h.rs"),
            // Symbol detail survives in flat use-paths and inline references;
            // brace-group imports collapse to their head module (documented
            // limitation — keep flat paths to get surface-checked).
            "use crate::engine::billing::Billing;\nfn go(h: crate::engine::billing::Secret) {}",
        )
        .unwrap();
        let srcs = vec![(
            "crates/t/src/api/h.rs".to_string(),
            std::fs::read_to_string(root.join("crates/t/src/api/h.rs")).unwrap(),
        )];

        let digest = crate::search::digest::Digest {
            generated_at: "now".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![module(
                "crates/t/src/engine/billing.rs",
                &[
                    ("pub", "struct", "Billing", "pub struct Billing"),
                    ("priv", "fn", "Secret", "fn Secret() {}"),
                ],
            )],
        };

        let mut v = Vec::new();
        check_declared_surface(&root, &manifest, Some(&digest), &srcs, &mut v);
        // `Billing` is declared (quiet); `Secret` is both undeclared AND
        // private — two findings, each naming its reason.
        assert_eq!(v.len(), 2);
        assert!(v.iter().all(|x| x.check == "declared-surface"));
        assert!(v.iter().any(|x| x.message.contains("not in provides")));
        assert!(v
            .iter()
            .any(|x| x.message.contains("private symbol `Secret`")));

        // Declaring the symbol quiets the declared side; the private side
        // is not negotiable.
        manifest.layers[1].containers[0]
            .provides
            .push("Secret".into());
        let mut v2 = Vec::new();
        check_declared_surface(&root, &manifest, Some(&digest), &srcs, &mut v2);
        assert_eq!(v2.len(), 1);
        assert!(v2[0].message.contains("private symbol"));
        Ok(())
    }

    #[test]
    fn undeclared_containers_are_not_gated_and_module_imports_are_never_symbols(
    ) -> anyhow::Result<()> {
        let (root, mut manifest) = fixture("nosurface");
        manifest.layers[0].depends_on = vec!["engine".into()];
        std::fs::create_dir_all(root.join("crates/t/src/api")).unwrap();
        std::fs::create_dir_all(root.join("crates/t/src/engine")).unwrap();
        // Whole-module import + a symbol import: no provides anywhere.
        std::fs::write(
            root.join("crates/t/src/api/h.rs"),
            "use crate::engine::billing;\npub fn go(s: crate::engine::billing::Handle) {}",
        )
        .unwrap();
        let srcs = vec![(
            "crates/t/src/api/h.rs".to_string(),
            std::fs::read_to_string(root.join("crates/t/src/api/h.rs")).unwrap(),
        )];
        let digest = crate::search::digest::Digest {
            generated_at: "now".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![module(
                "crates/t/src/engine/billing.rs",
                &[
                    ("pub", "struct", "Handle", "pub struct Handle"),
                    ("priv", "fn", "internal", "fn internal() {}"),
                ],
            )],
        };

        let mut v = Vec::new();
        check_declared_surface(&root, &manifest, Some(&digest), &srcs, &mut v);
        assert!(
            v.is_empty(),
            "no declared surface -> no gate yet; module stems are not symbols"
        );

        // Declaring a surface flips the gate: `Handle` declared (quiet),
        // `billing` whole-module still exempt.
        manifest.layers[1].containers[0].provides = vec!["Handle".into()];
        let mut v2 = Vec::new();
        check_declared_surface(&root, &manifest, Some(&digest), &srcs, &mut v2);
        assert!(v2.is_empty());
        Ok(())
    }
}
