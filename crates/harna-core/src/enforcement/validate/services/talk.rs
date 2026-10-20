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
    with_cargo: bool,
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
    // shape file -> its corridor, for the advice
    let shape_of: HashMap<&str, &str> = services
        .corridors
        .iter()
        .flat_map(|c| {
            c.messages
                .iter()
                .filter_map(move |m| m.type_ref().map(|(f, _)| (f, c.name.as_str())))
        })
        .collect();
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
                hint: if let Some(c) = shape_of.get(target.as_str()) {
                    // the import IS a message shape (0.8.4: "send it through
                    // postings" was circular advice)
                    format!(
                        "`{target}` holds corridor `{c}`'s message shape inside service `{to}`: move the shape to a shared container both link"
                    )
                } else if corridors.is_empty() {
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
    // (only when a Cargo.toml may have changed: a scoped run skips it, 0.9.2)
    if !with_cargo {
        return;
    }
    // a Cargo dependency between two services' crates couples their builds
    // even with no import yet (0.8.4, books field test)
    let index = crate::search::digest::crates::CrateIndex::load(root);
    let crate_of_service: Vec<(&str, String)> = services
        .services
        .iter()
        .flat_map(|s| {
            s.containers.iter().filter_map(|c| {
                let path = manifest
                    .layers
                    .iter()
                    .flat_map(|l| &l.containers)
                    .find(|x| &x.name == c)?
                    .path
                    .as_str();
                let (k, _) = index.locate_dir(path)?;
                Some((s.name.as_str(), k.dir.clone()))
            })
        })
        .collect();
    let mut seen_deps: BTreeSet<(String, String)> = BTreeSet::new();
    for (svc, dir) in &crate_of_service {
        for dep in index.cargo_deps(root, dir) {
            let Some((other, _)) = crate_of_service.iter().find(|(o, d)| *d == dep && o != svc)
            else {
                continue;
            };
            if !seen_deps.insert((dir.clone(), dep.clone())) {
                continue;
            }
            let file = if dir == "." {
                "Cargo.toml".to_string()
            } else {
                format!("{dir}/Cargo.toml")
            };
            out.push(Violation {
                check: "service-talk".into(),
                file,
                line: 0,
                severity: crate::enforcement::checks::severity_of("service-talk"),
                message: format!(
                    "service `{svc}`'s crate depends on `{dep}`, the crate service `{other}` runs — services talk through corridors"
                ),
                hint: "drop the dependency; what both need moves to a shared crate".into(),
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
        check(&root, &s, &m, &d, true, &mut out);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(out.len(), 1, "{out:#?}");
        assert!(out[0]
            .message
            .contains("service `api` imports `apps/worker/sync.ts`"));
        assert!(out[0].hint.contains("through postings"));
    }

    /// 0.8.4: a Cargo path dependency between two services' crates is named
    /// even with no import in the code.
    #[test]
    fn a_cargo_dependency_between_services_is_named() {
        let root = std::env::temp_dir().join(format!("harna-talk-cargo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let w = |rel: &str, body: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        w("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
        w(
            "crates/api/Cargo.toml",
            "[package]\nname = \"api\"\nversion = \"0.1.0\"\n",
        );
        w("crates/api/src/lib.rs", "pub fn x() {}\n");
        w(
            "crates/worker/Cargo.toml",
            "[package]\nname = \"worker\"\nversion = \"0.1.0\"\n\n[dependencies]\napi = { path = \"../api\" }\n",
        );
        w("crates/worker/src/lib.rs", "pub fn y() {}\n");
        let c = |n: &str| Container {
            name: n.into(),
            path: format!("crates/{n}/src"),
            ..Default::default()
        };
        let m = Manifest {
            layers: vec![Layer {
                name: "all".into(),
                containers: vec![c("api"), c("worker")],
                ..Default::default()
            }],
            ..Default::default()
        };
        let svc = |n: &str| Service {
            name: n.into(),
            containers: vec![n.into()],
            ..Default::default()
        };
        let s = Services {
            services: vec![svc("api"), svc("worker")],
            ..Default::default()
        };
        let mut out = Vec::new();
        check(&root, &s, &m, &Digest::default(), true, &mut out);
        assert_eq!(out.len(), 1, "{out:#?}");
        assert_eq!(out[0].file, "crates/worker/Cargo.toml");
        assert!(
            out[0].message.contains("depends on `crates/api`"),
            "{}",
            out[0].message
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
