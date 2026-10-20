//! `kind-coverage` (0.4.10, field test 049): "every view has a UI
//! integration test" as a check, not prose. A rule binds the check, names
//! the test kind(s) in `values` and the code in `scope`; every registered
//! component there must be named by a file of one of those kinds (the
//! same reading as `harna tests` — by module name, file stem or declared
//! surface). The kinds are the project's own (`harna test-kind add`).

use std::path::Path;

use super::Violation;
use crate::enforcement::validate::coherence::test_coverage::by_kind;
use crate::schema::model::manifest::{Component, Manifest};
use crate::schema::model::rules::Rules;
use crate::schema::model::scope::path_within;

pub fn check(
    root: &Path,
    rules: &Rules,
    env: Option<&str>,
    manifest: &Manifest,
    out: &mut Vec<Violation>,
) {
    let bound = crate::enforcement::validate::hygiene::bound(rules, env, "kind-coverage");
    if bound.is_empty() {
        return;
    }
    let files = by_kind::files(root, manifest);
    let severity = crate::enforcement::checks::severity_of("kind-coverage");
    for rule in bound {
        let unknown: Vec<&String> = rule
            .values
            .iter()
            .filter(|v| !manifest.test_kinds.iter().any(|k| &k.name == *v))
            .collect();
        if rule.values.is_empty() || !unknown.is_empty() {
            out.push(Violation {
                check: "kind-coverage".into(),
                file: ".harna/rules.toml".into(),
                line: 0,
                severity,
                message: format!(
                    "rule `{}` names {} — kind-coverage needs declared test kinds in its values",
                    rule.id,
                    if rule.values.is_empty() {
                        "no test kind".to_string()
                    } else {
                        format!(
                            "undeclared kind(s) {}",
                            unknown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                        )
                    }
                ),
                hint: "declare the kind (`harna test-kind add ui --path ...`) and name it: `--value ui`".into(),
            });
            continue;
        }
        let covered: std::collections::BTreeSet<&str> = files
            .iter()
            .filter(|f| rule.values.contains(&f.kind))
            .flat_map(|f| f.covers.iter().map(String::as_str))
            .collect();
        let mut comps: Vec<&Component> = Vec::new();
        fn walk<'m>(cs: &'m [Component], out: &mut Vec<&'m Component>) {
            for c in cs {
                out.push(c);
                walk(&c.children, out);
            }
        }
        for l in &manifest.layers {
            for c in &l.containers {
                walk(&c.components, &mut comps);
            }
        }
        for c in comps {
            if c.planned || c.path.is_empty() || covered.contains(c.name.as_str()) {
                continue;
            }
            if !rule.scope.is_empty() && !rule.scope.iter().any(|s| path_within(&c.path, s)) {
                continue;
            }
            out.push(Violation {
                check: "kind-coverage".into(),
                file: c.path.clone(),
                line: 0,
                severity,
                message: format!(
                    "component `{}` is named by no `{}` test (rule `{}`)",
                    c.name,
                    rule.values.join("`/`"),
                    rule.id
                ),
                hint: format!(
                    "write a {} test that exercises it — {}",
                    rule.values.join("/"),
                    rule.rationale
                ),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::{Container, Layer, TestKind};
    use crate::schema::model::rules::Rule;

    /// 0.4.10: "every view has a UI test" — the view a ui test names
    /// passes, the one nobody names is reported, a slot is not code yet.
    #[test]
    fn components_in_scope_need_a_test_of_the_kind() {
        let root = std::env::temp_dir().join(format!("harna-kind-cov-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let w = |rel: &str, body: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        w("Cargo.toml", "[package]\nname = \"app\"\n");
        w(
            "src/lib.rs",
            "pub mod login_view;\npub mod accounts_view;\n",
        );
        w("src/login_view.rs", "pub struct LoginView;\n");
        w("src/accounts_view.rs", "pub struct AccountsView;\n");
        w("tests/ui_login.rs", "use app::login_view::LoginView;\n");
        let comp = |name: &str, path: &str, planned: bool| Component {
            name: name.into(),
            path: path.into(),
            planned,
            ..Default::default()
        };
        let m = Manifest {
            layers: vec![Layer {
                name: "features".into(),
                containers: vec![Container {
                    name: "views".into(),
                    path: "src".into(),
                    components: vec![
                        comp("login-view", "src/login_view.rs", false),
                        comp("accounts-view", "src/accounts_view.rs", false),
                        comp("report-view", "src/report_view.rs", true),
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            test_kinds: vec![TestKind {
                name: "ui".into(),
                paths: vec!["tests/ui_*.rs".into()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut rules = Rules::default();
        rules.rules.push(Rule {
            id: "views-have-ui-tests".into(),
            check: Some("kind-coverage".into()),
            values: vec!["ui".into()],
            scope: vec!["src".into()],
            ..Default::default()
        });
        let mut out = Vec::new();
        check(&root, &rules, None, &m, &mut out);
        let files: Vec<&str> = out.iter().map(|v| v.file.as_str()).collect();
        assert_eq!(files, vec!["src/accounts_view.rs"], "{out:?}");
        rules.rules[0].values = vec!["uii".into()];
        let mut out = Vec::new();
        check(&root, &rules, None, &m, &mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].message.contains("undeclared kind"), "{out:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
