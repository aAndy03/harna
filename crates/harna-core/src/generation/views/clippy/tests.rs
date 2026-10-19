use super::*;
use crate::schema::model::rules::Rule;

/// deno study B3 (0.3.45): 61 hand-kept clippy.toml files carry the same
/// bans a lint script re-checks. One rule, written where clippy reads it;
/// existing keys and entries survive; a narrower scope writes nothing.
#[test]
fn banned_api_rules_land_in_each_covered_crates_clippy_toml() {
    let root = std::env::temp_dir().join(format!(
        "harna-clippy-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let w = |rel: &str, body: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    w("Cargo.toml", "[workspace]\nmembers = [\"ext/*\"]\n");
    w(
        "ext/fs/Cargo.toml",
        "[package]\nname = \"deno_fs\"\n[lib]\npath = \"lib.rs\"\n",
    );
    w(
        "ext/net/Cargo.toml",
        "[package]\nname = \"deno_net\"\n[lib]\npath = \"lib.rs\"\n",
    );
    w(
        "ext/fs/clippy.toml",
        "msrv = \"1.80\"\ndisallowed-methods = [{ path = \"std::env::var\", reason = \"config\" }]\n",
    );
    let mut rules = Rules::default();
    rules.rules.push(Rule {
        id: "fs-through-the-trait".into(),
        text: "t".into(),
        rationale: "should be done using FileSystem trait".into(),
        origin: "o".into(),
        scope: vec!["ext/fs".into()],
        check: Some("banned-api".into()),
        values: vec![
            "std::fs::read => deno_fs::FileSystem::read_file".into(),
            "std::env::var".into(),
        ],
        ..Default::default()
    });
    rules.rules.push(Rule {
        id: "narrow".into(),
        text: "t".into(),
        rationale: "r".into(),
        origin: "o".into(),
        scope: vec!["ext/net/tcp".into()],
        check: Some("banned-api".into()),
        values: vec!["std::net::TcpStream::connect".into()],
        ..Default::default()
    });

    assert_eq!(
        drift(&root, &rules).get("ext/fs").cloned(),
        Some(vec!["std::fs::read".to_string()])
    );
    let written = generate(&root, &rules).unwrap();
    assert_eq!(written.len(), 1, "{written:?}");
    let doc = std::fs::read_to_string(root.join("ext/fs/clippy.toml")).unwrap();
    assert!(doc.contains("msrv"), "other keys kept:\n{doc}");
    assert!(doc.contains("std::env::var") && doc.contains("std::fs::read"));
    assert!(doc.contains("FileSystem trait"));
    // R1 (0.3.48): the replacement lands where clippy reads it.
    assert!(
        doc.contains("replacement = \"deno_fs::FileSystem::read_file\""),
        "{doc}"
    );
    assert!(
        !root.join("ext/net/clippy.toml").exists(),
        "narrower scope: no crate-wide ban"
    );
    assert!(drift(&root, &rules).is_empty());
    assert!(generate(&root, &rules).unwrap().is_empty(), "idempotent");
    let _ = std::fs::remove_dir_all(&root);
}

/// ledger field test (0.3.49): `f64 => Money` and `Utc::now => Clock::now`
/// were written as clippy `disallowed-methods`; clippy warned "expected a
/// function, found a primitive type" on every run and banned nothing. Only
/// paths clippy can resolve land in clippy.toml.
#[test]
fn only_paths_clippy_resolves_are_written() {
    let root = std::env::temp_dir().join(format!(
        "harna-clippy-resolve-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let w = |rel: &str, body: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    w("Cargo.toml", "[workspace]\nmembers = [\"crates/domain\"]\n");
    w(
        "crates/domain/Cargo.toml",
        "[package]\nname = \"domain\"\n[dependencies]\nchrono = \"0.4\"\n",
    );
    let mut rules = Rules::default();
    rules.rules.push(Rule {
        id: "money".into(),
        rationale: "exact decimals".into(),
        scope: vec!["crates/domain".into()],
        check: Some("banned-api".into()),
        values: vec![
            "f64 => Money".into(),
            "Utc::now => Clock::now".into(),
            "chrono::Utc::now => Clock::now".into(),
            "std::time::SystemTime::now".into(),
        ],
        ..Default::default()
    });
    let req = required(&root, &rules);
    let paths: Vec<&str> = req["crates/domain"]
        .iter()
        .map(|(p, _, _)| p.as_str())
        .collect();
    assert_eq!(
        paths,
        vec!["chrono::Utc::now", "std::time::SystemTime::now"]
    );
    // An entry an older harna wrote for `f64` is withdrawn; a user's own stays.
    w(
        "crates/domain/clippy.toml",
        "disallowed-methods = [{ path = \"f64\", reason = \"x (harna rule `money`)\" }, { path = \"std::env::var\", reason = \"mine\" }]\n",
    );
    generate(&root, &rules).unwrap();
    let doc = std::fs::read_to_string(root.join("crates/domain/clippy.toml")).unwrap();
    assert!(
        !doc.contains("\"f64\"")
            && doc.contains("std::env::var")
            && doc.contains("chrono::Utc::now"),
        "{doc}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
