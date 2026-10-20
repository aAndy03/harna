use super::*;
use crate::schema::model::manifest::{Container, Layer};
use crate::search::digest::Module;

fn container(name: &str, path: &str, gateway: Option<&str>, connects: &[&str]) -> Container {
    Container {
        escapes: Vec::new(),
        owns: Vec::new(),
        name: name.into(),
        path: path.into(),
        description: String::new(),
        sequence: vec![],
        provides: vec![],
        role: None,
        gateway: gateway.map(|g| g.to_string()),
        connects: connects.iter().map(|s| s.to_string()).collect(),
        components: vec![],
    }
}

fn manifest(layers: Vec<(&str, Vec<Container>, Vec<&str>)>) -> Manifest {
    Manifest {
        test_kinds: Vec::new(),
        schema_version: 1,
        layers: layers
            .into_iter()
            .map(|(name, containers, depends_on)| Layer {
                isolated: false,
                name: name.into(),
                description: String::new(),
                sequence: vec![],
                depends_on: depends_on.into_iter().map(|s| s.to_string()).collect(),
                role: None,
                containers,
            })
            .collect(),
    }
}

fn module(path: &str, deps: &[&str]) -> Module {
    Module {
        reexports: Vec::new(),
        crate_dir: None,
        src_root: None,
        path: path.into(),
        lang: "rust".into(),
        signatures: vec![],
        deps: deps.iter().map(|s| s.to_string()).collect(),
        body_hash: "x".into(),
        inventory: vec![],
        mods: vec![],
    }
}

fn digest_of(modules: Vec<Module>) -> Digest {
    Digest {
        orphans: Vec::new(),
        generated_at: "now".into(),
        extractor_version: crate::search::digest::EXTRACTOR_VERSION,
        modules,
    }
}

fn cargo_fixtures() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "harna-gateway-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("crates/t/src")).unwrap();
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

fn run(manifest: &Manifest, root: &Path, files: &[(&str, &str)]) -> Vec<Violation> {
    for (rel, src) in files {
        let abs = root.join(rel);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(&abs, src).unwrap();
    }
    let srcs: Vec<(String, String)> = files
        .iter()
        .map(|(rel, src)| (rel.to_string(), src.to_string()))
        .collect();
    let v = Digest {
        orphans: Vec::new(),
        generated_at: "now".into(),
        extractor_version: crate::search::digest::EXTRACTOR_VERSION,
        modules: vec![module("crates/t/src/storage/gw.rs", &[])],
    };
    let mut out = Vec::new();
    check(root, manifest, Some(&v), &srcs, &mut out);
    check_declarations(manifest, &mut out);
    out
}

#[test]
fn write_gateway_gates_writers_not_readers() {
    let root = cargo_fixtures();
    let m = manifest(vec![(
        "data",
        vec![
            container("storage", "crates/t/src/storage", Some("fs:write"), &[]),
            container("app", "crates/t/src/app", None, &[]),
        ],
        vec![],
    )]);
    let out = run(
        &m,
        &root,
        &[
            (
                "crates/t/src/app/writer.rs",
                "fn go() { let s = fs::write(&path, b\"x\"); }",
            ),
            (
                "crates/t/src/app/reader.rs",
                "fn go() { fs::read_to_string(&path); }",
            ),
            (
                "crates/t/src/app/meta.rs",
                "fn go() { let m = std::fs::metadata(&path); }",
            ),
            (
                "crates/t/src/app/blanket.rs",
                "use std::fs;\nlet m = fs::read_dir(dir);",
            ),
            (
                "crates/t/src/storage/gw.rs",
                "use std::fs;\nstd::fs::create_dir_all(d);",
            ),
        ],
    );
    let hits: Vec<_> = out
        .iter()
        .filter(|v| v.check == "gateway-ownership")
        .collect();
    assert_eq!(hits.len(), 1, "only the writer is gated, was: {out:?}");
    assert_eq!(hits[0].file, "crates/t/src/app/writer.rs");
    assert_eq!(hits[0].line, 1);
    assert!(hits[0].message.contains("fs (write side)"));
    assert!(hits[0].message.contains("fs::write"));
    assert!(hits[0].message.contains("crates/t/src/storage"));
}

