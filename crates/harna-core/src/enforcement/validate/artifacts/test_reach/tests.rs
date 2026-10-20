use super::*;
use crate::schema::model::manifest::{Container, Layer};

fn workspace(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "harna-test-reach-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let w = |rel: &str, body: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    w("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
    w("crates/store/Cargo.toml", "[package]\nname = \"store\"\n");
    w("crates/store/src/lib.rs", "pub fn open() {}\n");
    w(
        "crates/api/Cargo.toml",
        "[package]\nname = \"api\"\n[dev-dependencies]\nstore = { path = \"../store\" }\n",
    );
    w("crates/api/src/lib.rs", "pub fn serve() {}\n");
    w(
        "crates/api/tests/end_to_end.rs",
        "use store::open;\n#[test]\nfn e2e() { open(); }\n",
    );
    root
}

fn manifest() -> Manifest {
    let layer = |name: &str, path: &str| Layer {
        name: name.into(),
        containers: vec![Container {
            name: name.into(),
            path: path.into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    Manifest {
        layers: vec![
            layer("api", "crates/api/src"),
            layer("store", "crates/store/src"),
        ],
        ..Default::default()
    }
}

/// household field test (0.4.7): `api` dev-depending on `store` passed
/// unexamined. Declared as a test kind's reach it is a choice; undeclared
/// it is named — the test file's import and the Cargo line both.
#[test]
fn a_test_kind_declares_what_tests_may_cross() {
    let root = workspace("e2e");
    let mut m = manifest();
    let mut out = Vec::new();
    check(&root, &m, &mut out);
    assert!(out.is_empty(), "no kinds declared: nothing runs");

    m.test_kinds.push(TestKind {
        name: "integration".into(),
        paths: vec!["crates/*/tests/**".into()],
        reach: vec![],
        description: String::new(),
    });
    let mut out = Vec::new();
    check(&root, &m, &mut out);
    let files: Vec<&str> = out.iter().map(|v| v.file.as_str()).collect();
    assert!(files.contains(&"crates/api/Cargo.toml"), "{out:?}");
    // 0.4.9 review #1: the test file itself — outside `src`, homed to its crate
    assert!(files.contains(&"crates/api/tests/end_to_end.rs"), "{out:?}");
    assert!(out.iter().all(|v| v.check == "test-reach"), "{out:?}");

    m.test_kinds[0].reach = vec!["store".into()];
    let mut out = Vec::new();
    check(&root, &m, &mut out);
    assert!(out.is_empty(), "declared crossing: {out:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// 0.4.9 review #2: a root package's dev-dependencies are covered by a
/// kind over `tests/**` (no `./` mismatch), and a file two kinds cover
/// may reach the union of their reaches.
#[test]
fn root_packages_and_overlapping_kinds_are_held_once() {
    let root = std::env::temp_dir().join(format!("harna-test-reach-root-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let w = |rel: &str, body: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    w(
        "Cargo.toml",
        "[package]\nname = \"app\"\n[workspace]\nmembers = [\"store\"]\n[dev-dependencies]\nstore = { path = \"store\" }\n",
    );
    w("src/lib.rs", "pub fn run() {}\n");
    w("store/Cargo.toml", "[package]\nname = \"store\"\n");
    w("store/src/lib.rs", "pub fn open() {}\n");
    w(
        "tests/it.rs",
        "use store::open;\n#[test]\nfn it() { open(); }\n",
    );
    let layer = |name: &str, path: &str| Layer {
        name: name.into(),
        containers: vec![Container {
            name: name.into(),
            path: path.into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut m = Manifest {
        layers: vec![layer("app", "src"), layer("store", "store/src")],
        ..Default::default()
    };
    m.test_kinds.push(TestKind {
        name: "it".into(),
        paths: vec!["tests/**".into()],
        reach: vec![],
        description: String::new(),
    });
    let mut out = Vec::new();
    check(&root, &m, &mut out);
    let files: Vec<&str> = out.iter().map(|v| v.file.as_str()).collect();
    assert!(
        files.contains(&"tests/it.rs") && files.contains(&"Cargo.toml"),
        "{out:?}"
    );
    m.test_kinds.push(TestKind {
        name: "seeded".into(),
        paths: vec!["**/it.rs".into()],
        reach: vec!["store".into()],
        description: String::new(),
    });
    let mut out = Vec::new();
    check(&root, &m, &mut out);
    assert!(out.is_empty(), "union of reaches: {out:?}");
    let _ = std::fs::remove_dir_all(&root);
}
