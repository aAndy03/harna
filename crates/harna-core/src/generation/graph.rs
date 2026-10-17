//! Component `graph` (0.3.19): the dependency graph as exportable views.
//!
//! Nodes are map modules; edges are the resolved internal import pairs the
//! metrics ledger already computes (`metrics::resolved_edges` — one
//! shared resolution, no second opinion). Clusters follow the manifest:
//! containers nested in layers, in manifest order, so the layer ranks are
//! the manifest's own, not a re-derivation. Unhomed modules cluster apart
//! (adoption debt is visible, not hidden in somebody else's box).
//!
//! Pure renderers: `build` resolves placement + edges once (nodes come
//! back in manifest order, unhomed last — the renderers' first-appearance
//! grouping is therefore the manifest's order); `dot` and `mermaid` turn
//! the same view into the two exchange formats. Export prints to stdout —
//! the interface layers never gain a filesystem writer for this (fs stays
//! with memory and generation).

use std::collections::BTreeMap;
use std::path::Path;

use crate::schema::digest_data::Digest;
use crate::schema::manifest::{module_homes, Manifest};
use crate::search::metrics;

/// One node: a map module plus its manifest home.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphNode {
    /// Module path as recorded in the map.
    pub path: String,
    /// Owning layer name; empty when the module is unhomed.
    pub layer: String,
    /// Owning container name; `"unhomed"` when the module is unhomed.
    pub container: String,
}

/// The resolved view both renderers consume.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GraphView {
    /// Manifest-ordered: each container's modules in map order, unhomed last.
    pub nodes: Vec<GraphNode>,
    /// (importer module path, imported module path), deduplicated.
    pub edges: Vec<(String, String)>,
}

/// Resolve placement and import edges once — the graph's whole content.
pub fn build(root: &Path, manifest: &Manifest, digest: &Digest) -> GraphView {
    let homes = module_homes(manifest, digest);

    // Nodes in manifest order: per layer, per container, its modules in
    // map order; unhomed modules go last (debt visibly at the bottom).
    let mut nodes = Vec::new();
    for l in &manifest.layers {
        for c in &l.containers {
            for m in &digest.modules {
                match homes.get(m.path.as_str()) {
                    Some(home) if home.layer == l.name && home.container_path == c.path => {
                        nodes.push(GraphNode {
                            path: m.path.clone(),
                            layer: l.name.clone(),
                            container: c.name.clone(),
                        });
                    }
                    _ => continue,
                }
            }
        }
    }
    for m in &digest.modules {
        if !homes.contains_key(m.path.as_str()) {
            nodes.push(GraphNode {
                path: m.path.clone(),
                layer: String::new(),
                container: "unhomed".to_string(),
            });
        }
    }

    let edges = metrics::resolved_edges(root, digest)
        .into_iter()
        .map(|(from, to)| (from.to_string(), to.to_string()))
        .collect();
    GraphView { nodes, edges }
}

/// First-appearance grouping over the ordered nodes: (layer, [container,
/// [node indexes]]), plus each module path's positional id. Because
/// `build` emits nodes manifest-first, the groups ARE the manifest's
/// layer/container order.
fn groups_of(view: &GraphView) -> (Vec<(String, Vec<(String, Vec<usize>)>)>, BTreeMap<&str, usize>) {
    let mut groups: Vec<(String, Vec<(String, Vec<usize>)>)> = Vec::new();
    let mut ids: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, n) in view.nodes.iter().enumerate() {
        ids.entry(n.path.as_str()).or_insert(i);
        match groups.iter_mut().find(|(l, _)| *l == n.layer) {
            Some(g) => match g.1.iter_mut().find(|(c, _)| *c == n.container) {
                Some((_, mods)) => mods.push(i),
                None => g.1.push((n.container.clone(), vec![i])),
            },
            None => groups.push((n.layer.clone(), vec![(n.container.clone(), vec![i])])),
        }
    }
    (groups, ids)
}

fn stem_of(path: &str) -> String {
    path.trim_end_matches(".rs")
        .trim_end_matches("/mod")
        .trim_end_matches(".rs")
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_string()
}

