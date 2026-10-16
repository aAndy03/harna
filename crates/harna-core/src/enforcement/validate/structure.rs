//! Component `structure` (0.3.6): two structural invariants read from the
//! committed map:
//! - `no-cycles`: the import graph stays acyclic. A cycle is reported as
//!   the ring it forms (a → b → a), never as an anonymous mystery.
//! - `engine-own-container`: containers that declare `role = "engine"`
//!   own their family — a module whose file leaf starts `<engine>-` may
//!   not live outside the engine's container path (opt-in: containers
//!   without a role owe nothing).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::{Severity, Violation};
use crate::schema::manifest::Manifest;
use crate::search::digest::{
    crate_name_dirs, crate_of, hosted_module, resolve_dep, workspace_crate_names, Digest,
};

pub fn check(
    root: &Path,
    manifest: &Manifest,
    digest: Option<&Digest>,
    out: &mut Vec<Violation>,
) {
    let Some(d) = digest else { return };
    check_no_cycles(root, d, out);
    check_engine_own_container(manifest, d, out);
}

// ----- no-cycles -----

/// The importer→imported module graph, resolved with the shared dep
/// machinery (`resolve_dep` + `hosted_module` — the same read budgets and
/// layering use, so the three can never disagree about who hosts what).
fn module_graph<'a>(root: &Path, d: &'a Digest) -> BTreeMap<&'a str, Vec<&'a str>> {
    let crate_names = workspace_crate_names(root);
    let dirs: std::collections::BTreeSet<String> = d
        .modules
        .iter()
        .filter_map(|m| crate_of(&m.path).0)
        .filter(|dir| dir != ".")
        .collect();
    let name_dirs = crate_name_dirs(root, &dirs.into_iter().collect::<Vec<_>>());

    let mut graph: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for m in &d.modules {
        let (importer_dir, importer_stem) = crate_of(&m.path);
        for dep in &m.deps {
            let Some(target) = resolve_dep(dep, importer_stem.as_str(), &crate_names) else {
                continue;
            };
            let Some(target_module) =
                hosted_module(d, &name_dirs, importer_dir.as_deref(), &target)
            else {
                continue;
            };
            if m.path.ends_with("checks.rs") {
                eprintln!("DBG checks.rs dep {} -> {:?}", dep, target_module.path);
            }
            if target_module.path != m.path {
                graph
                    .entry(m.path.as_str())
                    .or_default()
                    .push(target_module.path.as_str());
            }
        }
    }
    graph
}

fn check_no_cycles(root: &Path, d: &Digest, out: &mut Vec<Violation>) {
    let graph = module_graph(root, d);

    enum Color {
        White,
        Grey,
        Black,
    }
    let mut color: BTreeMap<&str, Color> = graph.keys().map(|k| (*k, Color::White)).collect();
    let mut reported: BTreeSet<Vec<String>> = BTreeSet::new();
    let mut stack: Vec<&str> = Vec::new();

    for start in graph.keys().copied().collect::<Vec<_>>() {
        dfs(start, &graph, &mut color, &mut stack, &mut reported, out);
    }

    fn dfs<'a>(
        node: &'a str,
        graph: &BTreeMap<&'a str, Vec<&'a str>>,
        color: &mut BTreeMap<&'a str, Color>,
        stack: &mut Vec<&'a str>,
        reported: &mut BTreeSet<Vec<String>>,
        out: &mut Vec<Violation>,
    ) {
        match color.get(node) {
            Some(Color::Black) => return,
            Some(Color::Grey) => {
                if let Some(pos) = stack.iter().position(|n| *n == node) {
                    // The ring: stack[pos..] plus the return edge.
                    let mut ring: Vec<String> =
                        stack[pos..].iter().map(|n| n.to_string()).collect();
                    ring.push(node.to_string());
                    ring = normalize_ring(&ring);
                    if reported.insert(ring.clone()) {
                        out.push(Violation {
                            check: "no-cycles".into(),
                            file: ring[0].clone(),
                            line: 0,
                            severity: Severity::Error,
                            message: format!(
                                "import cycle: {}",
                                ring.join(" -> ")
                            ),
                            hint: format!(
                                "break the ring ({} member(s)): extract the shared piece, route through a transiter, or re-home one of the modules",
                                ring.len() - 1
                            ),
                        });
                    }
                }
                return;
            }
            None => {
                color.insert(node, Color::White);
            }
            Some(Color::White) => {}
        }
        color.insert(node, Color::Grey);
        stack.push(node);
        if let Some(targets) = graph.get(node).cloned() {
            for t in targets {
                dfs(t, graph, color, stack, reported, out);
            }
        }
        stack.pop();
        color.insert(node, Color::Black);
    }
}

/// Rotate the ring so its smallest member opens — the same ring reports
/// once, deterministically, whichever member's DFS closed it.
fn normalize_ring(ring: &[String]) -> Vec<String> {
    let members = &ring[..ring.len() - 1]; // drop the closing repeat
    if members.is_empty() {
        return ring.to_vec();
    }
    let mut best = 0usize;
    for (i, m) in members.iter().enumerate() {
        if m < &members[best] {
            best = i;
        }
    }
    let n = members.len();
    let mut rotated: Vec<String> = (0..n).map(|i| members[(best + i) % n].clone()).collect();
    rotated.push(members[best].clone());
    rotated
}

