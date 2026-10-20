use super::*;
use crate::schema::model::rules::Rule;
use crate::search::digest::{Item, Module};

fn module(path: &str, sig: &str) -> Module {
    Module {
        path: path.into(),
        lang: "rust".into(),
        signatures: vec![sig.into()],
        deps: vec![],
        body_hash: String::new(),
        inventory: vec![Item {
            vis: "priv".into(),
            kind: "fn".into(),
            name: "helper".into(),
            text: "fn helper(x: u8)".into(),
        }],
        mods: vec![],
        crate_dir: None,
        src_root: None,
        reexports: vec![],
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
    assert!(expand(&mut rules, Some(&d)).is_empty());
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
    assert_eq!(expand(&mut bad, Some(&d)).len(), 1);
    assert_eq!(
        expand(&mut bad, None).len(),
        1,
        "no map is named, not ignored"
    );
}