/// DOT: layers as clusters in manifest order, containers nested inside,
/// plus a visible anchor per layer chained with invisible high-weight
/// edges — clusters alone only *suggest* vertical order, the anchor chain
/// pins the rank order. Node ids are quoted module paths; labels are
/// module stems.
pub fn dot(view: &GraphView) -> String {
    let mut out = String::from(
        "digraph harna {\n  rankdir=TB;\n  node [shape=box, fontname=\"Helvetica\", fontsize=10];\n",
    );
    let (groups, _) = groups_of(view);

    for (i, (layer, _)) in groups.iter().enumerate() {
        let name = if layer.is_empty() { "unhomed" } else { layer.as_str() };
        out.push_str(&format!(
            "  l{i} [label=\"{name}\", shape=plain, fontsize=11, style=bold];\n"
        ));
        if i > 0 {
            out.push_str(&format!("  l{} -> l{i} [style=invis, weight=100];\n", i - 1));
        }
    }

    for (i, (layer, containers)) in groups.iter().enumerate() {
        let layer_label = if layer.is_empty() { "unhomed" } else { layer.as_str() };
        out.push_str(&format!(
            "  subgraph cluster_{i} {{\n    label=\"{layer_label}\";\n    style=dashed;\n"
        ));
        for (j, (container, mods)) in containers.iter().enumerate() {
            out.push_str(&format!(
                "    subgraph cluster_{i}_{j} {{\n      label=\"{container}\";\n      style=solid;\n"
            ));
            for n in mods {
                let node = &view.nodes[*n];
                out.push_str(&format!(
                    "      \"{}\" [label=\"{}\"];\n",
                    node.path,
                    stem_of(&node.path)
                ));
            }
            out.push_str("    }\n");
        }
        out.push_str("  }\n");
    }

    for (from, to) in &view.edges {
        out.push_str(&format!("  \"{from}\" -> \"{to}\";\n"));
    }
    out.push('}');
    out
}