#[test]
fn full_fs_gateway_gates_reads_too_and_subtrees_are_exempt() {
    let root = cargo_fixtures();
    let m = manifest(vec![(
        "data",
        vec![
            container("storage", "crates/t/src/storage", Some("fs"), &[]),
            container("app", "crates/t/src/app", None, &[]),
        ],
        vec![],
    )]);
    let out = run(
        &m,
        &root,
        &[
            (
                "crates/t/src/app/reader.rs",
                "fn go() { fs::read_to_string(&path); }",
            ),
            (
                "crates/t/src/app/writer.rs",
                "fn go() { std::fs::write(&path, b\"x\"); }",
            ),
            ("crates/t/src/storage/local.rs", "fs::metadata(&path);"),
        ],
    );
    let hits: Vec<_> = out
        .iter()
        .filter(|v| v.check == "gateway-ownership")
        .collect();
    assert_eq!(
        hits.len(),
        2,
        "both touches outside the gateway, was: {out:?}"
    );
    assert!(hits.iter().all(|v| v
        .message
        .contains("outside its registered gateway (crates/t/src/storage (fs))")));
    assert!(
        hits.iter().all(|v| !v.file.contains("storage/local.rs")),
        "subtree exempt"
    );
}

#[test]
fn ungoverned_files_and_test_fixtures_stay_open() {
    let root = cargo_fixtures();
    let m = manifest(vec![(
        "data",
        vec![container(
            "storage",
            "crates/t/src/storage",
            Some("fs:write"),
            &[],
        )],
        vec![],
    )]);
    let out = run(
        &m,
        &root,
        &[
            // No container covers src/original: ungoverned carve-out.
            (
                "crates/t/src/original/rogue.rs",
                "fn go() { fs::write(&p, b\"x\"); }",
            ),
            // Declared but test-gated: fixtures write temp dirs freely.
            (
                "crates/t/src/app/t.rs",
                "#[cfg(test)]\nmod tests {\n  fn f() { std::fs::write(&t, b\"y\"); }\n}",
            ),
        ],
    );
    assert!(
        out.iter().all(|v| v.check != "gateway-ownership"),
        "was: {out:?}"
    );
}

#[test]
fn net_and_process_facilities_gate_by_declaration() {
    let root = cargo_fixtures();
    let m = manifest(vec![(
        "edges",
        vec![
            container("client", "crates/t/src/client", Some("net, process"), &[]),
            container("app", "crates/t/src/app", None, &[]),
        ],
        vec![],
    )]);
    let out = run(
        &m,
        &root,
        &[
            (
                "crates/t/src/app/net.rs",
                "use reqwest::Client;\nfn f(c: reqwest::Client) {}",
            ),
            ("crates/t/src/app/db.rs", "use sqlx::postgres;"),
            (
                "crates/t/src/app/run.rs",
                "fn f() { std::process::Command::new(\"cargo\"); }",
            ),
            ("crates/t/src/client/gw.rs", "use reqwest::Client;"),
            ("crates/t/src/app/io.rs", "use std::io::Write;"),
        ],
    );
    let hits: Vec<_> = out
        .iter()
        .filter(|v| v.check == "gateway-ownership")
        .collect();
    assert_eq!(
        hits.len(),
        3,
        "net client, db client and process spawn gate; io does not, was: {out:?}"
    );
    assert!(hits.iter().any(|v| v.file.ends_with("net.rs")));
    assert!(hits.iter().any(|v| v.file.ends_with("db.rs")));
    assert!(hits.iter().any(|v| v.file.ends_with("run.rs")));
}

#[test]
fn base_facility_narrows_the_reach_to_registered_gateways() {
    let root = cargo_fixtures();
    let m = manifest(vec![
        (
            "base",
            vec![container("storage", "crates/t/src/storage", None, &[])],
            vec![],
        ),
        (
            "app",
            vec![
                container("gw", "crates/t/src/repo", Some("base"), &[]),
                container("business", "crates/t/src/business", None, &[]),
            ],
            vec!["base"],
        ),
    ]);
    let d = digest_of(vec![
        module("crates/t/src/storage/files.rs", &[]),
        module("crates/t/src/repo/gwmain.rs", &["crate::storage::files"]),
        module("crates/t/src/business/math.rs", &["crate::storage::files"]),
    ]);
    let srcs: Vec<(String, String)> = vec![];
    let mut out = Vec::new();
    check(&root, &m, Some(&d), &srcs, &mut out);
    let hits: Vec<_> = out
        .iter()
        .filter(|v| v.check == "gateway-ownership")
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "the registered gateway mediates; business may not, was: {out:?}"
    );
    assert!(hits[0].file.ends_with("business/math.rs"));
    assert!(hits[0].message.contains("base layer `base`"));
}

