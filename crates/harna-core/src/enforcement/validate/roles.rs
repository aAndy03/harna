//! Component `roles` (0.3.25, layers as data capstone): the role-keyed
//! checks. The role model itself is data (`schema::manifest::ROLES` —
//! vocabulary + meanings); this container enforces what a declared role
//! promises:
//!
//! - `role-async`: async fns are inventoried by the map (kind `async-fn`) and
//!   may appear only in `orchestration-async` and `interface` roles —
//!   engines, bases, gateways, transiters and sync orchestration stay sync.
//!   Aggregate per container (one finding per role violation, not per fn).
//! - `rules-coverage`: every declared container is watched by at least one
//!   scoped, live rule (Info advisory — an unwatched container is an island,
//!   and an island is where conventions die silently).
//!
//! Adoption stays data-first: an undeclared role owes nothing
//! (`effective_role` returns None — same discipline as `provides` and
//! `gateway`).

use std::collections::BTreeMap;

use super::{module_homes, Severity, Violation};
use crate::enforcement::checks;
use crate::schema::manifest::{async_allowed, effective_role, Manifest};
use crate::schema::rules::Rules;
use crate::search::digest::Digest;

pub fn check(
    _root: &std::path::Path,
    manifest: &Manifest,
    rules: &Rules,
    env: Option<&str>,
    digest: Option<&Digest>,
    out: &mut Vec<Violation>,
) {
    check_role_async(manifest, digest, out);
    check_rules_coverage(manifest, rules, env, out);
}

/// Async inventory vs role: one finding per container whose effective role
/// cannot carry async but whose modules declare `async fn`s. Modules the
/// manifest homes nowhere are skipped (the map's adoption carve-out).
fn check_role_async(manifest: &Manifest, digest: Option<&Digest>, out: &mut Vec<Violation>) {
    let Some(d) = digest else { return };
    let severity = checks::spec("role-async")
        .map(|s| s.default_severity)
        .unwrap_or(Severity::Error);

    struct Finding {
        container: String,
        role: String,
        abs_async: usize,
        files: Vec<String>,
    }
    let mut findings: BTreeMap<&str, Finding> = BTreeMap::new();
    let homes = module_homes(manifest, d);

    for m in &d.modules {
        let count = m.inventory.iter().filter(|i| i.kind == "async-fn").count();
        if count == 0 {
            continue;
        }
        let Some(home) = homes.get(m.path.as_str()) else {
            continue;
        };
        let Some(layer) = manifest.layers.iter().find(|l| l.name == home.layer) else {
            continue;
        };
        let Some(container) = layer
            .containers
            .iter()
            .find(|c| c.path == home.container_path)
        else {
            continue;
        };
        let Some(role) = effective_role(layer, container) else {
            continue; // undeclared role owes nothing
        };
        if async_allowed(role) {
            continue;
        }
        let finding = findings
            .entry(home.container_path)
            .or_insert(Finding {
                container: container.name.clone(),
                role: role.to_string(),
                abs_async: 0,
                files: Vec::new(),
            });
        finding.abs_async += count;
        if finding.files.len() < 3 {
            finding.files.push(m.path.clone());
        }
    }

    for (path, f) in findings {
        out.push(Violation {
            check: "role-async".into(),
            file: path.to_string(),
            line: 0,
            severity,
            message: format!(
                "container `{}` (role {}) carries {} async fn(s) across the map — async lives only in the orchestration-async and interface roles",
                f.container, f.role, f.abs_async
            ),
            hint: "set the layer/container role to orchestration-async (async task flows) or interface (server handlers), or move the async fns — engines stay sync".into(),
        });
    }
}

