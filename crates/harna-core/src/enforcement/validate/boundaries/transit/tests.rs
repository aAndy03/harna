use super::*;
use crate::schema::model::manifest::{ComponentKind, Container, Layer};
use crate::search::digest::{Item, Module};

fn container(name: &str, path: &str, connects: &[&str]) -> Container {
    Container {
        escapes: Vec::new(),
        owns: Vec::new(),
        name: name.into(),
        path: path.into(),
        description: String::new(),
        sequence: vec![],
        provides: vec![],
        role: None,
        gateway: None,
        connects: connects.iter().map(|s| s.to_string()).collect(),
        components: vec![],
    }
}

fn transit_manifest() -> Manifest {
    Manifest {
        schema_version: 1,
        layers: vec![
            Layer {
                name: "api".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                containers: vec![container("main", "crates/t/src/api", &[])],
                role: None,
            },
            Layer {
                name: "transit".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                role: None,
                containers: vec![container(
                    "api-billing",
                    "crates/t/src/corridoro",
                    &["api", "billing"],
                )],
            },
            Layer {
                name: "billing".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                containers: vec![container("engine", "crates/t/src/billing", &[])],
                role: None,
            },
        ],
    }
}

fn module(path: &str, deps: &[&str], sigs: usize, items: usize) -> Module {
    Module {
        reexports: Vec::new(),
        crate_dir: None,
        src_root: None,
        path: path.into(),
        lang: "rust".into(),
        signatures: (0..sigs).map(|i| format!("fn s{i}()")).collect(),
        deps: deps.iter().map(|s| s.to_string()).collect(),
        body_hash: "x".into(),
        inventory: (0..items)
            .map(|i| Item {
                vis: "pub".into(),
                kind: "struct".into(),
                name: format!("T{i}"),
                text: format!("pub struct T{i}"),
            })
            .collect(),
        mods: vec![],
    }
}

fn digest_of(modules: Vec<Module>) -> Digest {
    Digest {
        generated_at: "now".into(),
        extractor_version: crate::search::digest::EXTRACTOR_VERSION,
        modules,
    }
}

fn thin_rule(id: &str, limit: u32, env: Option<&str>, scope: &[&str]) -> Rule {
    Rule {
        values: Vec::new(),
        id: id.into(),
        text: format!("transiters stay under {limit}"),
        rationale: "thin corridors".into(),
        scope: scope.iter().map(|s| s.to_string()).collect(),
        tags: vec![],
        origin: "test".into(),
        examples: vec![],
        env: env.map(|s| s.to_string()),
        check: Some("transiter-thinness".into()),
        severity: None,
        pattern: None,
        limit: Some(limit),
        status: None,
    }
}

fn workspace() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "harna-transit-{}-{}",
        std::process::id(),
        std::thread::current()
            .name()
            .unwrap_or("t")
            .replace("::", "__")
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("crates/t")).unwrap();
    std::fs::write(
        root.join("crates/t/Cargo.toml"),
        "[package]\nname = \"t\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/t\"]\n",
    )
    .unwrap();
    root
}

#[test]
fn mediated_import_passes_direct_bypass_flags() {
    let root = workspace();
    let m = transit_manifest();
    let d = digest_of(vec![
        module(
            "crates/t/src/api/h.rs",
            &["crate::corridoro::relay", "crate::billing::charger"],
            1,
            0,
        ),
        module(
            "crates/t/src/corridoro/relay.rs",
            &["crate::billing::charger"],
            1,
            0,
        ),
        module("crates/t/src/billing/charger.rs", &[], 1, 0),
    ]);
    let mut out = Vec::new();
    check(&root, &m, &Rules::default(), None, Some(&d), &mut out);
    let hits: Vec<_> = out.iter().filter(|v| v.check == "no-sideways").collect();
    assert_eq!(hits.len(), 1, "only the direct edge bypasses, was: {out:?}");
    assert!(hits[0].file.ends_with("api/h.rs"));
    assert_eq!(hits[0].severity, Severity::Error);
    assert!(hits[0]
        .message
        .contains("skips the transiter api-billing (api -> billing)"));
    assert!(hits[0].message.contains("crate::billing::charger"));
}

#[test]
fn declared_depends_on_does_not_nullify_the_corridor() -> anyhow::Result<()> {
    let root = workspace();
    let mut m = transit_manifest();
    m.layers[0].depends_on = vec!["billing".into()]; // legal for layer-boundary
    let d = digest_of(vec![
        module("crates/t/src/api/h.rs", &["crate::billing::charger"], 0, 0),
        module("crates/t/src/billing/charger.rs", &[], 0, 0),
    ]);
    let mut out = Vec::new();
    check(&root, &m, &Rules::default(), None, Some(&d), &mut out);
    assert_eq!(
        out.iter().filter(|v| v.check == "no-sideways").count(),
        1,
        "the corridor is the stricter shape; a declared direct edge still bypasses, was: {out:?}"
    );
    Ok(())
}

#[test]
fn undeclared_corridors_gate_nothing() {
    let root = workspace();
    let mut m = transit_manifest();
    for l in &mut m.layers {
        for c in &mut l.containers {
            c.connects.clear();
        }
    }
    let d = digest_of(vec![
        module(
            "crates/t/src/api/h.rs",
            &["crate::corridoro::relay", "crate::billing::charger"],
            0,
            0,
        ),
        module(
            "crates/t/src/corridoro/relay.rs",
            &["crate::billing::charger"],
            0,
            0,
        ),
        module("crates/t/src/billing/charger.rs", &[], 0, 0),
    ]);
    let mut out = Vec::new();
    check(&root, &m, &Rules::default(), None, Some(&d), &mut out);
    assert!(out.is_empty(), "no data, no gate — was: {out:?}");
}

