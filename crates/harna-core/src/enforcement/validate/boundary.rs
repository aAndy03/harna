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
    manifest: &crate::schema::manifest::Manifest,
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
            .filter(|l| file.starts_with(&format!("{}/", l.container_path)))
            .collect();
        if home.is_empty() {
            continue; // ungoverned file
        }
        let home_layers: Vec<&str> = home.iter().map(|l| l.layer.as_str()).collect();
        let home_crate_dir = home[0].crate_dir.as_str();
        let importer_stem = crate::search::digest::crate_of(file).1;

        let deps = crate::search::digest::extract_deps(src, &crate_names);
        for dep in deps {
            let Some(target) = crate::search::digest::resolve_dep(&dep, &importer_stem, &crate_names)
            else {
                continue;
            };
            let target_labels: Vec<&ContainerLabel> = labels
                .iter()
                .filter(|l| match target.external.as_deref() {
                    Some(crate_name) => l.crate_pkg == crate_name,
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
}

fn container_labels(
    root: &Path,
    manifest: &crate::schema::manifest::Manifest,
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
            let Ok(src) = std::fs::read_to_string(&cargo) else { continue };
            let Ok(value) = toml::from_str::<toml::Value>(&src) else { continue };
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
                container_path: c.path.clone(),
            });
        }
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::manifest::{Container, Layer, Manifest};

    fn fixture(tag: &str) -> (std::path::PathBuf, Manifest) {
        let root = std::env::temp_dir().join(format!("harna-boundary-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for (dir, pkg) in [(".", "t"), ("crates/t", "t")] {
            let cargo = root.join(dir).join("Cargo.toml");
            std::fs::create_dir_all(cargo.parent().unwrap()).unwrap();
            std::fs::write(&cargo, format!("[package]\nname = \"{pkg}\"\nversion = \"0.1.0\"\n")).unwrap();
        }
        let manifest = Manifest {
            schema_version: 1,
            layers: vec![
                Layer {
                    name: "api".into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec![],
                    containers: vec![Container {
                        name: "handlers".into(),
                        path: "crates/t/src/api".into(),
                        sequence: vec![],
                        components: vec![],
                    }],
                },
                Layer {
                    name: "engine".into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec![],
                    containers: vec![Container {
                        name: "core".into(),
                        path: "crates/t/src/engine".into(),
                        sequence: vec![],
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
}