/// Mermaid: `flowchart TD` with nested subgraphs per layer/container —
/// same view, same order, same edges. Mermaid ids must be its own
/// identifiers, so nodes are positional (`m0..`) and labels are stems.
pub fn mermaid(view: &GraphView) -> String {
    let mut out = String::from("flowchart TD\n");
    let (groups, ids) = groups_of(view);

    for (li, (layer, containers)) in groups.iter().enumerate() {
        let layer_label = if layer.is_empty() { "unhomed" } else { layer.as_str() };
        out.push_str(&format!("  subgraph L{li}[\"{layer_label}\"]\n"));
        for (ci, (container, mods)) in containers.iter().enumerate() {
            out.push_str(&format!("    subgraph L{li}C{ci}[\"{container}\"]\n"));
            for &i in mods {
                out.push_str(&format!("      m{i}[\"{}\"]\n", stem_of(&view.nodes[i].path)));
            }
            out.push_str("    end\n");
        }
        out.push_str("  end\n");
    }

    for (from, to) in &view.edges {
        let (Some(f), Some(t)) = (ids.get(from.as_str()), ids.get(to.as_str())) else {
            continue;
        };
        out.push_str(&format!("  m{f} --> m{t}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::digest_data::Module;
    use crate::schema::manifest::{Container, Layer};

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

    fn fixture() -> (std::path::PathBuf, Manifest, Digest) {
        // resolved_edges reads workspace crate names from Cargo.toml — a
        // tiny two-member workspace on disk keeps the resolution honest.
        let tmp = std::env::temp_dir().join(format!(
            "harna-graph-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("crates/core/src/schema")).unwrap();
        std::fs::create_dir_all(tmp.join("crates/cli/src")).unwrap();
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/core\", \"crates/cli\"]\n",
        )
        .unwrap();
        std::fs::write(
            tmp.join("crates/core/Cargo.toml"),
            "[package]\nname = \"harna-core\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();
        std::fs::write(
            tmp.join("crates/cli/Cargo.toml"),
            "[package]\nname = \"harna-cli\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();

        let manifest = Manifest {
            schema_version: crate::SCHEMA_VERSION,
            layers: vec![
                Layer {
                    name: "memory".into(),
                    containers: vec![Container {
                        name: "schema".into(),
                        path: "crates/core/src/schema".into(),
                        ..Default::default()
                    }],
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec![],
                    role: None,
                },
                Layer {
                    name: "interface".into(),
                    containers: vec![Container {
                        name: "cli".into(),
                        path: "crates/cli/src".into(),
                        ..Default::default()
                    }],
                    description: String::new(),
                    sequence: vec![],
                    depends_on: vec![],
                    role: None,
                },
            ],
        };
        let digest = Digest {
            generated_at: String::new(),
            extractor_version: 0,
            modules: vec![
                module("crates/core/src/schema/store.rs", &[]),
                module("crates/core/src/schema/migrate.rs", &["crate::schema::store::Store"]),
                module("crates/cli/src/main.rs", &["harna_core::schema::store::Store"]),
                module("stray.rs", &["unknown::dep::Thing"]),
            ],
        };
        (tmp, manifest, digest)
    }

    #[test]
    fn build_resolves_homes_and_edges_one_resolution() {
        let (tmp, manifest, digest) = fixture();
        let view = build(&tmp, &manifest, &digest);

        // Homes: manifest longest-prefix; the stray lands as unhomed.
        let store = view.nodes.iter().find(|n| n.path.ends_with("store.rs")).unwrap();
        assert_eq!((store.layer.as_str(), store.container.as_str()), ("memory", "schema"));
        let stray = view.nodes.iter().find(|n| n.path == "stray.rs").unwrap();
        assert_eq!((stray.layer.as_str(), stray.container.as_str()), ("", "unhomed"));

        // Node order is manifest order: schema's modules first, cli's next,
        // the unhomed stray last (debt visibly at the bottom).
        assert_eq!(view.nodes.iter().map(|n| n.path.as_str()).collect::<Vec<_>>(), vec![
            "crates/core/src/schema/store.rs",
            "crates/core/src/schema/migrate.rs",
            "crates/cli/src/main.rs",
            "stray.rs",
        ]);

        // Edges: same resolution as the metrics ledger — one intra-crate,
        // one cross-crate; the unknown dep drops; no self-loops exist.
        assert_eq!(view.edges.len(), 2, "{:?}", view.edges);
        assert!(view
            .edges
            .iter()
            .any(|(f, t)| f.ends_with("migrate.rs") && t.ends_with("store.rs")));
        assert!(view
            .edges
            .iter()
            .any(|(f, t)| f.ends_with("cli/src/main.rs") && t.ends_with("store.rs")));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn dot_groups_layers_in_manifest_order_and_pins_ranks() {
        let (tmp, manifest, digest) = fixture();
        let view = build(&tmp, &manifest, &digest);
        let text = dot(&view);

        let memory_at = text.find("cluster_0").expect("memory cluster");
        let interface_at = text.find("cluster_1").expect("interface cluster");
        assert!(memory_at < interface_at, "manifest order: {text}");
        assert!(text.contains("digraph harna"));
        assert!(
            text.contains("l0 -> l1 [style=invis, weight=100]"),
            "rank anchors pin layers: {text}"
        );
        assert!(text.contains("\"crates/core/src/schema/store.rs\" [label=\"store\"]"));
        assert!(text.contains("\"crates/cli/src/main.rs\" -> \"crates/core/src/schema/store.rs\""));
        // The stray renders its own unhomed cluster, never inside a home.
        assert!(text.contains("label=\"unhomed\""));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn mermaid_renders_the_same_view_with_positional_ids() {
        let (tmp, manifest, digest) = fixture();
        let view = build(&tmp, &manifest, &digest);
        let text = mermaid(&view);

        assert!(text.starts_with("flowchart TD"));
        let memory_at = text.find("subgraph L0[\"memory\"]").expect("memory subgraph");
        let cli_at = text.find("subgraph L1[\"interface\"]").expect("interface subgraph");
        assert!(memory_at < cli_at);
        assert!(text.contains("subgraph L0C0[\"schema\"]"));
        assert!(text.contains("m2 --> m0"), "positional edge ids: {text}");
        assert!(!text.contains("unknown"), "unresolvable deps never render: {text}");
        assert!(!text.contains("stray.rs -->"), "the stray has no resolvable edges");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