// ----- engine-own-container -----

fn check_engine_own_container(manifest: &Manifest, d: &Digest, out: &mut Vec<Violation>) {
    let engines: Vec<(&str, &str)> = manifest
        .layers
        .iter()
        .flat_map(|l| l.containers.iter())
        .filter(|c| {
            c.role
                .as_deref()
                .map_or(false, |r| r.eq_ignore_ascii_case("engine"))
        })
        .map(|c| (c.name.as_str(), c.path.as_str()))
        .collect();
    for (engine_name, engine_path) in engines {
        let marker = format!("{engine_name}-");
        for m in &d.modules {
            if m.path.starts_with(&format!("{engine_path}/")) || m.path == engine_path {
                continue;
            }
            let stem = crate_of(&m.path).1;
            let leaf = stem.rsplit('/').next().unwrap_or("");
            if leaf.starts_with(&marker) {
                out.push(Violation {
                    check: "engine-own-container".into(),
                    file: m.path.clone(),
                    line: 0,
                    severity: Severity::Error,
                    message: format!(
                        "engine `{engine_name}` has a variant outside its home: `{}` is not under `{engine_path}`",
                        m.path
                    ),
                    hint: format!(
                        "move the variant inside the engine's own container `{engine_path}` and register it in the manifest"
                    ),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::manifest::{Container, Layer};
    use crate::search::digest::Module;

    fn module(path: &str, deps: &[&str]) -> Module {
        Module {
            path: path.into(),
            lang: "rust".into(),
            signatures: vec![],
            deps: deps.iter().map(|s| s.to_string()).collect(),
            body_hash: "x".into(),
            inventory: vec![],
            mods: vec![],
        }
    }

    fn manifest_engine() -> Manifest {
        Manifest {
            schema_version: 1,
            layers: vec![Layer {
                name: "engines".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                containers: vec![Container {
                    name: "billing".into(),
                    path: "crates/t/src/billing".into(),
                    description: String::new(),
                    sequence: vec![],
                    provides: vec![],
                    role: Some("engine".into()),
                    gateway: None,
                    connects: vec![],
                    components: vec![],
                }],
            }],
        }
    }

    #[test]
    fn two_node_cycle_is_reported_as_a_ring() {
        let d = Digest {
            generated_at: "now".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![
                module("crates/t/src/a.rs", &["crate::b"]),
                module("crates/t/src/b.rs", &["crate::a"]),
                module("crates/t/src/leaf.rs", &["crate::a"]),
            ],
        };
        let mut v = Vec::new();
        check_no_cycles(Path::new("."), &d, &mut v);
        assert_eq!(v.len(), 1, "one ring, whatever member closed the DFS");
        assert_eq!(v[0].check, "no-cycles");
        assert_eq!(v[0].severity, Severity::Error);
        assert!(
            v[0]
                .message
                .contains("crates/t/src/a.rs -> crates/t/src/b.rs -> crates/t/src/a.rs"),
            "the ring is named with its members, was: {}",
            v[0].message
        );
    }

    #[test]
    fn self_references_and_acyclic_graphs_stay_silent() {
        let d = Digest {
            generated_at: "now".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![
                module("crates/t/src/selfy.rs", &["crate::selfy"]),
                module("crates/t/src/chain_top.rs", &["crate::chain_mid"]),
                module("crates/t/src/chain_mid.rs", &["crate::chain_bot"]),
                module("crates/t/src/chain_bot.rs", &[]),
            ],
        };
        let mut v = Vec::new();
        check_no_cycles(Path::new("."), &d, &mut v);
        assert!(v.is_empty(), "self-import is a no-op edge; chains are not cycles");
    }

    #[test]
    fn engine_variants_must_live_in_the_engine_container() {
        let manifest = manifest_engine();
        let d = Digest {
            generated_at: "now".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![
                module("crates/t/src/billing/currency.rs", &[]),
                module("crates/t/src/api/billing-taxes.rs", &[]),
                module("crates/t/src/api/otherthing.rs", &[]),
            ],
        };
        let mut v = Vec::new();
        check_engine_own_container(&manifest, &d, &mut v);
        assert_eq!(v.len(), 1);
        assert!(v[0].file.ends_with("billing-taxes.rs"));
        assert!(v[0].message.contains("billing-taxes.rs"));
        assert!(v[0].hint.contains("crates/t/src/billing"));

        // No engine declared: nothing gated.
        let plain = Manifest {
            layers: vec![Layer {
                name: "x".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                containers: vec![Container {
                    name: "billing".into(),
                    path: "crates/t/src/billing".into(),
                    description: String::new(),
                    sequence: vec![],
                    provides: vec![],
                    role: None,
                    gateway: None,
                    connects: vec![],
                    components: vec![],
                }],
            }],
            ..crate::schema::manifest::Manifest::default()
        };
        let mut v2 = Vec::new();
        check_engine_own_container(&plain, &d, &mut v2);
        assert!(v2.is_empty(), "opt-in: containers without role owe nothing");
    }
}
