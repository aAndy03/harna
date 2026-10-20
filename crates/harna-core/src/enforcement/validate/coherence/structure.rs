//! Component `structure` (0.3.6): two structural invariants read from the
//! committed map:
//! - `no-cycles`: the import graph stays acyclic. Each finding is an
//!   import that breaks rings (0.3.48, H12: the feedback edges), named with
//!   how many rings it closes and one of them (a → b → a).
//! - `engine-own-container`: containers that declare `role = "engine"`
//!   own their family — a module whose file leaf starts `<engine>-` may
//!   not live outside the engine's container path (opt-in: containers
//!   without a role owe nothing).

use std::collections::BTreeMap;
use std::path::Path;

use super::{Severity, Violation};
use crate::schema::model::manifest::Manifest;
use crate::search::digest::{crate_name_dirs, resolve_dep, workspace_crate_names, Digest};

pub fn check(root: &Path, manifest: &Manifest, digest: Option<&Digest>, out: &mut Vec<Violation>) {
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
        .filter_map(|m| m.crate_of().0)
        .filter(|dir| dir != ".")
        .collect();
    let name_dirs = crate_name_dirs(root, &dirs.into_iter().collect::<Vec<_>>());
    let hosts = crate::search::digest::HostIndex::new(d);

    let mut graph: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for m in &d.modules {
        let (importer_dir, importer_stem) = m.crate_of();
        for dep in &m.deps {
            let Some(target) = resolve_dep(dep, importer_stem.as_str(), &crate_names) else {
                continue;
            };
            let Some(target_module) = hosts.hosted(&name_dirs, importer_dir.as_deref(), &target)
            else {
                continue;
            };
            if target_module.path != m.path && !is_facade_edge(m, target_module) {
                graph
                    .entry(m.path.as_str())
                    .or_default()
                    .push(target_module.path.as_str());
            }
        }
    }
    graph
}

/// A parent module reaching its own descendant (`lib.rs` → `cache/node.rs`,
/// `cache/mod.rs` → `cache/caches.rs`) is the module tree's declaration
/// and re-export structure, not a dependency (0.3.45, deno study A7: 366
/// "cycles" after adoption were `pub use child::X` facades whose children
/// use `super::`/`crate::` items). Sibling and cross-tree rings still count.
fn is_facade_edge(
    parent: &crate::search::digest::Module,
    child: &crate::search::digest::Module,
) -> bool {
    let (p_dir, p_stem) = parent.crate_of();
    let (c_dir, c_stem) = child.crate_of();
    if p_dir != c_dir {
        return false;
    }
    if parent.is_crate_root() {
        return !c_stem.is_empty() && c_stem != p_stem;
    }
    c_stem.starts_with(&format!("{p_stem}/"))
}

fn check_no_cycles(root: &Path, d: &Digest, out: &mut Vec<Violation>) {
    let graph = module_graph(root, d);
    // One finding per edge that breaks rings (0.3.48, H12), not one per
    // ring: hundreds of rings after adoption share a few imports.
    for f in feedback::edges(&graph) {
        out.push(Violation {
            check: "no-cycles".into(),
            file: f.from.clone(),
            line: 0,
            severity: Severity::Error,
            message: format!(
                "import cycle: `{} -> {}` closes {} ring(s), e.g. {}",
                f.from,
                f.to,
                f.rings,
                f.example.join(" -> ")
            ),
            hint: format!(
                "removing this one import breaks {} ring(s): invert it (move the shared piece below both), route through a transiter, or re-home a module",
                f.rings
            ),
        });
    }
}

pub mod feedback;

// ----- engine-own-container -----

