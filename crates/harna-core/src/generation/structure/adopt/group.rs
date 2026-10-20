//! Component `adopt-group` (0.6.4, deno study A5): a draft grouped by
//! top-level directory. deno's real layers are folders of many crates
//! (`ext/`, `runtime/`, `cli/`, `libs/`), not one layer per crate; a
//! mixed repo's `web/` and `src-tauri/` likewise. `harna adopt --by-dir`
//! folds the per-crate draft into one layer per top-level folder, its
//! crates as containers, and maps the crate edges onto the groups — a
//! group depends on another when any of its crates did.

use std::collections::BTreeMap;

use crate::schema::model::manifest::{Layer, Manifest};

/// The group a container path belongs to: its first path segment
/// (`ext/fs/lib.rs`-rooted `ext/fs` → `ext`), `root` for the repo root.
fn group_of(path: &str) -> String {
    let p = crate::schema::model::scope::normalize_scope(path);
    match p.split('/').next() {
        Some(seg) if !seg.is_empty() && p.contains('/') => seg.to_string(),
        Some(seg) if !seg.is_empty() && seg != "src" => seg.to_string(),
        // the repository root: a name no folder can take (0.6.5)
        _ => "(root)".to_string(),
    }
}

pub fn by_dir(manifest: &Manifest) -> Manifest {
    // layer (per crate) -> its group
    let group_of_layer: BTreeMap<&str, String> = manifest
        .layers
        .iter()
        .map(|l| {
            let path = l.containers.first().map(|c| c.path.as_str()).unwrap_or("");
            (l.name.as_str(), group_of(path))
        })
        .collect();
    let mut groups: BTreeMap<String, Layer> = BTreeMap::new();
    for l in &manifest.layers {
        let g = group_of_layer[l.name.as_str()].clone();
        let entry = groups.entry(g.clone()).or_insert_with(|| Layer {
            name: g.clone(),
            ..Default::default()
        });
        for c in &l.containers {
            let mut c = c.clone();
            // container names stay unique inside the group
            let base = l.name.clone();
            let mut name = base.clone();
            let mut n = 2;
            while entry.containers.iter().any(|x| x.name == name) {
                name = format!("{base}-{n}");
                n += 1;
            }
            c.name = name;
            entry.containers.push(c);
        }
        for d in &l.depends_on {
            if let Some(tg) = group_of_layer.get(d.as_str()) {
                if *tg != g && !entry.depends_on.contains(tg) {
                    entry.depends_on.push(tg.clone());
                }
            }
        }
    }
    let mut layers: Vec<Layer> = groups
        .into_values()
        .map(|mut l| {
            l.depends_on.sort();
            l.description = format!(
                "drafted from the `{}` folder — {} crate(s) as containers; a proposal to prune, not a verdict",
                l.name,
                l.containers.len()
            );
            l
        })
        .collect();
    layers.sort_by(|a, b| a.name.cmp(&b.name));
    Manifest {
        layers,
        ..manifest.clone()
    }
}

/// The layers a `depends_on` cycle leaves unrankable (the same reading
/// validate's layering check makes).
pub fn unranked(manifest: &Manifest) -> Vec<String> {
    let names: Vec<&str> = manifest.layers.iter().map(|l| l.name.as_str()).collect();
    let mut ranked: std::collections::BTreeSet<&str> = Default::default();
    loop {
        let before = ranked.len();
        for l in &manifest.layers {
            if ranked.contains(l.name.as_str()) {
                continue;
            }
            let ok = l
                .depends_on
                .iter()
                .filter(|d| names.contains(&d.as_str()))
                .all(|d| d != &l.name && ranked.contains(d.as_str()));
            if ok {
                ranked.insert(l.name.as_str());
            }
        }
        if ranked.len() == before {
            break;
        }
    }
    names
        .into_iter()
        .filter(|n| !ranked.contains(n))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::Container;

    fn layer(name: &str, path: &str, deps: &[&str]) -> Layer {
        Layer {
            name: name.into(),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            containers: vec![Container {
                name: name.into(),
                path: path.into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    /// deno's shape: crates under ext/ and runtime/ fold into two layers;
    /// a crate edge from runtime to ext becomes runtime -> ext; an edge
    /// inside a group disappears.
    #[test]
    fn crates_fold_into_their_top_level_folders() {
        let m = Manifest {
            layers: vec![
                layer("deno_fs", "ext/fs", &["deno_io"]),
                layer("deno_io", "ext/io", &[]),
                layer("deno_runtime", "runtime/src", &["deno_fs"]),
                layer("deno", "src", &["deno_runtime"]),
            ],
            ..Default::default()
        };
        let g = by_dir(&m);
        let names: Vec<&str> = g.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, vec!["(root)", "ext", "runtime"]);
        let ext = &g.layers[1];
        assert_eq!(ext.containers.len(), 2);
        assert!(
            ext.depends_on.is_empty(),
            "an edge inside the group disappears"
        );
        assert_eq!(g.layers[2].depends_on, vec!["ext"]);
        assert_eq!(g.layers[0].depends_on, vec!["runtime"]);
        assert!(unranked(&g).is_empty());
        let mut cyc = g.clone();
        cyc.layers[1].depends_on.push("runtime".into());
        assert_eq!(unranked(&cyc), vec!["(root)", "ext", "runtime"]);
    }
}