#[test]
fn depends_on_does_not_exempt_base_reaches() {
    let root = cargo_fixtures();
    let mut m = manifest(vec![
        (
            "base",
            vec![container("storage", "crates/t/src/storage", None, &[])],
            vec![],
        ),
        (
            "app",
            vec![
                container("gw", "crates/t/src/repo", Some("base"), &[]),
                container("business", "crates/t/src/business", None, &[]),
            ],
            vec![],
        ),
    ]);
    // The layer declares its dependency: legal for layer-boundary, but
    // the gateway narrows the base reach to registered containers.
    m.layers[1].depends_on = vec!["base".into()];
    let d = digest_of(vec![
        module("crates/t/src/storage/files.rs", &[]),
        module("crates/t/src/business/math.rs", &["crate::storage::files"]),
    ]);
    let srcs: Vec<(String, String)> = vec![];
    let mut out = Vec::new();
    check(&root, &m, Some(&d), &srcs, &mut out);
    assert_eq!(
        out.iter()
            .filter(|v| v.check == "gateway-ownership")
            .count(),
        1,
        "declared reach is not a registered gateway, was: {out:?}"
    );
}

#[test]
fn malformed_gateway_declarations_advise_and_stay_out_of_enforcement() {
    let root = cargo_fixtures();
    let m = manifest(vec![(
        "data",
        vec![container(
            "storage",
            "crates/t/src/storage",
            Some("fs:writing, banana"),
            &[],
        )],
        vec![],
    )]);
    let out = run(
        &m,
        &root,
        &[(
            "crates/t/src/app/writer.rs",
            "fn go() { fs::write(&p, b\"x\"); }",
        )],
    );
    assert!(
        out.iter().all(|v| v.file == ".harna/manifest.toml"),
        "nothing valid -> nothing engages (advisories only), was: {out:?}"
    );
    let infos: Vec<_> = out
        .iter()
        .filter(|v| v.check == "gateway-ownership")
        .collect();
    assert_eq!(infos.len(), 2, "both malformed parts named, was: {out:?}");
    assert!(infos.iter().all(|v| v.severity == Severity::Info));

    // A partially valid declaration engages the valid half only.
    let m2 = manifest(vec![(
        "data",
        vec![
            container("storage", "crates/t/src/storage", Some("fs, banana"), &[]),
            container("app", "crates/t/src/app", None, &[]),
        ],
        vec![],
    )]);
    let out2 = run(
        &m2,
        &root,
        &[
            (
                "crates/t/src/app/writer.rs",
                "fn go() { fs::write(&p, b\"x\"); }",
            ),
            ("crates/t/src/app/net.rs", "use reqwest::Client;"),
        ],
    );
    let hits: Vec<_> = out2
        .iter()
        .filter(|v| v.check == "gateway-ownership" && v.file != ".harna/manifest.toml")
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "fs fire, net stays ungoverned, was: {out2:?}"
    );
    assert!(hits[0].file.ends_with("writer.rs"));
}

#[test]
fn terminal_flushes_are_not_storage_writes() {
    let root = cargo_fixtures();
    let m = manifest(vec![(
        "data",
        vec![
            container("storage", "crates/t/src/storage", Some("fs:write"), &[]),
            container("app", "crates/t/src/app", None, &[]),
        ],
        vec![],
    )]);
    let out = run(
        &m,
        &root,
        &[
            (
                "crates/t/src/app/term.rs",
                "use std::io::Write as _;\nlet _ = std::io::stdout().flush();",
            ),
            (
                "crates/t/src/app/w.rs",
                "fn f(p: &str) { let f = File::create(p); }",
            ),
        ],
    );
    let hits: Vec<_> = out
        .iter()
        .filter(|v| v.check == "gateway-ownership" && v.file != ".harna/manifest.toml")
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "the flush stays open; `File::create` scores, was: {out:?}"
    );
    assert!(hits[0].file.ends_with("w.rs"));
    assert!(hits[0].message.contains("File::create"));
}

