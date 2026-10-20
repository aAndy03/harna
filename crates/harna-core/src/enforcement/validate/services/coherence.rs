//! Check `service-coherence` (0.8.0): each service is one thing the data
//! can stand on. Names are unique; every container a service runs exists
//! and runs in that service alone; every resource has one owning service;
//! a declared deploy folder exists.

use std::collections::BTreeMap;
use std::path::Path;

use super::super::Violation;
use crate::schema::model::manifest::Manifest;
use crate::schema::store::services::Services;

pub fn check(root: &Path, services: &Services, manifest: &Manifest, out: &mut Vec<Violation>) {
    let mut push = |message: String, hint: &str| {
        out.push(Violation {
            check: "service-coherence".into(),
            file: ".harna/services.toml".into(),
            line: 0,
            severity: crate::enforcement::checks::severity_of("service-coherence"),
            message,
            hint: hint.into(),
        });
    };
    let containers: Vec<&str> = manifest
        .layers
        .iter()
        .flat_map(|l| l.containers.iter().map(|c| c.name.as_str()))
        .collect();
    let mut names: BTreeMap<&str, usize> = BTreeMap::new();
    let mut runs_in: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    // resources compare case-insensitively (0.8.4: DB:Ledger and
    // db:ledger were two resources)
    let mut owned_by: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut deploys: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let container_path = |name: &str| {
        manifest
            .layers
            .iter()
            .flat_map(|l| &l.containers)
            .find(|x| x.name == name)
            .map(|x| crate::schema::model::scope::normalize_scope(&x.path))
    };
    for s in &services.services {
        *names.entry(s.name.as_str()).or_default() += 1;
        for c in &s.containers {
            if !containers.contains(&c.as_str()) {
                push(
                    format!("service `{}` runs container `{c}`, which the manifest does not declare", s.name),
                    "declare it (harna container add) or drop it from the service (harna service set)",
                );
            }
            runs_in.entry(c.as_str()).or_default().push(&s.name);
        }
        for r in &s.owns {
            if !r
                .split_once(':')
                .is_some_and(|(k, n)| !k.trim().is_empty() && !n.trim().is_empty())
            {
                push(
                    format!("service `{}` owns `{r}`, which names no kind", s.name),
                    "write resources as kind:name (db:ledger, queue:sync)",
                );
            }
            owned_by
                .entry(crate::schema::store::services::normalize_resource(r).to_ascii_lowercase())
                .or_default()
                .push(&s.name);
        }
        if !s.runs.is_empty() {
            let runs = crate::schema::model::scope::normalize_scope(&s.runs);
            deploys.entry(runs.clone()).or_default().push(&s.name);
            // deploying from code another service runs (the repository root
            // deploys everything: no overlap to name)
            for o in services
                .services
                .iter()
                .filter(|o| o.name != s.name && !runs.is_empty())
            {
                if let Some(p) = o
                    .containers
                    .iter()
                    .filter_map(|c| container_path(c))
                    .find(|p| {
                        crate::schema::model::scope::path_within(p, &runs)
                            || crate::schema::model::scope::path_within(&runs, p)
                    })
                {
                    push(
                        format!(
                            "service `{}` deploys from `{runs}`, which holds `{p}` that service `{}` runs",
                            s.name, o.name
                        ),
                        "point --runs at this service's own crate or package",
                    );
                }
            }
        }
        if !s.runs.is_empty() && !root.join(&s.runs).exists() {
            push(
                format!(
                    "service `{}` deploys from `{}`, which does not exist",
                    s.name, s.runs
                ),
                "point --runs at the service's crate or package folder, or clear it",
            );
        }
    }
    for (n, count) in names {
        if count > 1 {
            push(
                format!("service `{n}` is declared {count} times"),
                "one entry per service: merge them",
            );
        }
    }
    for (c, owners) in runs_in {
        if owners.len() > 1 {
            push(
                format!("container `{c}` runs in {} services: {}", owners.len(), owners.join(", ")),
                "code shared by services belongs to no service (shared code is linked, not run); drop it from all but one",
            );
        }
    }
    for (folder, who) in deploys {
        if who.len() > 1 {
            push(
                format!("services {} all deploy from `{folder}`", who.join(", ")),
                "one deploy unit per service",
            );
        }
    }
    for (r, owners) in owned_by {
        if owners.len() > 1 {
            push(
                format!("resource `{r}` is owned by {} services: {}", owners.len(), owners.join(", ")),
                "one service writes it; the others ask that service (a corridor), or the resource splits",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::{Container, Layer};
    use crate::schema::store::services::Service;

    /// 0.8.0: unknown containers, a container in two services, a resource
    /// with two owners, a missing deploy folder and a repeated name are
    /// each named; a coherent declaration passes.
    #[test]
    fn services_hold_together() {
        let m = Manifest {
            layers: vec![Layer {
                name: "core".into(),
                containers: ["ledger", "sync", "money"]
                    .iter()
                    .map(|n| Container {
                        name: n.to_string(),
                        path: format!("crates/{n}/src"),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let svc = |name: &str, containers: &[&str], owns: &[&str]| Service {
            name: name.into(),
            containers: containers.iter().map(|s| s.to_string()).collect(),
            owns: owns.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        let root = std::env::temp_dir();
        let good = Services {
            services: vec![
                svc("api", &["ledger"], &["db:ledger"]),
                svc("worker", &["sync"], &["queue:sync"]),
            ],
            ..Default::default()
        };
        let mut out = Vec::new();
        check(&root, &good, &m, &mut out);
        assert!(out.is_empty(), "{out:?}");
        let mut bad = good.clone();
        bad.services
            .push(svc("api", &["ledger", "ghost"], &["db:ledger"]));
        bad.services[1].runs = "no/such/folder".into();
        let mut out = Vec::new();
        check(&root, &bad, &m, &mut out);
        let msgs: Vec<&str> = out.iter().map(|v| v.message.as_str()).collect();
        assert_eq!(msgs.len(), 5, "{msgs:#?}");
        assert!(msgs.iter().any(|m| m.contains("`ghost`")));
        assert!(msgs.iter().any(|m| m.contains("no/such/folder")));
        assert!(msgs.iter().any(|m| m.contains("declared 2 times")));
        assert!(msgs
            .iter()
            .any(|m| m.contains("`ledger` runs in 2 services")));
        assert!(msgs.iter().any(|m| m.contains("`db:ledger` is owned by 2")));
    }
}
