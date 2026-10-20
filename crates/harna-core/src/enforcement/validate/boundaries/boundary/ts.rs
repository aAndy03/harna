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
    let ctx = crate::search::digest::ts::Context::load(root);
    for m in digest
        .modules
        .iter()
        .filter(|m| m.lang == "typescript" || m.lang == "javascript")
    {
        let home = home_of(manifest, &m.path);
        let src = std::fs::read_to_string(root.join(&m.path)).unwrap_or_default();
        surface(root, &ctx, manifest, m, home.as_ref(), &src, out);
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

/// The names an importer takes from each specifier (0.6.2): `import X`
/// → `default`, `{ a, b as c }` → `a`, `b`, `* as ns` → `*`; type-only
/// imports and `export { a } from` count the same. Side-effect imports
/// take nothing.
pub fn imported_names(src: &str) -> Vec<(String, Vec<String>)> {
    let text = crate::search::digest::ts::code_lines(src).join("\n");
    let re = regex::Regex::new(
        r#"(?s)\b(?:import|export)\s+(?:type\s+)?([^;'"]*?)\s+from\s*['"]([^'"]+)['"]"#,
    )
    .expect("static");
    let mut out = Vec::new();
    for cap in re.captures_iter(&text) {
        let clause = cap[1].trim();
        let mut names = Vec::new();
        let (head, group) = match clause.find('{') {
            Some(at) => (&clause[..at], Some(&clause[at..])),
            None => (clause, None),
        };
        for part in head.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            if part.starts_with('*') {
                names.push("*".to_string());
            } else if part != "type" {
                names.push("default".to_string());
            }
        }
        if let Some(g) = group {
            for item in g
                .trim_matches(|c| c == '{' || c == '}' || c == ' ')
                .split(',')
            {
                let item = item.trim().trim_start_matches("type ").trim();
                let name = item.split(" as ").next().unwrap_or("").trim();
                if !name.is_empty() {
                    names.push(name.to_string());
                }
            }
        }
        if clause == "*" || clause.starts_with("* as") {
            names = vec!["*".to_string()];
        }
        out.push((cap[2].to_string(), names));
    }
    out
}

/// Declared surface for TS (0.6.2): a container with `provides` may be
/// imported from outside only for those names (`*` admits a namespace
/// import, `default` the default export).
fn surface(
    root: &std::path::Path,
    ctx: &crate::search::digest::ts::Context,
    manifest: &Manifest,
    m: &crate::search::digest::Module,
    home: Option<&Home>,
    src: &str,
    out: &mut Vec<Violation>,
) {
    for (spec, names) in imported_names(src) {
        let dep = crate::search::digest::ts::dep_of(root, ctx, &m.path, &spec);
        let Some(Some(target)) = file_edge(&dep) else {
            continue;
        };
        let Some(to) = home_of(manifest, target) else {
            continue;
        };
        if home.is_some_and(|h| h.container == to.container) {
            continue;
        }
        let Some(c) = manifest
            .layers
            .iter()
            .flat_map(|l| &l.containers)
            .find(|c| c.name == to.container)
        else {
            continue;
        };
        if c.provides.is_empty() {
            continue;
        }
        for n in names {
            if !c.provides.iter().any(|p| p == &n) {
                let line = src
                    .lines()
                    .position(|l| l.contains(spec.as_str()))
                    .map_or(0, |i| i + 1);
                out.push(Violation {
                    check: "declared-surface".into(),
                    file: m.path.clone(),
                    line,
                    severity: crate::enforcement::checks::severity_of("declared-surface"),
                    message: format!(
                        "imports `{n}` from container `{}`, which provides only {}",
                        c.name,
                        c.provides.join(", ")
                    ),
                    hint: format!(
                        "use what `{}` provides, or declare it there (harna container set {} --provides ...)",
                        c.name, c.name
                    ),
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

    /// 0.6.2: import clauses name what they take; a container's provides
    /// admits only those names from outside.
    #[test]
    fn ts_imports_respect_declared_surface() {
        let got = imported_names(concat!(
            "import Def, { a, b as c, type T } from './x';
",
            "import * as ns from \"./y\";
",
            "import type { U } from './z';
",
            "import './side.css';
",
            "export { e } from './w';
",
        ));
        assert_eq!(
            got,
            vec![
                (
                    "./x".to_string(),
                    vec!["default".to_string(), "a".into(), "b".into(), "T".into()]
                ),
                ("./y".to_string(), vec!["*".to_string()]),
                ("./z".to_string(), vec!["U".to_string()]),
                ("./w".to_string(), vec!["e".to_string()]),
            ]
        );
        let root = std::env::temp_dir().join(format!("harna-ts-surface-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let w = |rel: &str, body: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        w(
            "web/src/api/index.ts",
            "export const get = 1;
export const secret = 2;
",
        );
        w(
            "web/src/app.ts",
            "import { get, secret } from './api';
",
        );
        let m = Manifest {
            layers: vec![Layer {
                name: "l".into(),
                containers: vec![Container {
                    name: "api".into(),
                    path: "web/src/api".into(),
                    provides: vec!["get".into()],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let d = Digest {
            modules: vec![module("web/src/app.ts", &["file:web/src/api/index.ts"])],
            ..Default::default()
        };
        let mut out = Vec::new();
        check(&root, &m, &d, &mut out);
        let msgs: Vec<&str> = out.iter().map(|v| v.message.as_str()).collect();
        assert_eq!(msgs.len(), 1, "{msgs:?}");
        assert!(msgs[0].contains("imports `secret` from container `api`"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