/// font-manager (0.3.42): `write!(f, …)` in `impl Display` and an
/// `std::io::Error` variant are formatting/error plumbing — they blocked
/// validation as fs writes; a top-of-file `#[cfg(test)] use …;` hid a real
/// `std::fs::read` below it under a full `fs` gateway.
#[test]
fn display_impls_are_not_storage_and_gated_uses_do_not_hide_reads() {
    let root = cargo_fixtures();
    let m = manifest(vec![(
        "backend",
        vec![
            container("discovery", "crates/t/src/discovery", Some("fs"), &[]),
            container("metadata", "crates/t/src/metadata", None, &[]),
            container("app", "crates/t/src/app", None, &[]),
        ],
        vec![],
    )]);
    let out = run(
        &m,
        &root,
        &[
            (
                "crates/t/src/metadata/parser.rs",
                "use std::fmt;
pub enum ParseError { Io(std::io::Error), Bad(String) }
                 impl fmt::Display for ParseError {
                     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                         write!(f, \"unreadable font: {why}\")
    }
}
",
            ),
            (
                "crates/t/src/app/probe.rs",
                "#[cfg(test)]
use std::path::PathBuf;

                 pub fn probe(p: &Path) { let _ = std::fs::read(p); }
",
            ),
        ],
    );
    let hits: Vec<_> = out
        .iter()
        .filter(|v| v.check == "gateway-ownership")
        .collect();
    assert_eq!(hits.len(), 1, "only the probe's read, was: {out:?}");
    assert_eq!(hits[0].file, "crates/t/src/app/probe.rs");
    assert_eq!(hits[0].line, 4);
}

#[test]
fn duplicate_facility_gateways_are_named_once_and_both_exempt() {
    let root = cargo_fixtures();
    let m = manifest(vec![(
        "data",
        vec![
            container("a", "crates/t/src/storage", Some("fs:write"), &[]),
            container("b", "crates/t/src/render", Some("fs:write"), &[]),
        ],
        vec![],
    )]);
    let out = run(
        &m,
        &root,
        &[
            (
                "crates/t/src/storage/writer.rs",
                "fn go() { fs::write(&p, b\"x\"); }",
            ),
            ("crates/t/src/render/view.rs", "std::fs::create_dir_all(d);"),
        ],
    );
    assert!(
        out.iter()
            .all(|v| v.check != "gateway-ownership" || v.file == ".harna/manifest.toml"),
        "both registered containers are exempt on the file side, was: {out:?}"
    );
    let claimed = out
        .iter()
        .find(|v| v.message.contains("2 registered gateways"))
        .expect("the doubled ownership is named");
    assert_eq!(claimed.severity, Severity::Info);
    assert!(claimed.message.contains("fs` has 2"));
}

#[test]
fn tokens_do_not_score_longer_paths_or_cross_crate_names() {
    let root = cargo_fixtures();
    let m = manifest(vec![(
        "data",
        vec![
            container("storage", "crates/t/src/storage", Some("fs"), &[]),
            container("app", "crates/t/src/app", None, &[]),
        ],
        vec![],
    )]);
    let out = run(
        &m,
        &root,
        &[
            (
                "crates/t/src/app/z.rs",
                "let t = my_fs::read_to_string(p);\nlet u = self::fs::read_dir(d);",
            ),
            ("crates/t/src/app/o.rs", "fn f() { fs::read_to_string(p); }"),
        ],
    );
    let hits: Vec<_> = out
        .iter()
        .filter(|v| v.check == "gateway-ownership")
        .collect();
    assert_eq!(hits.len(), 1, "only the std fs touch, was: {out:?}");
    assert!(hits[0].file.ends_with("app/o.rs"));
}

#[test]
fn brace_form_is_a_read_side_limitation_pinned() {
    let root = cargo_fixtures();
    let writeo = manifest(vec![(
        "data",
        vec![
            container("storage", "crates/t/src/storage", Some("fs:write"), &[]),
            container("app", "crates/t/src/app", None, &[]),
        ],
        vec![],
    )]);
    let full = manifest(vec![(
        "data",
        vec![
            container("storage", "crates/t/src/storage", Some("fs"), &[]),
            container("app", "crates/t/src/app", None, &[]),
        ],
        vec![],
    )]);
    let files = &[(
        "crates/t/src/app/brace.rs",
        "use std::fs::{write, File};\nfn f() {}",
    )];
    assert!(
        run(&writeo, &root, files)
            .iter()
            .all(|v| v.check != "gateway-ownership"),
        "documented limitation: braces hide the write side under :write"
    );
    assert_eq!(
        run(&full, &root, files)
            .iter()
            .filter(|v| v.check == "gateway-ownership")
            .count(),
        1,
        "a full fs gateway still catches the blanket head"
    );
}