fn check_engine_own_container(manifest: &Manifest, d: &Digest, out: &mut Vec<Violation>) {
    let engines: Vec<(&str, &str)> = manifest
        .layers
        .iter()
        .flat_map(|l| l.containers.iter())
        .filter(|c| {
            c.role
                .as_deref()
                .is_some_and(|r| r.eq_ignore_ascii_case("engine"))
        })
        .map(|c| (c.name.as_str(), c.path.as_str()))
        .collect();
    for (engine_name, engine_path) in engines {
        let marker = format!("{engine_name}-");
        for m in &d.modules {
            if m.path.starts_with(&format!("{engine_path}/")) || m.path == engine_path {
                continue;
            }
            let stem = m.crate_of().1;
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
    use crate::schema::model::manifest::{Container, Layer};
    use crate::search::digest::Module;

    fn module(path: &str, deps: &[&str]) -> Module {
        Module {
            reexports: Vec::new(),
            crate_dir: None,
            src_root: None,
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
            test_kinds: Vec::new(),
            schema_version: 1,
            layers: vec![Layer {
                owners: Vec::new(),
                isolated: false,
                name: "engines".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                role: None,
                containers: vec![Container {
                    owners: Vec::new(),
                    escapes: Vec::new(),
                    owns: Vec::new(),
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
            orphans: Vec::new(),
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
            v[0].message
                .contains("crates/t/src/a.rs -> crates/t/src/b.rs -> crates/t/src/a.rs"),
            "the ring is named with its members, was: {}",
            v[0].message
        );
    }

    #[test]
    fn self_references_and_acyclic_graphs_stay_silent() {
        let d = Digest {
            orphans: Vec::new(),
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
        assert!(
            v.is_empty(),
            "self-import is a no-op edge; chains are not cycles"
        );
    }

    /// deno study A7 (0.3.45): `cache/mod.rs` re-exports `caches`, and
    /// `caches.rs` uses `super::` items — structure, not a cycle. A ring
    /// between siblings through the facade still reports.
    #[test]
    fn facade_edges_to_own_children_are_not_cycles() {
        let d = Digest {
            orphans: Vec::new(),
            generated_at: "now".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![
                module("crates/t/src/lib.rs", &["crate::cache::Caches"]),
                module(
                    "crates/t/src/cache/mod.rs",
                    &["crate::cache::caches::Caches"],
                ),
                module("crates/t/src/cache/caches.rs", &["crate::cache::Shared"]),
                module("crates/t/src/cli.rs", &["crate::Config"]),
            ],
        };
        let mut v = Vec::new();
        check_no_cycles(Path::new("."), &d, &mut v);
        assert!(v.is_empty(), "facades only: {v:?}");

        let d = Digest {
            orphans: Vec::new(),
            generated_at: "now".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![
                module("crates/t/src/cache/mod.rs", &[]),
                module("crates/t/src/cache/a.rs", &["crate::cache::b"]),
                module("crates/t/src/cache/b.rs", &["crate::cache::a"]),
            ],
        };
        let mut v = Vec::new();
        check_no_cycles(Path::new("."), &d, &mut v);
        assert_eq!(v.len(), 1, "siblings still ring: {v:?}");
    }

    /// zed (0.3.46): a crate root named after the crate (`crates/agent/src/
    /// agent.rs`) is a facade like `lib.rs` — 445 "cycles" were this.
    #[test]
    fn a_crate_root_named_after_its_crate_is_a_facade() {
        let d = Digest {
            orphans: Vec::new(),
            generated_at: "now".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![
                module("crates/agent/src/agent.rs", &["crate::thread::Thread"]),
                module("crates/agent/src/thread.rs", &["crate::Agent"]),
            ],
        };
        let mut v = Vec::new();
        check_no_cycles(Path::new("."), &d, &mut v);
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn engine_variants_must_live_in_the_engine_container() {
        let manifest = manifest_engine();
        let d = Digest {
            orphans: Vec::new(),
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
                owners: Vec::new(),
                isolated: false,
                name: "x".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                role: None,
                containers: vec![Container {
                    owners: Vec::new(),
                    escapes: Vec::new(),
                    owns: Vec::new(),
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
            ..crate::schema::model::manifest::Manifest::default()
        };
        let mut v2 = Vec::new();
        check_engine_own_container(&plain, &d, &mut v2);
        assert!(v2.is_empty(), "opt-in: containers without role owe nothing");
    }
}
