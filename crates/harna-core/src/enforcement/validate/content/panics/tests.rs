use super::*;
use crate::enforcement::violation::Severity;
use crate::schema::model::rules::Rule;

fn rules(scope: &[&str]) -> Rules {
    let mut r = Rules::default();
    r.rules.push(Rule {
        id: "backend-no-unwrap".into(),
        text: "No unwrap/expect/panic in backend non-test code".into(),
        rationale: "a bad font file must never crash the app".into(),
        scope: scope.iter().map(|s| s.to_string()).collect(),
        origin: "user via agent".into(),
        check: Some("no-panic".into()),
        ..Default::default()
    });
    r
}

fn run(rules: &Rules, file: &str, src: &str) -> Vec<Violation> {
    let mut out = Vec::new();
    check(rules, None, file, src, &mut out);
    out
}

/// font-manager (0.3.42): the agent's rule `backend-no-unwrap` was prose
/// only; a parser `unwrap` slipped in unnoticed. Bound, it now gates.
#[test]
fn a_bound_rule_gates_backend_panics_and_leaves_tests_alone() {
    let src = "pub fn parse(b: &[u8]) -> Face {\n    let n = read_u16(b).unwrap();\n    \
               let t = table(b).expect(\"head table\");\n    if n == 0 { todo!() }\n    \
               let msg = \"never .unwrap() here\";\n    // x.unwrap() in a comment\n    \
               n.unwrap_or(0);\n    my_panic!(\"custom\");\n    unreachable!()\n}\n\
               #[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { parse(&[]).unwrap(); }\n}\n";
    let out = run(
        &rules(&["src/backend"]),
        "src/backend/metadata/parser.rs",
        src,
    );
    let lines: Vec<usize> = out.iter().map(|v| v.line).collect();
    assert_eq!(lines, vec![2, 3, 4], "was: {out:?}");
    assert!(out[0].message.contains("`unwrap`"));
    assert!(out[0].message.contains("backend-no-unwrap"));
    assert!(out[2].message.contains("`todo!`"));
    assert!(out.iter().all(|v| v.severity == Severity::Error));
}

#[test]
fn unbound_or_out_of_scope_files_stay_open() {
    let src = "fn f() { x.unwrap(); }\n";
    assert!(run(&Rules::default(), "src/backend/a.rs", src).is_empty());
    assert!(run(&rules(&["src/backend"]), "src/ui/app/window.rs", src).is_empty());
    assert_eq!(
        run(&rules(&[]), "src/ui/app/window.rs", src).len(),
        1,
        "global scope"
    );
}
