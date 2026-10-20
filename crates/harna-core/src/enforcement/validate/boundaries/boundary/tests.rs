use super::*;
use crate::schema::model::manifest::{Container, Layer, Manifest};

fn fixture(tag: &str) -> (std::path::PathBuf, Manifest) {
    let root = std::env::temp_dir().join(format!("harna-boundary-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (dir, pkg) in [(".", "t"), ("crates/t", "t")] {
        let cargo = root.join(dir).join("Cargo.toml");
        std::fs::create_dir_all(cargo.parent().unwrap()).unwrap();
        std::fs::write(
            &cargo,
            format!("[package]\nname = \"{pkg}\"\nversion = \"0.1.0\"\n"),
        )
        .unwrap();
    }
    let manifest = Manifest {
        test_kinds: Vec::new(),
        schema_version: 1,
        layers: vec![
            Layer {
                isolated: false,
                name: "api".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                role: None,
                containers: vec![Container {
                    escapes: Vec::new(),
                    owns: Vec::new(),
                    name: "handlers".into(),
                    path: "crates/t/src/api".into(),
                    description: String::new(),
                    sequence: vec![],
                    provides: vec![],
                    role: None,
                    gateway: None,
                    connects: vec![],
                    components: vec![],
                }],
            },
            Layer {
                isolated: false,
                name: "engine".into(),
                description: String::new(),
                sequence: vec![],
                depends_on: vec![],
                role: None,
                containers: vec![Container {
                    escapes: Vec::new(),
                    owns: Vec::new(),
                    name: "core".into(),
                    path: "crates/t/src/engine".into(),
                    description: String::new(),
                    sequence: vec![],
                    provides: vec![],
                    role: None,
                    gateway: None,
                    connects: vec![],
                    components: vec![],
                }],
            },
        ],
    };
    (root, manifest)
}

#[test]
fn flags_undeclared_cross_layer_import() {
    let (root, mut manifest) = fixture("flags");
    std::fs::create_dir_all(root.join("crates/t/src/api")).unwrap();
    std::fs::create_dir_all(root.join("crates/t/src/engine")).unwrap();
    std::fs::write(
        root.join("crates/t/src/api/h.rs"),
        "use crate::engine::billing::charge;\nfn main() {}",
    )
    .unwrap();
    let srcs = vec![(
        "crates/t/src/api/h.rs".to_string(),
        std::fs::read_to_string(root.join("crates/t/src/api/h.rs")).unwrap(),
    )];

    let mut v = Vec::new();
    check(&root, &manifest, &srcs, &mut v);
    assert_eq!(v.len(), 1, "undeclared cross-layer import is an error");
    assert_eq!(v[0].check, "layer-boundary");
    assert_eq!(v[0].line, 1);
    assert!(v[0].message.contains("engine"));

    // Declaring the dependency makes the same import legal.
    manifest.layers[0].depends_on = vec!["engine".into()];
    let mut v2 = Vec::new();
    check(&root, &manifest, &srcs, &mut v2);
    assert!(v2.is_empty());
}

#[test]
fn allows_same_layer_and_ungoverned_stems() {
    let (root, manifest) = fixture("allowed");
    std::fs::create_dir_all(root.join("crates/t/src/api")).unwrap();
    std::fs::create_dir_all(root.join("crates/t/src/engine")).unwrap();

    // Same-container import + import of a crate-root symbol no container
    // governs (stem `t` -> ungoverned -> skipped).
    std::fs::write(
        root.join("crates/t/src/api/h.rs"),
        "use crate::api::helpers;\nuse crate::T;\nfn main() {}",
    )
    .unwrap();
    let srcs = vec![(
        "crates/t/src/api/h.rs".to_string(),
        std::fs::read_to_string(root.join("crates/t/src/api/h.rs")).unwrap(),
    )];

    let mut v = Vec::new();
    check(&root, &manifest, &srcs, &mut v);
    assert!(v.is_empty(), "same-layer and ungoverned imports allowed");
}

fn module(path: &str, items: &[(&str, &str, &str, &str)]) -> crate::search::digest::Module {
    crate::search::digest::Module {
        reexports: Vec::new(),
        crate_dir: None,
        src_root: None,
        path: path.into(),
        lang: "rust".into(),
        signatures: vec![],
        deps: vec![],
        body_hash: "x".into(),
        inventory: items
            .iter()
            .map(|(vis, kind, name, text)| crate::search::digest::Item {
                vis: vis.to_string(),
                kind: kind.to_string(),
                name: name.to_string(),
                text: text.to_string(),
            })
            .collect(),
        mods: vec![],
    }
}

#[test]
fn declared_surface_gates_cross_container_symbols_and_priv() -> anyhow::Result<()> {
    let (root, mut manifest) = fixture("surface");
    manifest.layers[0].depends_on = vec!["engine".into()]; // layer gate silent
                                                           // The engine container declares its export surface.
    manifest.layers[1].containers[0].provides = vec!["Billing".into(), "Facade".into()];
    std::fs::create_dir_all(root.join("crates/t/src/api")).unwrap();
    std::fs::create_dir_all(root.join("crates/t/src/engine")).unwrap();
    std::fs::write(
        root.join("crates/t/src/api/h.rs"),
        // Symbol detail survives in flat use-paths and inline references;
        // brace-group imports collapse to their head module (documented
        // limitation — keep flat paths to get surface-checked).
        "use crate::engine::billing::Billing;\nfn go(h: crate::engine::billing::Secret) {}",
    )
    .unwrap();
    let srcs = vec![(
        "crates/t/src/api/h.rs".to_string(),
        std::fs::read_to_string(root.join("crates/t/src/api/h.rs")).unwrap(),
    )];

    let digest = crate::search::digest::Digest {
        orphans: Vec::new(),
        generated_at: "now".into(),
        extractor_version: crate::search::digest::EXTRACTOR_VERSION,
        modules: vec![module(
            "crates/t/src/engine/billing.rs",
            &[
                ("pub", "struct", "Billing", "pub struct Billing"),
                ("priv", "fn", "Secret", "fn Secret() {}"),
            ],
        )],
    };

    let mut v = Vec::new();
    check_declared_surface(&root, &manifest, Some(&digest), &srcs, &mut v);
    // `Billing` is declared (quiet); `Secret` is both undeclared AND
    // private — two findings, each naming its reason.
    assert_eq!(v.len(), 2);
    assert!(v.iter().all(|x| x.check == "declared-surface"));
    assert!(v.iter().any(|x| x.message.contains("not in provides")));
    assert!(v
        .iter()
        .any(|x| x.message.contains("private symbol `Secret`")));

    // Declaring the symbol quiets the declared side; the private side
    // is not negotiable.
    manifest.layers[1].containers[0]
        .provides
        .push("Secret".into());
    let mut v2 = Vec::new();
    check_declared_surface(&root, &manifest, Some(&digest), &srcs, &mut v2);
    assert_eq!(v2.len(), 1);
    assert!(v2[0].message.contains("private symbol"));
    Ok(())
}

#[test]
fn undeclared_containers_are_not_gated_and_module_imports_are_never_symbols() -> anyhow::Result<()>
{
    let (root, mut manifest) = fixture("nosurface");
    manifest.layers[0].depends_on = vec!["engine".into()];
    std::fs::create_dir_all(root.join("crates/t/src/api")).unwrap();
    std::fs::create_dir_all(root.join("crates/t/src/engine")).unwrap();
    // Whole-module import + a symbol import: no provides anywhere.
    std::fs::write(
        root.join("crates/t/src/api/h.rs"),
        "use crate::engine::billing;\npub fn go(s: crate::engine::billing::Handle) {}",
    )
    .unwrap();
    let srcs = vec![(
        "crates/t/src/api/h.rs".to_string(),
        std::fs::read_to_string(root.join("crates/t/src/api/h.rs")).unwrap(),
    )];
    let digest = crate::search::digest::Digest {
        orphans: Vec::new(),
        generated_at: "now".into(),
        extractor_version: crate::search::digest::EXTRACTOR_VERSION,
        modules: vec![module(
            "crates/t/src/engine/billing.rs",
            &[
                ("pub", "struct", "Handle", "pub struct Handle"),
                ("priv", "fn", "internal", "fn internal() {}"),
            ],
        )],
    };

    let mut v = Vec::new();
    check_declared_surface(&root, &manifest, Some(&digest), &srcs, &mut v);
    assert!(
        v.is_empty(),
        "no declared surface -> no gate yet; module stems are not symbols"
    );

    // Declaring a surface flips the gate: `Handle` declared (quiet),
    // `billing` whole-module still exempt.
    manifest.layers[1].containers[0].provides = vec!["Handle".into()];
    let mut v2 = Vec::new();
    check_declared_surface(&root, &manifest, Some(&digest), &srcs, &mut v2);
    assert!(v2.is_empty());
    Ok(())
}

/// deno study A8 (0.3.45): `deno_core::anyhow::Context` is anyhow's
/// trait re-exported through deno_core — the path runs through no module
/// of the target, so it cannot name the target's private `Context`.
#[test]
fn a_path_through_a_re_export_is_not_a_private_symbol() -> anyhow::Result<()> {
    let (root, mut manifest) = fixture("reexport");
    manifest.layers[0].depends_on = vec!["engine".into()];
    std::fs::create_dir_all(root.join("crates/t/src/api"))?;
    std::fs::write(
        root.join("crates/t/src/api/h.rs"),
        "use crate::engine::anyhow::Context;\nfn go(c: crate::engine::billing::Context) {}",
    )?;
    let srcs = vec![(
        "crates/t/src/api/h.rs".to_string(),
        std::fs::read_to_string(root.join("crates/t/src/api/h.rs"))?,
    )];
    let digest = crate::search::digest::Digest {
        orphans: Vec::new(),
        generated_at: "now".into(),
        extractor_version: crate::search::digest::EXTRACTOR_VERSION,
        modules: vec![module(
            "crates/t/src/engine/billing.rs",
            &[("priv", "struct", "Context", "struct Context")],
        )],
    };
    let mut v = Vec::new();
    check_declared_surface(&root, &manifest, Some(&digest), &srcs, &mut v);
    assert_eq!(v.len(), 1, "only the real module path is judged: {v:?}");
    assert!(v[0].message.contains("engine::billing::Context"));
    Ok(())
}

/// deno (0.3.45): `deno_runtime::exit` is `pub use deno_os::exit` at the
/// crate root; a private `exit` elsewhere in runtime is a namesake, not
/// the target — public anywhere in the container means not private.
#[test]
fn a_public_or_re_exported_name_shadows_a_private_namesake() -> anyhow::Result<()> {
    let (root, mut manifest) = fixture("shadow");
    manifest.layers[0].depends_on = vec!["engine".into()];
    std::fs::create_dir_all(root.join("crates/t/src/api"))?;
    std::fs::write(
        root.join("crates/t/src/api/h.rs"),
        "fn go() { crate::engine::billing::exit(1); }",
    )?;
    let srcs = vec![(
        "crates/t/src/api/h.rs".to_string(),
        std::fs::read_to_string(root.join("crates/t/src/api/h.rs"))?,
    )];
    let mut billing = module(
        "crates/t/src/engine/billing.rs",
        &[("priv", "fn", "exit", "fn exit()")],
    );
    let digest = |m: crate::search::digest::Module| crate::search::digest::Digest {
        orphans: Vec::new(),
        generated_at: "now".into(),
        extractor_version: crate::search::digest::EXTRACTOR_VERSION,
        modules: vec![m],
    };
    let mut v = Vec::new();
    check_declared_surface(
        &root,
        &manifest,
        Some(&digest(billing.clone())),
        &srcs,
        &mut v,
    );
    assert_eq!(v.len(), 1, "private and nothing public: named");
    billing.reexports = vec!["exit".into()];
    let mut v = Vec::new();
    check_declared_surface(&root, &manifest, Some(&digest(billing)), &srcs, &mut v);
    assert!(v.is_empty(), "re-exported publicly: {v:?}");
    Ok(())
}

/// 0.4.2 (gpui-kit's guide: a feature "must not reach into a sibling
/// feature's internals"): in an isolated layer, peer containers never
/// import each other; the same import is legal in an ordinary layer.
#[test]
fn isolated_layers_keep_their_containers_apart() {
    let (root, mut manifest) = fixture("isolated");
    manifest.layers[0].containers.push(Container {
        name: "search".into(),
        path: "crates/t/src/search".into(),
        ..Default::default()
    });
    std::fs::create_dir_all(root.join("crates/t/src/api")).unwrap();
    std::fs::create_dir_all(root.join("crates/t/src/search")).unwrap();
    let src = "use crate::search::filters::Filter;\nfn main() {}";
    let srcs = vec![("crates/t/src/api/h.rs".to_string(), src.to_string())];
    let mut v = Vec::new();
    check(&root, &manifest, &srcs, &mut v);
    assert!(v.is_empty(), "same layer, not isolated: {v:?}");
    manifest.layers[0].isolated = true;
    let mut v = Vec::new();
    check(&root, &manifest, &srcs, &mut v);
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].check, "sibling-isolation");
    assert!(
        v[0].message.contains("crates/t/src/search"),
        "{}",
        v[0].message
    );
}
