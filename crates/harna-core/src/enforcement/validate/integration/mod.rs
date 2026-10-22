//! Check `route-drift` (1.1.7): the frontend calls only routes the backend
//! declares.
//!
//! The seam between a backend and its frontend is a list of paths both
//! sides spell by hand; an agent renames one side and the other keeps
//! calling the old path until a user meets the 404. The bound rule says
//! how each side spells a route, as regexes in `values`, so any framework
//! fits (tools, not prescriptions):
//! - `server:<regex>` — its first capture group is a declared route
//!   (`server:\.(?:get|post|put|patch|delete)\(\s*["'](/[^"']*)` for
//!   Express, `server:route\(\s*"(/[^"]*)"` for axum);
//! - `client:<regex>` — its first capture group is a called route
//!   (`client:fetch\(\s*[`"'](/api/[^`"'?]*)`);
//! - `server-files:<glob>` / `client-files:<glob>` — where each side lives
//!   (default: every file);
//! - `prefix:<path>` — a mount prefix the client adds and the server's
//!   router does not spell (`prefix:/api`).
//!
//! Parameters compare equal however they are spelled: `:id`, `{id}`,
//! `<id>`, `[id]` and `${id}` are all one segment.

use std::collections::BTreeSet;
use std::path::Path;

use regex::Regex;

use super::Violation;
use crate::schema::model::rules::{Rule, Rules};
use crate::schema::model::scope::glob_match;

/// One rule's two sides, compiled.
struct Sides {
    server: Vec<Regex>,
    client: Vec<Regex>,
    server_files: Vec<String>,
    client_files: Vec<String>,
    prefixes: Vec<String>,
}

fn compile(rule: &Rule, out: &mut Vec<Violation>) -> Option<Sides> {
    let mut s = Sides {
        server: Vec::new(),
        client: Vec::new(),
        server_files: Vec::new(),
        client_files: Vec::new(),
        prefixes: Vec::new(),
    };
    let mut problem = None;
    for v in &rule.values {
        let (key, raw) = v.split_once(':').unwrap_or(("", v));
        match key {
            "server" | "client" => match Regex::new(raw) {
                Ok(re) if re.captures_len() > 1 => {
                    if key == "server" {
                        s.server.push(re)
                    } else {
                        s.client.push(re)
                    }
                }
                Ok(_) => problem = Some(format!("`{v}` needs a capture group: the route")),
                Err(e) => problem = Some(format!("`{v}` does not compile: {e}")),
            },
            "server-files" => s.server_files.push(raw.to_string()),
            "client-files" => s.client_files.push(raw.to_string()),
            "prefix" => s.prefixes.push(raw.trim_end_matches('/').to_string()),
            _ => {
                problem = Some(format!(
                    "value `{v}` is none of server:, client:, server-files:, client-files:, prefix:"
                ))
            }
        }
    }
    if problem.is_none() && (s.server.is_empty() || s.client.is_empty()) {
        problem = Some("route-drift needs a server:<regex> and a client:<regex>".into());
    }
    if let Some(why) = problem {
        out.push(Violation {
            check: "harna-data".into(),
            file: ".harna/rules.toml".into(),
            line: 0,
            severity: crate::enforcement::violation::Severity::Error,
            message: format!("rule `{}`: {why} — it checks nothing until fixed", rule.id),
            hint: "fix the rule's values (harna try-rule shows what a regex catches)".into(),
        });
        return None;
    }
    Some(s)
}

/// One route as a comparable shape: lowercase segments, every parameter
/// one `:` segment, no query, no trailing slash.
pub fn shape(route: &str) -> String {
    let path = route.split(['?', '#']).next().unwrap_or(route);
    let segs: Vec<String> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let param = s.starts_with(':')
                || (s.starts_with('{') && s.ends_with('}'))
                || (s.starts_with('<') && s.ends_with('>'))
                || (s.starts_with('[') && s.ends_with(']'))
                || s.contains("${");
            if param {
                ":".to_string()
            } else {
                s.to_ascii_lowercase()
            }
        })
        .collect();
    format!("/{}", segs.join("/"))
}

