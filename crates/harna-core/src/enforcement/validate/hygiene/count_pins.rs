//! `count-pin` (C2): a declared count of a kind in a scope. deno pins
//! `EXPECTED_OP_COUNT = 44` in `tests/unit/ops_test.ts` so an op added or
//! dropped never slips by; parity's signature diff cannot see a count.
//! The rule's `values[0]` is an inventory kind (`fn`, `struct`, `enum`,
//! `trait`, `const`, `static`, `type`, `mod`, `impl`, `async-fn`) counted
//! over the map, or a marker (`#[op2`, `extension!` — anything else)
//! counted over live code; `limit` is the pinned number.

use std::path::Path;

use super::{bound, finding, Violation};
use crate::schema::model::rules::Rules;
use crate::schema::model::scope::path_within;
use crate::search::digest::Digest;

const KINDS: &[&str] = &[
    "fn", "struct", "enum", "trait", "const", "static", "type", "mod", "impl", "async-fn",
];

pub fn check(
    root: &Path,
    rules: &Rules,
    env: Option<&str>,
    digest: Option<&Digest>,
    out: &mut Vec<Violation>,
) {
    for rule in bound(rules, env, "count-pin") {
        let (Some(what), Some(pinned)) = (rule.values.first(), rule.limit) else {
            out.push(finding(
                "count-pin",
                ".harna/rules.toml",
                0,
                format!(
                    "rule `{}` needs a kind or marker in `values` and the count in `limit`",
                    rule.id
                ),
                "harna add-rule --check count-pin --value <kind|marker> --limit <n> --scope <path>"
                    .into(),
            ));
            continue;
        };
        let in_scope =
            |p: &str| rule.scope.is_empty() || rule.scope.iter().any(|s| path_within(p, s));
        let found = if KINDS.contains(&what.as_str()) {
            let Some(d) = digest else { continue };
            d.modules
                .iter()
                .filter(|m| in_scope(&m.path))
                .flat_map(|m| &m.inventory)
                .filter(|i| &i.kind == what)
                .count()
        } else {
            crate::search::digest::respected_rust_files(root, crate::search::digest::MAX_FILES)
                .into_iter()
                .filter(|f| in_scope(f))
                .filter_map(|f| std::fs::read_to_string(root.join(f)).ok())
                .map(|src| {
                    crate::search::digest::live::live_code(&src)
                        .iter()
                        .filter(|(_, l)| l.contains(what.as_str()))
                        .count()
                })
                .sum()
        };
        if found != pinned as usize {
            let delta = found as i64 - pinned as i64;
            out.push(finding(
                "count-pin",
                rule.scope.first().map(String::as_str).unwrap_or("."),
                0,
                format!(
                    "`{what}` count is {found}, pinned at {pinned} ({delta:+}) (rule `{}`)",
                    rule.id
                ),
                "an addition or removal is a decision: update the pin (`limit`) with the change"
                    .into(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::rules::Rule;

    #[test]
    fn a_count_off_its_pin_is_named_with_the_delta() {
        let root = std::env::temp_dir().join(format!(
            "harna-pins-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("ext/fs")).unwrap();
        std::fs::write(
            root.join("ext/fs/ops.rs"),
            "#[op2]\nfn op_a() {}\n#[op2(fast)]\nfn op_b() {}\n#[op2]\nfn op_c() {}\n// #[op2] in a comment\n",
        )
        .unwrap();
        let mut rules = Rules::default();
        rules.rules.push(Rule {
            id: "op-count".into(),
            text: "t".into(),
            rationale: "r".into(),
            origin: "o".into(),
            scope: vec!["ext/fs".into()],
            check: Some("count-pin".into()),
            values: vec!["#[op2".into()],
            limit: Some(2),
            ..Default::default()
        });
        let mut out = Vec::new();
        check(&root, &rules, None, None, &mut out);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(
            out[0].message.contains("count is 3, pinned at 2 (+1)"),
            "{}",
            out[0].message
        );
        rules.rules[0].limit = Some(3);
        let mut out = Vec::new();
        check(&root, &rules, None, None, &mut out);
        assert!(out.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
