//! Check `service-talk` (0.8.2): services talk only through declared
//! corridors. Code one service runs never imports code another service
//! runs — a direct import is a call that bypasses every corridor, and
//! couples two deploys into one. Shared code (no service names its
//! container) is linked by anyone; a message shape another service needs
//! belongs there. Both Rust paths and TS/JS file edges are read.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use super::super::Violation;
use crate::schema::model::manifest::Manifest;
use crate::schema::store::services::Services;
use crate::search::digest::Digest;

pub fn check(
    root: &Path,
    services: &Services,
    manifest: &Manifest,
    d: &Digest,
    out: &mut Vec<Violation>,
) {
    if services.services.len() < 2 {
        return; // one service talks to nobody
    }
    // module -> the service running it
    let by_path: HashMap<&str, &str> = manifest
        .layers
        .iter()
        .flat_map(|l| &l.containers)
        .map(|c| (c.path.as_str(), c.name.as_str()))
        .collect();
    let homes = crate::schema::model::manifest::module_homes(manifest, d);
    let service_of = |module: &str| -> Option<&str> {
        let home = homes.get(module)?;
        let container = by_path.get(home.container_path)?;
        services.service_of(container).map(|s| s.name.as_str())
    };
    let crate_names = crate::search::digest::workspace_crate_names(root);
    let name_dirs = crate::search::digest::crate_name_dirs(
        root,
        &d.modules
            .iter()
            .filter_map(|m| m.crate_of().0)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>(),
    );
    let hosts = crate::search::digest::HostIndex::new(d);
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for m in &d.modules {
        let Some(from) = service_of(&m.path) else {
            continue;
        };
        let (dir, stem) = m.crate_of();
        for dep in &m.deps {
            let target: Option<String> = match crate::search::digest::file_edge(dep) {
                Some(edge) => edge.map(str::to_string),
                None => crate::search::digest::resolve_dep(dep, &stem, &crate_names)
                    .and_then(|t| hosts.hosted(&name_dirs, dir.as_deref(), &t))
                    .map(|t| t.path.clone()),
            };
            let Some(target) = target else {
                continue;
            };
            let Some(to) = service_of(&target) else {
                continue; // shared code, or outside the map
            };
            if to == from || !seen.insert((m.path.clone(), target.clone())) {
                continue;
            }
            let corridors: Vec<&str> = services
                .corridors
                .iter()
                .filter(|c| {
                    (c.from.iter().any(|s| s == from) && c.to.iter().any(|s| s == to))
                        || (c.from.iter().any(|s| s == to) && c.to.iter().any(|s| s == from))
                })
                .map(|c| c.name.as_str())
                .collect();
            out.push(Violation {
                check: "service-talk".into(),
                file: m.path.clone(),
                line: 0,
                severity: crate::enforcement::checks::severity_of("service-talk"),
                message: format!(
                    "service `{from}` imports `{target}`, code service `{to}` runs (`{dep}`) — services talk through corridors"
                ),
                hint: if corridors.is_empty() {
                    format!(
                        "declare a corridor (harna service corridor add … --from {from} --to {to}) and send a message; a shape both need moves to shared code"
                    )
                } else {
                    format!(
                        "send it through {} instead; a shape both need moves to shared code",
                        corridors.join(", ")
                    )
                },
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::{Container, Layer};
    use crate::schema::store::services::{Corridor, Service};
    use crate::search::digest::Module;

    /// 0.8.2: a TS module of one service importing another service's file
    /// is named (with the corridor to use); importing shared code is not.
    #[test]
    fn services_never_import_each_other() {
        let c = |n: &str| Container {
            name: n.into(),
            path: format!("apps/{n}"),
            ..Default::default()
        };
        let m = Manifest {
            layers: vec![Layer {
                name: "all".into(),
                containers: vec![c("api"), c("worker"), c("shared")],
                ..Default::default()
            }],
            ..Default::default()
        };
        let module = |path: &str, deps: &[&str]| Module {
            path: path.into(),
            lang: "typescript".into(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        let d = Digest {
            modules: vec![
                module(
                    "apps/api/post.ts",
                    &["file:apps/worker/sync.ts", "file:apps/shared/events.ts"],
                ),
                module("apps/worker/sync.ts", &["file:apps/shared/events.ts"]),
                module("apps/shared/events.ts", &[]),
            ],
            ..Default::default()
        };
        let svc = |n: &str| Service {
            name: n.into(),
            containers: vec![n.into()],
            ..Default::default()
        };
        let s = Services {
            services: vec![svc("api"), svc("worker")],
            corridors: vec![Corridor {
                name: "postings".into(),
                from: vec!["api".into()],
                to: vec!["worker".into()],
                ..Default::default()
            }],
            ..Default::default()
        };
        // an empty root: the crate index reads nothing
        let root = std::env::temp_dir().join(format!("harna-talk-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut out = Vec::new();
        check(&root, &s, &m, &d, &mut out);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(out.len(), 1, "{out:#?}");
        assert!(out[0]
            .message
            .contains("service `api` imports `apps/worker/sync.ts`"));
        assert!(out[0].hint.contains("through postings"));
    }
}
