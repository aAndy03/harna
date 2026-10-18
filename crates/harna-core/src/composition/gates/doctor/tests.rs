use super::*;
use crate::schema::store::Store;

#[test]
fn fresh_repo_is_healthy_with_expected_warnings() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("harna-doctor-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();

    let r = build(&store, &tmp)?;
    let find = |name: &str| r.checks.iter().find(|c| c.check == name).unwrap();
    assert_eq!(find(".harna").grade, Grade::Ok);
    assert_eq!(find("schema").grade, Grade::Ok);
    assert_eq!(find("rules").grade, Grade::Ok);
    assert_eq!(find("digest").grade, Grade::Warn, "no scan yet");
    // No burst journal yet: optional, so Ok rather than a warning.
    assert_eq!(
        find("handoff").grade,
        Grade::Ok,
        "no handoff recorded is a legitimate state"
    );
    assert_eq!(r.fail_count(), 0, "a fresh init must not fail");

    // A corrupt handoff must fail loudly: it would strand the "where
    // was I?" answer silently at every session start.
    std::fs::write(
        crate::schema::journal::handoff::path(&store),
        "not = [valid toml {{{{",
    )
    .unwrap();
    let r2 = build(&store, &tmp)?;
    let handoff2 = r2.checks.iter().find(|c| c.check == "handoff").unwrap();
    assert_eq!(handoff2.grade, Grade::Fail);
    Ok(())
}

