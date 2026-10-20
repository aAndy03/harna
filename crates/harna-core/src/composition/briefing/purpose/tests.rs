use super::*;
use crate::schema::model::manifest::{Container, Layer};
use crate::schema::registers::terms::{Term, Terms};

fn manifest() -> Manifest {
    let c = |name: &str, desc: &str| Container {
        name: name.into(),
        description: desc.into(),
        ..Default::default()
    };
    Manifest {
        layers: vec![
            Layer {
                name: "store".into(),
                description: "SQLite behind the ports".into(),
                containers: vec![c("sqlite", "persistence of households and expenses")],
                ..Default::default()
            },
            Layer {
                name: "app".into(),
                containers: vec![c("reporting", "monthly summaries per category")],
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

/// household field test (0.4.4): "persist household members" never
/// reached the store layer — no code mentioned it yet. Declared purpose
/// does, through the stem "persis".
#[test]
fn the_store_is_found_by_its_purpose() {
    let idea = idea_stems(&["persist".into(), "members".into()], &Terms::default());
    let homes = homes(&manifest(), &idea);
    assert_eq!(homes[0].2, "sqlite", "{homes:?}");
    let block = homes_block(&manifest(), &idea);
    assert!(block.contains("persistence of households"), "{block}");
}

/// The project's glossary widens the idea: an alias reaches its word.
#[test]
fn glossary_aliases_reach_their_word() {
    let mut terms = Terms::default();
    terms.terms.push(Term {
        word: "reporting".into(),
        aliases: vec!["dashboard".into()],
        means: "summaries".into(),
    });
    let idea = idea_stems(&["dashboard".into()], &terms);
    assert_eq!(homes(&manifest(), &idea)[0].2, "reporting");
    assert!(homes(
        &manifest(),
        &idea_stems(&["dashboard".into()], &Terms::default())
    )
    .is_empty());
}

/// 0.4.9 review #12: the pairs the doc promises meet, and short plurals too.
#[test]
fn stems_meet_the_documented_pairs() {
    for (a, b) in [
        ("persist", "persistence"),
        ("store", "storage"),
        ("report", "reporting"),
        ("user", "users"),
        ("file", "files"),
        ("cache", "caches"),
        ("process", "processing"),
    ] {
        assert_eq!(super::stem(a), super::stem(b), "{a} / {b}");
    }
    assert_ne!(super::stem("user"), super::stem("store"));
}