/// (route shape, file, line) for every capture of `res` in `files`.
fn routes(root: &Path, files: &[String], res: &[Regex]) -> Vec<(String, String, usize)> {
    let mut out = Vec::new();
    for f in files {
        let Ok(src) = std::fs::read_to_string(root.join(f)) else {
            continue;
        };
        for (n, line) in crate::search::digest::live::live_code(&src) {
            for re in res {
                for c in re.captures_iter(line) {
                    if let Some(m) = c.get(1) {
                        out.push((shape(m.as_str()), f.clone(), n));
                    }
                }
            }
        }
    }
    out
}

/// `files`: the repository's source files (the map's modules).
pub fn check(
    root: &Path,
    rules: &Rules,
    env: Option<&str>,
    files: &[String],
    out: &mut Vec<Violation>,
) {
    for rule in rules.rules.iter().filter(|r| {
        !r.is_deprecated() && r.matches_env(env) && r.check.as_deref() == Some("route-drift")
    }) {
        let Some(s) = compile(rule, out) else {
            continue;
        };
        // a side with globs is read from the tree (any language: a Go or
        // Python server is not in the map); without, from the map's files
        let pick = |globs: &[String]| -> Vec<String> {
            let all: Vec<String> = if globs.is_empty() {
                files.to_vec()
            } else {
                crate::search::digest::respected_files_where(
                    root,
                    crate::search::digest::MAX_FILES,
                    |f| globs.iter().any(|g| glob_match(g, f)),
                )
            };
            all.into_iter()
                .filter(|f| !crate::search::digest::is_test_path(f))
                .collect()
        };
        let server_files = pick(&s.server_files);
        let client_files = pick(&s.client_files);
        let declared: BTreeSet<String> = routes(root, &server_files, &s.server)
            .into_iter()
            .map(|(r, _, _)| r)
            .collect();
        let strip = |r: &str| -> String {
            for p in &s.prefixes {
                if let Some(rest) = r.strip_prefix(p.as_str()) {
                    if rest.is_empty() || rest.starts_with('/') {
                        return shape(rest);
                    }
                }
            }
            r.to_string()
        };
        let mut seen = BTreeSet::new();
        for (called, file, line) in routes(root, &client_files, &s.client) {
            let bare = strip(&called);
            if declared.contains(&called) || declared.contains(&bare) {
                continue;
            }
            if !seen.insert((called.clone(), file.clone())) {
                continue;
            }
            out.push(Violation {
                check: "route-drift".into(),
                file,
                line,
                severity: crate::enforcement::checks::severity_of("route-drift"),
                message: format!(
                    "calls `{called}`, which no server route declares (rule `{}`): renamed or removed on one side only",
                    rule.id
                ),
                hint: if declared.is_empty() {
                    "the rule's server:<regex> matched no route at all — check it with harna try-rule".into()
                } else {
                    "call a declared route, or add the route to the server in the same change".into()
                },
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_compare_equal_however_spelled() {
        for r in [
            "/users/:id",
            "/users/{id}",
            "/users/<id>",
            "/users/[id]",
            "/users/${user.id}",
            "/Users/:id/?x=1",
        ] {
            assert_eq!(shape(r), "/users/:", "{r}");
        }
        assert_eq!(shape("/"), "/");
    }

    #[test]
    fn a_client_call_with_no_server_route_is_named() {
        let root = std::env::temp_dir().join(format!("harna-routes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("server")).unwrap();
        std::fs::create_dir_all(root.join("web")).unwrap();
        std::fs::write(
            root.join("server/app.ts"),
            "router.get('/users/:id', show);\nrouter.post('/orders', create);\n",
        )
        .unwrap();
        std::fs::write(
            root.join("web/api.ts"),
            "fetch(`/api/users/${id}`);\nfetch('/api/orders');\nfetch('/api/invoices');\n// fetch('/api/old')\n",
        )
        .unwrap();
        let rules = Rules {
            rules: vec![Rule {
                id: "routes".into(),
                text: "t".into(),
                check: Some("route-drift".into()),
                values: vec![
                    r#"server:\.(?:get|post)\(\s*['"](/[^'"]*)"#.into(),
                    r#"client:fetch\(\s*[`'"](/api/[^`'"?]*)"#.into(),
                    "server-files:server/**".into(),
                    "client-files:web/**".into(),
                    "prefix:/api".into(),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let files = vec!["server/app.ts".to_string(), "web/api.ts".to_string()];
        let mut out = Vec::new();
        check(&root, &rules, None, &files, &mut out);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].message.contains("/api/invoices"));
        assert_eq!(out[0].line, 3);
        let _ = std::fs::remove_dir_all(&root);
    }
}