#[test]
fn corrupted_rules_fail_but_do_not_stop_other_checks() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("harna-doctor2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    std::fs::write(store.rules_path(), "not = [valid toml {{{{").unwrap();

    let r = build(&store, &tmp)?;
    assert_eq!(r.fail_count(), 1);
    assert_eq!(r.checks.iter().filter(|c| c.check == "rules").count(), 1);
    let rules_check = r.checks.iter().find(|c| c.check == "rules").unwrap();
    assert_eq!(rules_check.grade, Grade::Fail);
    // The doctor still reports the rest.
    assert!(r.checks.iter().any(|c| c.check == "schema"));
    Ok(())
}

#[test]
fn the_glossary_is_audited_when_recorded() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("harna-doctor6-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();

    // Absent: optional, so Ok rather than a warning.
    let r = build(&store, &tmp)?;
    assert_eq!(r.checks.iter().find(|c| c.check == "terms").unwrap().grade, Grade::Ok);

    // Corrupt: fails loudly — a silent glossary would teach the wrong
    // vocabulary at every session start.
    std::fs::write(store.terms_path(), "not = [valid toml {{{{").unwrap();
    let r2 = build(&store, &tmp)?;
    let terms2 = r2.checks.iter().find(|c| c.check == "terms").unwrap();
    assert_eq!(terms2.grade, Grade::Fail);
    assert!(terms2.action.contains("terms.toml"));

    // Coherent: named terms counted.
    let terms = crate::schema::registers::terms::Terms {
        schema_version: crate::SCHEMA_VERSION,
        terms: vec![crate::schema::registers::terms::Term {
            word: "engine".into(),
            aliases: vec!["service".into()],
            means: "a container that carries the engine role".into(),
        }],
    };
    store.save_terms(&terms).unwrap();
    let r3 = build(&store, &tmp)?;
    let terms3 = r3.checks.iter().find(|c| c.check == "terms").unwrap();
    assert_eq!(terms3.grade, Grade::Ok);
    assert!(terms3.message.contains("1 term(s)"));

    // Two spellings one meaning: warned, named by owners.
    let alerted = crate::schema::registers::terms::Terms {
        schema_version: crate::SCHEMA_VERSION,
        terms: vec![
            crate::schema::registers::terms::Term {
                word: "engine".into(),
                aliases: vec!["service".into()],
                means: "a container that carries the engine role".into(),
            },
            crate::schema::registers::terms::Term {
                word: "gateway".into(),
                aliases: vec!["service".into()],
                means: "a facility monopoly (fs | net | process | base)".into(),
            },
        ],
    };
    store.save_terms(&alerted).unwrap();
    let r4 = build(&store, &tmp)?;
    let terms4 = r4.checks.iter().find(|c| c.check == "terms").unwrap();
    assert_eq!(terms4.grade, Grade::Warn);
    assert!(terms4.message.contains("alias collision"));
    assert!(terms4.message.contains("engine"));
    assert!(terms4.message.contains("gateway"));
    Ok(())
}

#[test]
fn future_schema_is_a_fail_with_update_hint() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("harna-doctor3-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    std::fs::write(store.manifest_path(), "schema_version = 999\n").unwrap();

    let r = build(&store, &tmp)?;
    let schema = r.checks.iter().find(|c| c.check == "schema").unwrap();
    assert_eq!(schema.grade, Grade::Fail);
    assert!(schema.action.contains("update harna"));
    Ok(())
}

#[test]
fn drifting_bindings_warn_but_do_not_fail() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("harna-doctor4-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    std::fs::write(
        store.rules_path(),
        "schema_version = 1\n\n[[rules]]\nid = \"t\"\ntext = \"t\"\nrationale = \"r\"\norigin = \"test\"\ncheck = \"no-such-check\"\nseverity = \"warn\"\n",
    )
    .unwrap();

    let r = build(&store, &tmp)?;
    let binding = r.checks.iter().find(|c| c.check == "rule-bindings").unwrap();
    assert_eq!(binding.grade, Grade::Warn);
    assert!(binding.message.contains("2 binding problem(s)"));
    assert!(binding.action.contains("rules.toml"));
    assert_eq!(r.fail_count(), 0, "binding drift advises, it must not fail");
    Ok(())
}

#[test]
fn decline_and_deadend_registers_are_audited() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("harna-doctor7-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();

    // Absent: optional, Ok.
    let r = build(&store, &tmp)?;
    assert_eq!(r.checks.iter().find(|c| c.check == "non-goals").unwrap().grade, Grade::Ok);
    assert_eq!(r.checks.iter().find(|c| c.check == "lessons").unwrap().grade, Grade::Ok);

    // Corrupt: fails loudly — a silent decline register would
    // silently un-refuse refused work.
    std::fs::write(store.non_goals_path(), "not = [valid toml {{{{").unwrap();
    std::fs::write(store.lessons_path(), "not = [valid toml {{{{").unwrap();
    let r2 = build(&store, &tmp)?;
    assert_eq!(r2.checks.iter().find(|c| c.check == "non-goals").unwrap().grade, Grade::Fail);
    assert_eq!(r2.checks.iter().find(|c| c.check == "lessons").unwrap().grade, Grade::Fail);

    // A verdict-less lesson is WARNED, not dead-silent.
    let mut l = crate::schema::registers::lessons::Lessons::default();
    l.upsert(crate::schema::registers::lessons::Lesson {
        id: "thin".into(),
        claim: "tried once".into(),
        verdict: String::new(),
        evidence: vec![],
        recorded: "2026-10-17".into(),
    });
    store.save_lessons(&l).unwrap();
    std::fs::remove_file(store.non_goals_path()).unwrap();
    let r3 = build(&store, &tmp)?;
    let lessons3 = r3.checks.iter().find(|c| c.check == "lessons").unwrap();
    assert_eq!(lessons3.grade, Grade::Warn);
    assert!(lessons3.message.contains("1 lesson(s) carry no verdict"));
    Ok(())
}

#[test]
fn the_roadmap_is_audited_when_declared() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("harna-doctor8-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();

    // Not solved: optional, Ok.
    let r = build(&store, &tmp)?;
    assert_eq!(
        r.checks.iter().find(|c| c.check == "roadmap").unwrap().grade,
        Grade::Ok
    );

    // Declared + coherent: "my current situation" is named.
    let mut rdoc = crate::schema::registers::roadmap::Roadmap::default();
    rdoc.upsert(crate::schema::registers::roadmap::Milestone {
        id: "0.3.28".into(),
        title: "roadmap as data".into(),
        state: "current".into(),
        notes: String::new(),
    });
    store.save_roadmap(&rdoc).unwrap();
    let r2 = build(&store, &tmp)?;
    let road2 = r2.checks.iter().find(|c| c.check == "roadmap").unwrap();
    assert_eq!(road2.grade, Grade::Ok);
    assert!(road2.message.contains("now is `0.3.28`"), "{}", road2.message);

    // Incoherent declaration: warned, not dead-silent.
    rdoc.milestones.push(crate::schema::registers::roadmap::Milestone {
        id: "0.3.29".into(),
        title: "dup".into(),
        state: "current".into(),
        notes: String::new(),
    });
    store.save_roadmap(&rdoc).unwrap();
    let r3 = build(&store, &tmp)?;
    let road3 = r3.checks.iter().find(|c| c.check == "roadmap").unwrap();
    assert_eq!(road3.grade, Grade::Warn);
    assert!(road3.message.contains("2 milestones marked `current`"));
    Ok(())
}

#[test]
fn coherent_binding_is_ok() -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("harna-doctor5-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let store = Store::open(&tmp).unwrap();
    store.init().unwrap();
    std::fs::write(
        store.rules_path(),
        "schema_version = 1\n\n[[rules]]\nid = \"t\"\ntext = \"t\"\nrationale = \"r\"\norigin = \"test\"\ncheck = \"layer-boundary\"\nseverity = \"info\"\n",
    )
    .unwrap();

    let r = build(&store, &tmp)?;
    let binding = r.checks.iter().find(|c| c.check == "rule-bindings").unwrap();
    assert_eq!(binding.grade, Grade::Ok, "override included");
    Ok(())
}