/// Coverage as an advisory: a container no live rule watches is reported
/// once per container with its layer named (a rule is global when its
/// `scope` is empty, else it covers the paths its scope prefixes — the
/// same matching `Rules::for_scope` gives the briefings).
fn check_rules_coverage(
    manifest: &Manifest,
    rules: &Rules,
    env: Option<&str>,
    out: &mut Vec<Violation>,
) {
    let severity = checks::spec("rules-coverage")
        .map(|s| s.default_severity)
        .unwrap_or(Severity::Info);
    let watched = |scope: &str| -> bool {
        rules
            .rules
            .iter()
            .filter(|r| !r.is_deprecated() && r.matches_env(env))
            .any(|r| r.scope.is_empty() || r.scope.iter().any(|s| crate::schema::scope::path_within(scope, s)))
    };
    for layer in &manifest.layers {
        for c in &layer.containers {
            if c.path.is_empty() || watched(&c.path) {
                continue;
            }
            out.push(Violation {
                check: "rules-coverage".into(),
                file: ".harna/manifest.toml".into(),
                line: 0,
                severity,
                message: format!(
                    "no scoped rule watches layer `{}` container `{}` (path `{}`)",
                    layer.name, c.name, c.path
                ),
                hint: "bind a rule with this path in `scope` (or keep a global rule: empty `scope` covers everything) — an unwatched container is where conventions die silently".into(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::digest_data::{Digest, Item, Module};
    use crate::schema::manifest::{Container, Layer};
    use std::path::Path;

    fn manifest(entries: &[(&str, Option<&str>, Option<&str>)]) -> Manifest {
        // (layer, layer role, container role); container path = src/<layer>.
        let mut m = Manifest::default();
        for (name, layer_role, container_role) in entries {
            let mut l = Layer {
                name: (*name).into(),
                role: layer_role.map(Into::into),
                ..Default::default()
            };
            let mut c = Container {
                name: format!("{name}-bag"),
                path: format!("src/{name}"),
                ..Default::default()
            };
            c.role = container_role.map(Into::into);
            l.containers.push(c);
            m.layers.push(l);
        }
        m
    }

    fn digest_of(engine_items: &[(&str, &str)]) -> Digest {
        Digest {
            generated_at: "t".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![Module {
                path: "src/engines/motor.rs".into(),
                lang: "rust".into(),
                signatures: vec![],
                deps: vec![],
                body_hash: String::new(),
                inventory: engine_items
                    .iter()
                    .map(|(kind, name)| Item {
                        vis: "pub".into(),
                        kind: (*kind).into(),
                        name: (*name).into(),
                        text: String::new(),
                    })
                    .collect(),
                mods: vec![],
            }],
        }
    }

    fn run(m: &Manifest, rules: &Rules, d: Option<&Digest>) -> Vec<Violation> {
        let mut out = Vec::new();
        check(Path::new("."), m, rules, None, d, &mut out);
        out
    }

    fn rule(id: &str, scope: &[&str]) -> crate::schema::rules::Rule {
        crate::schema::rules::Rule {
            id: id.into(),
            text: format!("rule {id}"),
            rationale: "test".into(),
            scope: scope.iter().map(|s| (*s).into()).collect(),
            origin: "test".into(),
            ..Default::default()
        }
    }

    #[test]
    fn async_in_an_engine_container_is_an_error() {
        let m = manifest(&[("engines", Some("engine"), None), ("agents", Some("interface"), None)]);
        let d = digest_of(&[("async-fn", "poll_queue"), ("fn", "sync_boring")]);
        let out = run(&m, &Rules::default(), Some(&d));
        let hits: Vec<_> = out.iter().filter(|v| v.check == "role-async").collect();
        assert_eq!(hits.len(), 1, "one finding per container, not per fn: {out:?}");
        assert_eq!(hits[0].severity, Severity::Error, "registry default gates");
        assert!(hits[0].message.contains("engines-bag"), "names the container");
        assert!(hits[0].message.contains("role engine"));
        assert_eq!(hits[0].file, "src/engines");
        // 1 async fn was inventoried exactly once.
        assert!(hits[0].message.contains("1 async fn"));
    }

    #[test]
    fn interface_and_orchestration_async_roles_carry_async() {
        let d = digest_of(&[("async-fn", "poll_queue")]);
        for (layer, role) in [("agents", "interface"), ("workers", "orchestration-async")] {
            let m = manifest(&[(layer, Some(role), None)]);
            let out = run(&m, &Rules::default(), Some(&d));
            assert!(
                out.iter().all(|v| v.check != "role-async"),
                "{role} carries async legitimately: {out:?}"
            );
        }
    }

    #[test]
    fn container_role_overrides_layer_role() {
        let d = digest_of(&[("async-fn", "poll_queue")]);
        // The layer forbids async, the container is the async orchestration:
        // no finding.
        let m = manifest(&[("engines", Some("engine"), Some("orchestration-async"))]);
        assert!(run(&m, &Rules::default(), Some(&d))
            .iter()
            .all(|v| v.check != "role-async"));
        // The inverse: layer promises async, container narrows to engine —
        // the container's own role is what counts.
        let m = manifest(&[("engines", Some("orchestration-async"), Some("engine"))]);
        assert!(run(&m, &Rules::default(), Some(&d))
            .iter()
            .any(|v| v.check == "role-async"));
    }

    #[test]
    fn undeclared_roles_owe_nothing() {
        let d = digest_of(&[("async-fn", "poll_queue")]);
        let m = manifest(&[("engines", None, None)]);
        assert!(run(&m, &Rules::default(), Some(&d))
            .iter()
            .all(|v| v.check != "role-async"));
        // Plain fn items never trip the check.
        let d = digest_of(&[("fn", "sync_boring")]);
        let m = manifest(&[("engines", Some("engine"), None)]);
        assert!(run(&m, &Rules::default(), Some(&d))
            .iter()
            .all(|v| v.check != "role-async"));
    }

    #[test]
    fn rules_coverage_advises_on_unwatched_containers() {
        let m = manifest(&[("engines", None, None), ("orphans", None, None)]);
        let rules = Rules {
            rules: vec![rule("watch-engines", &["src/engines"])],
            ..Default::default()
        };
        let out = run(&m, &rules, None);
        let hits: Vec<_> = out.iter().filter(|v| v.check == "rules-coverage").collect();
        assert_eq!(hits.len(), 1, "exactly the unwatched container: {out:?}");
        assert_eq!(hits[0].severity, Severity::Info);
        assert!(hits[0].message.contains("orphans-bag"));

        // A global rule (empty scope) covers everything.
        let global = Rules { rules: vec![rule("everywhere", &[])], ..Default::default() };
        assert!(run(&m, &global, None).is_empty());

        // A deprecated rule no longer covers: the island comes back.
        let mut dead = rule("watch-engines", &["src/engines"]);
        dead.status = Some("deprecated".into());
        let dead = Rules { rules: vec![dead], ..Default::default() };
        let out = run(&m, &dead, None);
        assert_eq!(out.iter().filter(|v| v.check == "rules-coverage").count(), 2);
    }
}
