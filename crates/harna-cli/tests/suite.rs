//! The field-test suite, mechanically (0.9.2): the three project types
//! fresh agents test before every release, replayed through the real
//! binary on generated projects. Each story is the break list the field
//! tests accumulated — what harna must catch — so a release that loses a
//! catch fails here before an agent ever sees it.
//! The agent briefs: research/field-tests/suite/.

use std::path::{Path, PathBuf};
use std::process::Command;

fn harna(root: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_harna"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("run harna");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn ok(root: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_harna"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("run harna");
    assert!(
        out.status.success(),
        "harna {args:?} failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("harna-suite-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .unwrap()
        .success());
    ok(&root, &["init"]);
    root
}

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn commit(root: &Path) {
    for args in [
        &["add", "-A"][..],
        &[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "base",
        ][..],
    ] {
        assert!(Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .unwrap()
            .success());
    }
}

fn validate(root: &Path) -> String {
    ok(root, &["scan"]);
    harna(root, &["validate"])
}

/// Project type 1: a Rust workspace built by layers.
#[test]
fn rust_workspace_story() {
    let root = project("rust");
    write(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n",
    );
    for c in ["core", "app", "worker"] {
        write(
            &root,
            &format!("crates/{c}/Cargo.toml"),
            &format!("[package]\nname = \"{c}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        );
    }
    let core = "pub fn total(a: u64) -> u64 { a }\npub fn rate() -> u64 { 1 }\n";
    write(&root, "crates/core/src/lib.rs", core);
    write(
        &root,
        "crates/app/src/lib.rs",
        "pub fn view() -> u64 { core::total(1) }\n",
    );
    write(&root, "crates/worker/src/lib.rs", "pub fn run() {}\n");
    ok(
        &root,
        &[
            "container",
            "add",
            "core",
            "--layer",
            "domain",
            "--path",
            "crates/core/src",
        ],
    );
    ok(
        &root,
        &[
            "container",
            "add",
            "app",
            "--layer",
            "ui",
            "--path",
            "crates/app/src",
        ],
    );
    ok(
        &root,
        &[
            "container",
            "add",
            "worker",
            "--layer",
            "ui",
            "--path",
            "crates/worker/src",
        ],
    );
    ok(&root, &["layer", "set", "ui", "--depends-on", "domain"]);
    ok(
        &root,
        &["container", "set", "core", "--provides", "total,rate"],
    );
    assert!(
        validate(&root).contains("verdict: PASS"),
        "a clean start passes"
    );

    // the edit loop: a committed baseline, one upward import, judged by
    // --changed with no scan (0.9.1: scoped to what the change touches)
    commit(&root);
    write(
        &root,
        "crates/core/src/lib.rs",
        "pub fn total(a: u64) -> u64 { a }
pub fn rate() -> u64 { app::view() }
",
    );
    let v = harna(&root, &["validate", "--changed"]);
    assert!(v.contains("[!layer-boundary]"), "{v}");
    write(&root, "crates/core/src/lib.rs", core);

    // an upward import: the layer boundary holds
    write(
        &root,
        "crates/core/src/lib.rs",
        "pub fn total(a: u64) -> u64 { a }\npub fn rate() -> u64 { app::view() }\n",
    );
    let v = validate(&root);
    assert!(
        v.contains("[!layer-boundary]") && v.contains("[!no-cycles]"),
        "{v}"
    );
    write(&root, "crates/core/src/lib.rs", core);

    // a frozen contract: a re-signed name is a break
    ok(&root, &["scan"]);
    ok(&root, &["contract", "freeze", "core", "--version", "1.0.0"]);
    write(
        &root,
        "crates/core/src/lib.rs",
        "pub fn total(a: u64, b: u64) -> u64 { a + b }\npub fn rate() -> u64 { 1 }\n",
    );
    let v = validate(&root);
    assert!(
        v.contains("[!contract]") && v.contains("`total` changed"),
        "{v}"
    );
    write(&root, "crates/core/src/lib.rs", core);

    // services: one service importing another's code
    ok(&root, &["service", "add", "web", "--containers", "app"]);
    ok(&root, &["service", "add", "jobs", "--containers", "worker"]);
    write(
        &root,
        "crates/worker/src/lib.rs",
        "pub fn run() { app::view(); }\n",
    );
    let v = validate(&root);
    assert!(v.contains("[!service-talk]"), "{v}");
    // the edit is judged live by --changed, no scan needed
    write(&root, "crates/worker/src/lib.rs", "pub fn run() {}\n");
    assert!(ok(&root, &["migrate", "--check"]).contains("every .harna/ file is current"));
    let _ = std::fs::remove_dir_all(&root);
}

/// Project type 2: a TypeScript app, with services and a mirrored shape.
#[test]
fn typescript_app_story() {
    let root = project("ts");
    write(
        &root,
        "package.json",
        "{\"name\":\"web\",\"version\":\"1.0.0\"}",
    );
    let money = "export interface Posted { id: number; amount: number }\nexport const total = (n: number) => n;\n";
    write(&root, "web/src/domain/money.ts", money);
    write(
        &root,
        "web/src/ui/view.ts",
        "import { total } from '../domain/money';\nexport const view = () => total(1);\n",
    );
    ok(
        &root,
        &[
            "container",
            "add",
            "domain",
            "--layer",
            "core",
            "--path",
            "web/src/domain",
        ],
    );
    ok(
        &root,
        &[
            "container",
            "add",
            "ui",
            "--layer",
            "ui",
            "--path",
            "web/src/ui",
        ],
    );
    ok(&root, &["layer", "set", "ui", "--depends-on", "core"]);
    assert!(
        validate(&root).contains("verdict: PASS"),
        "a clean start passes"
    );

    write(
        &root,
        "web/src/domain/money.ts",
        &format!("import {{ view }} from '../ui/view';\n{money}"),
    );
    let v = validate(&root);
    assert!(
        v.contains("[!layer-boundary]") && v.contains("web/src/ui/view.ts"),
        "{v}"
    );
    write(&root, "web/src/domain/money.ts", money);

    // a TS mirror of the corridor's shape drifts
    write(
        &root,
        "web/src/ui/mirror.ts",
        "export interface Posted { id: number; note: string }\n",
    );
    ok(&root, &["service", "add", "api", "--containers", "domain"]);
    ok(&root, &["service", "add", "front", "--containers", "ui"]);
    ok(
        &root,
        &[
            "service",
            "corridor",
            "add",
            "postings",
            "--kind",
            "event",
            "--from",
            "api",
            "--to",
            "front",
            "--message",
            "web/src/domain/money.ts::Posted",
        ],
    );
    let v = validate(&root);
    assert!(
        v.contains("[!corridor-shape]") && v.contains("missing [amount], extra [note]"),
        "{v}"
    );
    assert!(v.contains("[!service-talk]"), "{v}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Project type 3: an existing repository adopted as it is.
#[test]
fn adopt_existing_story() {
    let root = project("adopt");
    write(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n",
    );
    for (c, body) in [
        ("store", "pub fn save() {}\n"),
        ("ledger", "pub fn post() { store::save(); }\n"),
        ("cli", "pub fn main() { ledger::post(); }\n"),
    ] {
        write(
            &root,
            &format!("crates/{c}/Cargo.toml"),
            &format!("[package]\nname = \"{c}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        );
        write(&root, &format!("crates/{c}/src/lib.rs"), body);
    }
    write(&root, "crates/ledger/Cargo.toml", "[package]\nname = \"ledger\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nstore = { path = \"../store\" }\n");
    write(&root, "crates/cli/Cargo.toml", "[package]\nname = \"cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nledger = { path = \"../ledger\" }\n");
    // adopt prints its draft; the adopter keeps it as the manifest
    let draft = ok(&root, &["adopt"]);
    assert!(draft.contains("[[layers]]"), "{draft}");
    std::fs::write(root.join(".harna/manifest.toml"), &draft).unwrap();
    let v = validate(&root);
    assert!(
        v.contains("verdict: PASS"),
        "an adopted repo passes as it is: {v}"
    );
    // the map knows who imports the store, across crates
    let d = harna(&root, &["dependents", "crates/store/src/lib.rs"]);
    assert!(d.contains("crates/ledger/src/lib.rs"), "{d}");
    assert!(ok(&root, &["doctor"]).contains("0 fail"));
    assert!(ok(&root, &["migrate", "--check"]).contains("every .harna/ file is current"));
    let _ = std::fs::remove_dir_all(&root);
}

/// A harna root inside a git subfolder (0.9.2, codex field test: codex
/// keeps its Rust in `codex-rs/`): the committed map, `diff` and parity
/// resolve from the harna root, not the git root.
#[test]
fn harna_root_in_a_git_subfolder() {
    let repo = std::env::temp_dir().join(format!("harna-suite-sub-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&repo);
    std::fs::create_dir_all(&repo).unwrap();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .unwrap()
        .success());
    let root = repo.join("rs");
    std::fs::create_dir_all(&root).unwrap();
    ok(&root, &["init"]);
    write(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n",
    );
    write(
        &root,
        "crates/core/Cargo.toml",
        "[package]\nname = \"core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(
        &root,
        "crates/core/src/lib.rs",
        "pub fn total(a: u64) -> u64 { a }\n",
    );
    ok(
        &root,
        &[
            "container",
            "add",
            "core",
            "--layer",
            "domain",
            "--path",
            "crates/core/src",
        ],
    );
    ok(&root, &["scan"]);
    commit(&repo);
    write(
        &root,
        "crates/core/src/lib.rs",
        "pub fn total(a: u64, b: u64) -> u64 { a + b }\n",
    );
    ok(&root, &["scan"]);
    let d = ok(&root, &["diff"]);
    assert!(
        d.contains("total"),
        "diff reads the committed map from the subfolder: {d}"
    );
    let p = ok(&root, &["parity", "open", "sub"]);
    assert!(p.contains("parity `sub`"), "{p}");
    // CODEOWNERS where GitHub reads it: the repository root, paths from there
    ok(&root, &["container", "set", "core", "--owners", "@core"]);
    ok(&root, &["render"]);
    let co = std::fs::read_to_string(repo.join(".github/CODEOWNERS")).unwrap();
    assert!(co.contains("/rs/crates/core/ @core"), "{co}");
    let _ = std::fs::remove_dir_all(&repo);
}

/// The 0.9.2 field tests, replayed: a corridor on a clean tree passes the
/// commit gate; a serde-renamed Rust shape matches its TS mirror whose
/// base lives in another file; a ring through crate-root re-exports is a
/// cycle.
#[test]
fn field_tests_092_regressions() {
    let root = project("r092");
    write(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n",
    );
    for c in ["events", "store", "ledger"] {
        write(
            &root,
            &format!("crates/{c}/Cargo.toml"),
            &format!("[package]\nname = \"{c}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        );
    }
    write(
        &root,
        "crates/events/src/lib.rs",
        "pub struct Posted {\n    pub id: u64,\n    #[serde(rename = \"postedAt\")]\n    pub at: String,\n    #[serde(skip)]\n    pub cache: u8,\n}\n",
    );
    write(
        &root,
        "crates/store/src/lib.rs",
        "pub mod repo;\npub use repo::Repo;\n",
    );
    write(
        &root,
        "crates/store/src/repo.rs",
        "pub struct Repo;\npub fn save() {}\n",
    );
    write(
        &root,
        "crates/ledger/src/lib.rs",
        "pub mod bal;\npub use bal::balance;\n",
    );
    write(&root, "crates/ledger/src/bal.rs", "pub fn balance() {}\n");
    write(
        &root,
        "web/src/base.ts",
        "export interface Stamped { id: number }\n",
    );
    write(
        &root,
        "web/src/posted.ts",
        "import { Stamped } from './base';\nexport interface Posted extends Stamped { postedAt: string }\n",
    );
    ok(
        &root,
        &[
            "container",
            "add",
            "events",
            "--layer",
            "shared",
            "--path",
            "crates/events/src",
        ],
    );
    ok(
        &root,
        &[
            "container",
            "add",
            "store",
            "--layer",
            "core",
            "--path",
            "crates/store/src",
        ],
    );
    ok(
        &root,
        &[
            "container",
            "add",
            "ledger",
            "--layer",
            "core",
            "--path",
            "crates/ledger/src",
        ],
    );
    ok(
        &root,
        &[
            "container",
            "add",
            "web",
            "--layer",
            "ui",
            "--path",
            "web/src",
        ],
    );
    ok(&root, &["layer", "set", "core", "--depends-on", "shared"]);
    ok(&root, &["layer", "set", "ui", "--depends-on", "shared"]);
    ok(&root, &["service", "add", "api", "--containers", "store"]);
    ok(&root, &["service", "add", "front", "--containers", "web"]);
    ok(
        &root,
        &[
            "service",
            "corridor",
            "add",
            "postings",
            "--from",
            "api",
            "--to",
            "front",
            "--message",
            "crates/events/src/lib.rs::Posted",
        ],
    );
    let v = validate(&root);
    assert!(
        !v.contains("[!corridor-shape]"),
        "serde rename + cross-file base match: {v}"
    );
    assert!(v.contains("verdict: PASS"), "{v}");
    // the commit gate on a clean tree with a corridor declared
    commit(&root);
    let v = harna(&root, &["validate", "--changed"]);
    assert!(
        v.contains("verdict: PASS"),
        "a clean tree passes the gate: {v}"
    );
    // a reader's copy of the shape drifts in a file that is not the
    // corridor's: --changed sees it with no scan (0.9.4, cost field test)
    write(
        &root,
        "web/src/posted.ts",
        "import { Stamped } from './base';\nexport interface Posted extends Stamped { postedOn: string }\n",
    );
    let v = harna(&root, &["validate", "--changed"]);
    assert!(v.contains("[!corridor-shape]"), "{v}");
    // the same field, another wire type: the Rust `at: String` mirrored as a
    // TS number (0.9.5, books field test 2d: only names were compared)
    write(
        &root,
        "web/src/posted.ts",
        "import { Stamped } from './base';\nexport interface Posted extends Stamped { postedAt: number }\n",
    );
    let v = harna(&root, &["validate", "--changed"]);
    assert!(
        v.contains("[!corridor-shape]")
            && v.contains("types differ: postedAt: number (number) here, string in the shape"),
        "{v}"
    );
    write(
        &root,
        "web/src/posted.ts",
        "import { Stamped } from './base';\nexport interface Posted extends Stamped { postedAt: string }\n",
    );
    // a ring made only through crate-root re-exports
    write(
        &root,
        "crates/store/src/repo.rs",
        "use ledger::balance;\npub struct Repo;\npub fn save() { balance(); }\n",
    );
    write(
        &root,
        "crates/ledger/src/bal.rs",
        "use store::Repo;\npub fn balance() {}\npub fn g(_r: Repo) {}\n",
    );
    let v = validate(&root);
    assert!(v.contains("[!no-cycles]"), "{v}");
    let _ = std::fs::remove_dir_all(&root);
}

/// The 0.9.4 cost field test, replayed: `scaffold --path` naming the
/// folder, and `--new-crate bin` whose component is the crate root.
#[test]
fn field_tests_094_scaffold() {
    let root = project("scaffold094");
    write(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n",
    );
    let s = ok(
        &root,
        &[
            "scaffold",
            "--layer",
            "services",
            "--container",
            "worker",
            "--name",
            "main",
            "--kind",
            "primitive",
            "--path",
            "crates/worker/src",
            "--new-crate",
            "bin",
        ],
    );
    assert!(s.contains("registered main"), "{s}");
    ok(
        &root,
        &[
            "scaffold",
            "--layer",
            "domain",
            "--container",
            "domain",
            "--name",
            "money",
            "--kind",
            "primitive",
            "--path",
            "crates/domain/src",
            "--new-crate",
            "lib",
        ],
    );
    assert!(root.join("crates/domain/src/money.rs").is_file());
    assert!(!root.join("crates/domain/src.rs").exists());
    let m = std::fs::read_to_string(root.join(".harna/manifest.toml")).unwrap();
    assert!(
        m.contains("path = \"crates/worker/src/main.rs\"")
            && m.contains("path = \"crates/domain/src\""),
        "{m}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The 0.9.5 cost field test, replayed: containers declared at the crate
/// folder (`crates/domain`, not `crates/domain/src`) are held by every
/// crate-level check.
#[test]
fn field_tests_095_crate_root_containers() {
    let root = project("croot095");
    write(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n",
    );
    let pkg = |n: &str, deps: &[&str]| {
        let mut s = format!("[package]\nname = \"{n}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n");
        for d in deps {
            s.push_str(&format!("{d} = {{ path = \"../{d}\" }}\n"));
        }
        s
    };
    write(&root, "crates/domain/Cargo.toml", &pkg("domain", &[]));
    write(
        &root,
        "crates/domain/src/lib.rs",
        "pub fn money() -> i64 { 1 }\n",
    );
    write(&root, "crates/events/Cargo.toml", &pkg("events", &[]));
    write(
        &root,
        "crates/events/src/lib.rs",
        "pub struct Posted { pub id: u64 }\n",
    );
    for (n, layer) in [("domain", "domain"), ("events", "contracts")] {
        ok(
            &root,
            &[
                "container",
                "add",
                n,
                "--layer",
                layer,
                "--path",
                &format!("crates/{n}"),
            ],
        );
    }
    assert!(validate(&root).contains("verdict: PASS"));
    write(
        &root,
        "crates/domain/Cargo.toml",
        &pkg("domain", &["events"]),
    );
    write(
        &root,
        "crates/domain/src/lib.rs",
        "pub fn money() -> i64 { 1 }\npub fn e(_p: events::Posted) {}\n",
    );
    let v = validate(&root);
    assert!(
        v.contains("[!cargo-layering]") && v.contains("[!layer-boundary]"),
        "{v}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
