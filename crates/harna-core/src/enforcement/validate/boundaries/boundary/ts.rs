//! Component `ts-boundary` (0.6.2): layers, isolation and package
//! gateways for TypeScript/JavaScript containers, over the map's file
//! edges (0.6.0).
//!
//! A TS container is a manifest container whose path holds TS files —
//! `web/src/features/ledger`. Each import of a repository file is an edge
//! from the importer's container to the target's: the target's layer must
//! be the importer's own or one it `depends_on` (`layer-boundary`), and a
//! sibling container of an `isolated` layer is out of reach
//! (`sibling-isolation`). A container whose `gateway` names `npm:<pkg>`
//! is the only one that may import that package (`gateway-ownership`):
//! `npm:axios` behind the api client, `npm:@tauri-apps/api` behind the
//! bridge. The same words as Rust, read from the same manifest.

use crate::enforcement::violation::Violation;
use crate::schema::model::manifest::Manifest;
use crate::schema::model::scope::path_in_container;
use crate::search::digest::{file_edge, Digest};

struct Home<'m> {
    layer: &'m str,
    container: &'m str,
    path: &'m str,
}

/// The innermost container holding `path` (nested containers: the longest
/// path wins).
fn home_of<'m>(manifest: &'m Manifest, path: &str) -> Option<Home<'m>> {
    manifest
        .layers
        .iter()
        .flat_map(|l| l.containers.iter().map(move |c| (l, c)))
        .filter(|(_, c)| path_in_container(path, &c.path))
        .max_by_key(|(_, c)| c.path.len())
        .map(|(l, c)| Home {
            layer: &l.name,
            container: &c.name,
            path: &c.path,
        })
}

pub fn check(
    root: &std::path::Path,
    manifest: &Manifest,
    digest: &Digest,
    out: &mut Vec<Violation>,
) {
    let gateways: Vec<(String, &str)> = manifest
        .layers
        .iter()
        .flat_map(|l| &l.containers)
        .flat_map(|c| {
            c.gateway
                .as_deref()
                .unwrap_or("")
                .split(',')
                .map(str::trim)
                .filter_map(|g| g.strip_prefix("npm:"))
                .map(move |pkg| (pkg.to_string(), c.path.as_str()))
                .collect::<Vec<_>>()
        })
        .collect();
    for m in digest
        .modules
        .iter()
        .filter(|m| m.lang == "typescript" || m.lang == "javascript")
    {
        let home = home_of(manifest, &m.path);
        let src = std::fs::read_to_string(root.join(&m.path)).unwrap_or_default();
        let line_of = |needle: &str| {
            src.lines()
                .enumerate()
                .find(|(_, l)| l.contains(needle))
                .map_or(0, |(i, _)| i + 1)
        };
        for dep in &m.deps {
            if let Some(pkg) = dep.strip_prefix("npm:") {
                if let Some((_, owner)) = gateways.iter().find(|(p, _)| p == pkg) {
                    if !path_in_container(&m.path, owner) {
                        out.push(Violation {
                            check: "gateway-ownership".into(),
                            file: m.path.clone(),
                            line: line_of(pkg),
                            severity: crate::enforcement::checks::severity_of("gateway-ownership"),
                            message: format!("imports package `{pkg}`, which only `{owner}` may (gateway npm:{pkg})"),
                            hint: format!("go through the gateway in `{owner}`, or move this code there"),
                        });
                    }
                }
                continue;
            }
            let Some(Some(target)) = file_edge(dep) else {
                continue;
            };
            let (Some(from), Some(to)) = (home.as_ref(), home_of(manifest, target)) else {
                continue; // an ungoverned side: nothing declared to hold
            };
            if from.container == to.container {
                continue;
            }
            let stem = std::path::Path::new(target)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(target);
            if from.layer == to.layer {
                let isolated = manifest
                    .layers
                    .iter()
                    .any(|l| l.name == from.layer && l.isolated);
                if isolated {
                    out.push(Violation {
                        check: "sibling-isolation".into(),
                        file: m.path.clone(),
                        line: line_of(stem),
                        severity: crate::enforcement::checks::severity_of("sibling-isolation"),
                        message: format!(
                            "imports `{target}` of sibling container `{}` in isolated layer `{}`",
                            to.container, to.layer
                        ),
                        hint: "siblings meet through a lower layer (move the shared piece down) or a declared corridor".into(),
                    });
                }
                continue;
            }
            let allowed = manifest
                .layers
                .iter()
                .find(|l| l.name == from.layer)
                .is_some_and(|l| l.depends_on.iter().any(|d| d == to.layer));
            if !allowed {
                out.push(Violation {
                    check: "layer-boundary".into(),
                    file: m.path.clone(),
                    line: line_of(stem),
                    severity: crate::enforcement::checks::severity_of("layer-boundary"),
                    message: format!(
                        "imports `{target}` ({} in layer {}), not in {}'s allowed set",
                        to.path, to.layer, from.layer
                    ),
                    hint: "declare depends_on on the importing layer, or route through a corridor"
                        .into(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::{Container, Layer};
    use crate::search::digest::Module;

    fn module(path: &str, deps: &[&str]) -> Module {
        Module {
            path: path.into(),
            lang: "typescript".into(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    /// 0.6.2: the Rust words hold for TS — a downward import passes, an
    /// upward one is a layer-boundary, siblings of an isolated layer stay
    /// apart, and a package gateway has one owner.
    #[test]
    fn ts_containers_keep_layers_isolation_and_gateways() {
        let c = |name: &str, path: &str, gateway: Option<&str>| Container {
            name: name.into(),
            path: path.into(),
            gateway: gateway.map(str::to_string),
            ..Default::default()
        };
        let m = Manifest {
            layers: vec![
                Layer {
                    name: "features".into(),
                    depends_on: vec!["shared".into()],
                    isolated: true,
                    containers: vec![
                        c("ledger", "web/src/features/ledger", None),
                        c("reports", "web/src/features/reports", None),
                    ],
                    ..Default::default()
                },
                Layer {
                    name: "shared".into(),
                    containers: vec![c("api", "web/src/shared/api", Some("npm:axios"))],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let d = Digest {
            modules: vec![
                module(
                    "web/src/features/ledger/Table.tsx",
                    &[
                        "file:web/src/shared/api/client.ts",
                        "file:web/src/features/reports/chart.ts",
                        "npm:axios",
                        "npm:react",
                    ],
                ),
                module(
                    "web/src/shared/api/client.ts",
                    &["npm:axios", "file:web/src/features/ledger/Table.tsx"],
                ),
            ],
            ..Default::default()
        };
        let mut out = Vec::new();
        check(std::path::Path::new("."), &m, &d, &mut out);
        let got: Vec<(&str, &str)> = out
            .iter()
            .map(|v| (v.check.as_str(), v.file.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("sibling-isolation", "web/src/features/ledger/Table.tsx"),
                ("gateway-ownership", "web/src/features/ledger/Table.tsx"),
                ("layer-boundary", "web/src/shared/api/client.ts"),
            ],
            "{out:?}"
        );
    }
}
