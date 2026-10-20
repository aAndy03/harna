use super::*;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "harna-profile-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Every shipped profile loads, every register in it parses as harna
/// reads it, and the gpui-kit one says which upstream it mirrors.
#[test]
fn shipped_profiles_are_valid_data() {
    let all = shipped();
    assert!(!all.is_empty());
    for p in &all {
        for (f, t) in &p.files {
            validate(f, t).unwrap_or_else(|e| panic!("{}: {e:#}", p.meta.name));
        }
    }
    let gpui = all
        .iter()
        .find(|p| p.meta.name == "rust-gpui-desktop")
        .unwrap();
    assert_eq!(
        gpui.meta.mirrors.get("gpui-kit").map(String::as_str),
        Some("0.8.0")
    );
    assert!(gpui.files.iter().any(|(f, _)| f == "qualities.toml"));
}

/// A fresh project initialized from the gpui-kit profile validates clean:
/// declared-but-empty containers are intent, the features layer is
/// isolated, and the headless rule binds the shared crates.
#[test]
fn a_fresh_project_from_the_gpui_profile_validates() {
    let root = tmp("gpui");
    let store = Store::open(&root).unwrap();
    let p = resolve("rust-gpui-desktop", None).unwrap();
    let lines = apply(&store, &root, &p).unwrap();
    assert!(
        lines.iter().any(|l| l.contains("mirrors gpui-kit 0.8.0")),
        "{lines:?}"
    );
    let m = store.load_manifest().unwrap();
    assert!(m.layers.iter().any(|l| l.name == "features" && l.isolated));
    assert!(!store.load_qualities().unwrap().is_empty());
    let v = crate::enforcement::validate::run(&root, &store, 10_000).unwrap();
    let errors: Vec<_> = v
        .iter()
        .filter(|x| x.severity == crate::enforcement::violation::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
}

/// A folder profile substitutes the project name, replaces the starters,
/// and never overwrites a register the project already has.
#[test]
fn folder_profiles_substitute_and_never_overwrite_your_registers() {
    let prof = tmp("folder-src");
    std::fs::write(
        prof.join("profile.toml"),
        "name = \"svc\"\ndescription = \"d\"\nnext = [\"build {{project}}\"]\n",
    )
    .unwrap();
    std::fs::write(
        prof.join("manifest.toml"),
        "schema_version = 1\n[[layers]]\nname = \"core\"\n[[layers.containers]]\nname = \"k\"\npath = \"crates/{{project}}/src\"\n",
    )
    .unwrap();
    std::fs::write(
        prof.join("terms.toml"),
        "schema_version = 1\n[[terms]]\nword = \"x\"\nmeans = \"y\"\n",
    )
    .unwrap();
    let root = tmp("My App");
    let store = Store::open(&root).unwrap();
    store.init().unwrap();
    std::fs::write(
        store.manifest_path().parent().unwrap().join("terms.toml"),
        "schema_version = 1\n",
    )
    .unwrap();
    let p = resolve(prof.to_str().unwrap(), None).unwrap();
    let lines = apply(&store, &root, &p).unwrap();
    let m = std::fs::read_to_string(store.manifest_path()).unwrap();
    assert!(
        m.contains("-my-app-") && !m.contains("{{project}}"),
        "kebab of the folder name: {m}"
    );
    assert!(
        lines.iter().any(|l| l.contains("kept your terms.toml")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("  next 1: build harna-profile-my-app")),
        "{lines:?}"
    );
    // a broken register refuses the whole profile, before anything is written
    std::fs::write(prof.join("rules.toml"), "not toml [[[").unwrap();
    let p = resolve(prof.to_str().unwrap(), None).unwrap();
    assert!(apply(&store, &root, &p).is_err());
    assert!(resolve("no-such-profile", None).is_err());
}
