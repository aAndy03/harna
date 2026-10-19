//! `file-header` (E2): files in scope begin with a line matching the
//! rule's `pattern` (deno: `// Copyright 2018-2026 the Deno authors. MIT
//! license.` on every JS/TS/Rust/C file). `values` are the extensions
//! covered (default `rs`). The header may sit anywhere in the leading
//! comment block (after a shebang, lint directives, an upstream notice).

use std::path::Path;

use regex::Regex;

use super::{bound, files_in, finding, Violation};
use crate::schema::model::rules::Rules;

pub fn check(root: &Path, rules: &Rules, env: Option<&str>, out: &mut Vec<Violation>) {
    for rule in bound(rules, env, "file-header") {
        let Some(re) = rule.pattern.as_deref().and_then(|p| Regex::new(p).ok()) else {
            out.push(finding(
                "file-header",
                ".harna/rules.toml",
                0,
                format!(
                    "rule `{}` binds file-header without a valid `pattern`",
                    rule.id
                ),
                "set `pattern` to the header regex".into(),
            ));
            continue;
        };
        let exts: Vec<&str> = if rule.values.is_empty() {
            vec!["rs"]
        } else {
            rule.values
                .iter()
                .map(|v| v.trim_start_matches('.'))
                .collect()
        };
        let scopes: Vec<&str> = if rule.scope.is_empty() {
            vec![""]
        } else {
            rule.scope.iter().map(String::as_str).collect()
        };
        for scope in scopes {
            for file in files_in(root, scope) {
                let ext = file.rsplit('.').next().unwrap_or("");
                if !exts.contains(&ext) {
                    continue;
                }
                let Ok(src) = std::fs::read_to_string(root.join(&file)) else {
                    continue;
                };
                // The header may sit anywhere in the leading comment block —
                // after a shebang, lint directives or an upstream copyright
                // (deno's checker allows exactly that preamble).
                let found = src
                    .lines()
                    .map(str::trim)
                    .take_while(|l| {
                        l.is_empty()
                            || l.starts_with("//")
                            || l.starts_with("#")
                            || l.starts_with("/*")
                            || l.starts_with('*')
                    })
                    .any(|l| re.is_match(l));
                if !found {
                    out.push(finding(
                        "file-header",
                        &file,
                        1,
                        format!("missing the header rule `{}` requires", rule.id),
                        format!("start the file with a line matching `{}`", re.as_str()),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::rules::Rule;

    #[test]
    fn files_without_the_header_are_named() {
        let root = std::env::temp_dir().join(format!(
            "harna-headers-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("ext")).unwrap();
        std::fs::write(
            root.join("ext/a.rs"),
            "// Copyright 2018-2026 the Deno authors. MIT license.\nfn a() {}\n",
        )
        .unwrap();
        std::fs::write(root.join("ext/b.rs"), "fn b() {}\n").unwrap();
        // Code before the header: it no longer leads the file.
        std::fs::write(
            root.join("ext/e.rs"),
            "use x;\n// Copyright 2018-2026 the Deno authors. MIT license.\n",
        )
        .unwrap();
        std::fs::write(
            root.join("ext/c.js"),
            "#!/usr/bin/env node\n// deno-lint-ignore-file\n// Copyright 2018 Upstream. MIT.\n// Copyright 2018-2026 the Deno authors. MIT license.\nlet x = 1;\n",
        )
        .unwrap();
        std::fs::write(root.join("ext/d.md"), "no header needed\n").unwrap();
        let mut rules = Rules::default();
        rules.rules.push(Rule {
            id: "copyright".into(),
            text: "t".into(),
            rationale: "r".into(),
            origin: "o".into(),
            check: Some("file-header".into()),
            pattern: Some(r"^// Copyright \d{4}-\d{4} the Deno authors\. MIT license\.$".into()),
            values: vec!["rs".into(), "js".into()],
            ..Default::default()
        });
        let mut out = Vec::new();
        check(&root, &rules, None, &mut out);
        let files: Vec<&str> = out.iter().map(|v| v.file.as_str()).collect();
        assert_eq!(files, vec!["ext/b.rs", "ext/e.rs"], "{out:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
