use super::*;
use crate::schema::model::rules::Rule;
use crate::schema::model::rules::Rules;

/// font-manager (0.3.42): the agent recorded `no-font-install` and a
/// failed `ttf-parser`-only lesson; a fresh session must meet both in
/// onboarding, before it picks a direction — not only via place/plan.
#[test]
fn onboarding_carries_refusals_and_dead_ends_bounded() {
    use crate::schema::registers::lessons::{Lesson, Lessons};
    use crate::schema::registers::non_goals::{NonGoal, NonGoals};
    let tmp = std::env::temp_dir().join(format!(
        "harna-onb-registers-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    assert!(!build(&store).unwrap().text.contains("refused ("));

    let mut g = NonGoals::default();
    g.upsert(NonGoal {
        id: "no-font-install".into(),
        refusal: "the app never installs, deletes or moves font files".into(),
        reason: "read-only manager; system fonts belong to Windows".into(),
        recorded: "2026-10-18".into(),
    });
    store.save_non_goals(&g).unwrap();
    let mut l = Lessons::default();
    for i in 0..10 {
        l.upsert(Lesson {
            id: format!("lesson-{i}"),
            claim: format!("attempt {i} {}", "x".repeat(200)),
            verdict: "did not work".into(),
            evidence: vec![],
            recorded: "2026-10-18".into(),
        });
    }
    store.save_lessons(&l).unwrap();

    let text = build_full(&store).unwrap().text;
    assert!(text.contains("refused (1 non-goal(s) in force"), "{text}");
    assert!(text.contains("[no-font-install] the app never installs"));
    assert!(text.contains("dead ends (10 lesson(s)"));
    assert!(text.contains("[lesson-7]") && !text.contains("[lesson-8]"));
    assert!(text.contains("… +2 more (read .harna/lessons.toml)"));
    assert!(
        text.lines().all(|line| line.chars().count() <= 160),
        "clipped"
    );
    // the summary (0.9.5): every refusal and lesson by id, no reasons,
    // and the way to the detail
    let summary = build(&store).unwrap().text;
    assert!(summary.contains("no-font-install") && summary.contains("lesson-9"));
    assert!(!summary.contains("did not work"), "{summary}");
    assert!(summary.contains("harna onboarding <rules|glossary|map|registers|all>"));
    assert!(summary.len() < text.len());
    assert!(section(&store, "registers").unwrap().contains("[lesson-9]"));
    assert!(section(&store, "nope").is_err());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn a_repo_without_a_glossary_still_meets_the_vocabulary() {
    let tmp = std::env::temp_dir().join(format!(
        "harna-onb-concepts-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    let text = build_full(&store).unwrap().text;
    assert!(
        text.contains("concepts: harna's vocabulary (built in"),
        "{text}"
    );
    for word in [
        "parity",
        "transiter",
        "non-goal",
        "lesson",
        "handoff",
        "acknowledged",
    ] {
        assert!(
            text.contains(&format!("  {word} = ")),
            "missing concept {word}"
        );
    }
    // The built-in map is valid data with one meaning per word.
    let c = concepts();
    assert!(c.terms.len() >= 19);
    assert!(
        c.alias_collisions().is_empty(),
        "{:?}",
        c.alias_collisions()
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn includes_rules_and_flags_empty_manifest() {
    let tmp = std::env::temp_dir().join(format!("harna-onb-{}", std::process::id()));
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();

    let mut rules = Rules::default();
    rules.rules.push(Rule {
        scope_signature: None,
        command: None,
        values: Vec::new(),
        id: "test-rule".into(),
        text: "Do the thing".into(),
        rationale: "Because".into(),
        scope: vec![],
        tags: vec![],
        origin: "test".into(),
        examples: vec![],
        env: None,
        check: None,
        severity: None,
        pattern: None,
        limit: None,
        status: None,
    });
    store.save_rules(&rules).unwrap();

    let onb = build(&store).unwrap();
    assert!(onb.manifest_empty);
    assert_eq!(onb.rule_count, 1);
    assert!(onb.text.contains("test-rule"));

    // The glossary rides onboarding when recorded: a fresh session
    // meets harna's vocabulary without a separate query.
    let terms = crate::schema::registers::terms::Terms {
        schema_version: crate::SCHEMA_BASELINE,
        terms: vec![crate::schema::registers::terms::Term {
            word: "engine".into(),
            aliases: vec!["service".into()],
            means: "a role-carrying container hanging off a layer".into(),
        }],
    };
    store.save_terms(&terms).unwrap();
    let onb2 = build_full(&store).unwrap();
    assert!(onb2.text.contains("glossary:"));
    assert!(onb2
        .text
        .contains("engine (also: service) = a role-carrying"));

    // A recorded handoff arrives through the same call — the resumed
    // session does not need a separate query.
    crate::schema::journal::handoff::save(
        &store,
        "pre-compaction burst",
        &["wire the demo".to_string()],
        0,
    )
    .unwrap();
    let onb = build(&store).unwrap();
    assert!(onb.text.contains("pre-compaction burst"));
    assert!(onb.text.contains("1. wire the demo"));
}

#[test]
fn the_map_overview_is_bounded_and_identical_to_the_whole_map_era() {
    // The bounded read (0.3.16): totals ride the whole stream, detail
    // caps at ten — and the rendered text equals the whole-map era's
    // `render_overview`, byte for byte.
    use crate::search::digest::{Digest, Module};

    let tmp = std::env::temp_dir().join(format!("harna-onb-map-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(tmp.join(".harna"));
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();

    let mk = |i: usize| Module {
        reexports: Vec::new(),
        crate_dir: None,
        src_root: None,
        path: format!("src/m{i}.rs"),
        lang: "rust".into(),
        signatures: vec![format!("pub fn f{i}()")],
        deps: vec![],
        body_hash: String::new(),
        inventory: vec![],
        mods: vec![],
    };
    let modules: Vec<Module> = (0..14).map(mk).collect();
    let d = Digest {
        orphans: Vec::new(),
        generated_at: "now".into(),
        extractor_version: crate::search::digest::EXTRACTOR_VERSION,
        modules,
    };
    store
        .save_digest(&store.load_manifest().unwrap_or_default(), &d)
        .unwrap();

    let onb = build_full(&store).unwrap();
    assert_eq!(onb.module_count, 14);
    assert!(onb.text.contains("codebase map: 14 modules, 14 signatures"));
    assert!(
        onb.text.contains("src/m0.rs"),
        "detail rides the leading modules"
    );
    assert!(
        !onb.text.contains("src/m10.rs"),
        "detail caps before the tail"
    );
    assert!(onb.text.contains("… +4 more modules"));

    // Byte-identical to the whole-map renderer — the delegation holds.
    let expected = crate::search::digest::render_overview_from(
        "now",
        14,
        14,
        &d.modules[..10.min(d.modules.len())],
        10,
    );
    assert!(
        onb.text.contains(expected.trim_end()),
        "overview section must equal the whole-map rendering"
    );
    let _ = std::fs::remove_dir_all(tmp.join(".harna"));
}