#[test]
fn cross_crate_edges_obey_the_corridor_too() {
    let root = workspace();
    std::fs::create_dir_all(root.join("crates/u")).unwrap();
    std::fs::write(
        root.join("crates/u/Cargo.toml"),
        "[package]\nname = \"u\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/t\", \"crates/u\"]\n",
    )
    .unwrap();
    let mut m = transit_manifest();
    m.layers[0]
        .containers
        .push(container("other", "crates/u/src/api", &[]));
    let d = digest_of(vec![
        // Top imports across into the other crate directly.
        module("crates/u/src/api/h.rs", &["t::billing::charger"], 0, 0),
        // Same import through the corridor: mediated.
        module("crates/u/src/api/gwuse.rs", &["t::corridoro::relay"], 0, 0),
        module("crates/t/src/billing/charger.rs", &[], 0, 0),
        module("crates/t/src/corridoro/relay.rs", &[], 0, 0),
    ]);
    let mut out = Vec::new();
    check(&root, &m, &Rules::default(), None, Some(&d), &mut out);
    let hits: Vec<_> = out.iter().filter(|v| v.check == "no-sideways").collect();
    assert_eq!(
        hits.len(),
        1,
        "cross-crate direct edge bypasses too, was: {out:?}"
    );
    assert!(hits[0].file.ends_with("crates/u/src/api/h.rs"));
}

#[test]
fn corridor_declarations_speak_their_coherence() {
    let mut m = transit_manifest();
    m.layers[1].containers[0].connects = vec!["api".into(), "banana".into(), "transit".into()];
    let mut out = Vec::new();
    check_declarations(&m, &mut out);
    let infos: Vec<_> = out
        .iter()
        .filter(|v| v.severity == Severity::Info)
        .collect();
    assert!(
        infos
            .iter()
            .any(|v| v.message.contains("unknown layer `banana`")),
        "was: {out:?}"
    );
    assert!(
        infos
            .iter()
            .any(|v| v.message.contains("its own layer `transit`")),
        "was: {out:?}"
    );
    // A single-layer corridor is narrated too.
    let mut m2 = transit_manifest();
    m2.layers[1].containers[0].connects = vec!["api".into()];
    let mut out2 = Vec::new();
    check_declarations(&m2, &mut out2);
    assert!(out2.iter().any(|v| v.message.contains("corridor of 1")));
}

#[test]
fn thinness_holds_declared_transit_modules_to_the_rule_budget() {
    let root = workspace();
    let mut m = transit_manifest();
    m.layers[1].containers[0].components.push(Component {
        name: "relay".into(),
        kind: ComponentKind::Primitive,
        path: "crates/t/src/corridoro/relay.rs".into(),
        summary: String::new(),
        tests: vec![],
        provides: vec![],
        children: vec![],
    });
    let d = digest_of(vec![
        module(
            "crates/t/src/corridoro/relay.rs",
            &["crate::billing::charger"],
            5,
            5,
        ),
        module("crates/t/src/billing/charger.rs", &[], 3, 3),
    ]);

    // No rule: silent (thinness is adopted as data).
    let mut out = Vec::new();
    check(&root, &m, &Rules::default(), None, Some(&d), &mut out);
    assert!(out.iter().all(|v| v.check != "transiter-thinness"));

    // Budget 7: the corridor module (10 items) crosses it once.
    let mut rules = Rules::default();
    rules.rules.push(thin_rule("thin7", 7, None, &[]));
    let mut out2 = Vec::new();
    check(&root, &m, &rules, None, Some(&d), &mut out2);
    let hits: Vec<_> = out2
        .iter()
        .filter(|v| v.check == "transiter-thinness")
        .collect();
    assert_eq!(hits.len(), 1, "was: {out2:?}");
    assert!(hits[0].file.ends_with("corridoro/relay.rs"));
    assert!(hits[0]
        .message
        .contains("holds 10 declared item(s) — budget 7"));
    assert_eq!(hits[0].severity, Severity::Info);
    assert!(
        !out2
            .iter()
            .any(|v| v.check == "transiter-thinness" && v.file.ends_with("charger.rs")),
        "the engines' own modules are not transiters"
    );

    // Budget 10: exactly at the line, no crossing.
    let mut rules10 = Rules::default();
    rules10.rules.push(thin_rule("thin10", 10, None, &[]));
    let mut out3 = Vec::new();
    check(&root, &m, &rules10, None, Some(&d), &mut out3);
    assert!(out3.iter().all(|v| v.check != "transiter-thinness"));

    // The strictest applicable limit wins; env-tagged rules stay out of
    // other runs; deprecated rules owe no budget; unhosted components
    // say nothing here (manifest-path owns that finding).
    let mut rules4 = Rules::default();
    let mut dead = thin_rule("dead", 100, None, &[]);
    dead.status = Some("deprecated".into());
    rules4.rules.push(dead);
    rules4.rules.push(thin_rule("loose", 15, Some("dev"), &[]));
    rules4.rules.push(thin_rule("tight", 4, None, &[]));
    let mut out4 = Vec::new();
    check(&root, &m, &rules4, None, Some(&d), &mut out4);
    assert!(
        out4.iter()
            .any(|v| v.check == "transiter-thinness" && v.message.contains("budget 4")),
        "only the in-scope, live, strictest rule gates, was: {out4:?}"
    );
}
