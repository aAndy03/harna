use super::*;
use crate::schema::model::rules::Rule;
use crate::search::digest::{Item, Module};

fn module(path: &str, sig: &str) -> Module {
    Module {
        path: path.into(),
        lang: "rust".into(),
        signatures: vec![sig.into()],
        inventory: vec![Item {
            vis: "priv".into(),
            kind: "fn".into(),
            name: "helper".into(),
            text: "fn helper(x: u8)".into(),
        }],
        ..Default::default()
    }
}

/// R6 (0.4.8): "money is never a float" binds every module that handles
/// `Money`, wherever it lives — and nothing else; a path scope narrows it.
#[test]
fn a_rule_scoped_by_meaning_binds_the_files_that_declare_it() {
    let d = Digest {
        modules: vec![
            module(
                "crates/app/src/report.rs",
                "pub fn total(m: &Money) -> Money",
            ),
            module("crates/domain/src/money.rs", "pub struct Money"),
            module("crates/app/src/ui.rs", "pub fn draw()"),
        ],
        ..Default::default()
    };
    let mut rules = Rules::default();
    rules.rules.push(Rule {
        id: "money-exact".into(),
        scope_signature: Some(r"\bMoney\b".into()),
        ..Default::default()
    });
    rules.rules.push(Rule {
        id: "app-money".into(),
        scope: vec!["crates/app".into()],
        scope_signature: Some(r"\bMoney\b".into()),
        ..Default::default()
    });
    rules.rules.push(Rule {
        id: "nothing-matches".into(),
        scope_signature: Some(r"\bYen\b".into()),
        ..Default::default()
    });
    let found = expand(&mut rules, Some(&d), &[]);
    assert!(found.broken.is_empty() && found.degraded.is_empty());
    assert_eq!(
        rules.rules[0].scope,
        vec!["crates/app/src/report.rs", "crates/domain/src/money.rs"]
    );
    assert_eq!(rules.rules[1].scope, vec!["crates/app/src/report.rs"]);
    assert!(
        !crate::schema::model::scope::path_within("crates/app/src/ui.rs", &rules.rules[2].scope[0]),
        "no match applies nowhere, never everywhere"
    );
    let mut bad = Rules::default();
    bad.rules.push(Rule {
        id: "broken".into(),
        scope_signature: Some("([".into()),
        ..Default::default()
    });
    assert_eq!(expand(&mut bad, Some(&d), &[]).broken.len(), 1);
    let mut unmapped = Rules::default();
    unmapped.rules.push(Rule {
        id: "unmapped".into(),
        scope_signature: Some("Money".into()),
        ..Default::default()
    });
    assert_eq!(
        expand(&mut unmapped, None, &[]).degraded.len(),
        1,
        "no map is named, not ignored"
    );
}

/// 0.4.9 review #5/#6: a file the map has not seen (born this burst) is
/// matched on its live declarations — comments never count — and a
/// folder-level check paired with a meaning scope is refused, not inert.
#[test]
fn live_files_bind_and_folder_checks_are_refused() {
    let d = Digest::default();
    let mut rules = Rules::default();
    rules.rules.push(Rule {
        id: "money-exact".into(),
        scope_signature: Some(r"\bMoney\b".into()),
        ..Default::default()
    });
    rules.rules.push(Rule {
        id: "money-folders".into(),
        check: Some("folder-size".into()),
        scope_signature: Some(r"\bMoney\b".into()),
        ..Default::default()
    });
    let live = vec![
        (
            "src/new.rs".to_string(),
            "pub fn pay(amount: Money) {}\n".to_string(),
        ),
        (
            "src/talk.rs".to_string(),
            "// Money is exact\nfn talk() {}\n".to_string(),
        ),
    ];
    let found = expand(&mut rules, Some(&d), &live);
    assert_eq!(rules.rules[0].scope, vec!["src/new.rs"]);
    assert_eq!(
        found.broken.len(),
        1,
        "folder-size + meaning scope is refused"
    );
    assert!(problem(&rules.rules[1]).is_some());
}
