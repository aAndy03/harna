use super::*;
use crate::schema::model::manifest::{Container, Layer};

fn manifest() -> Manifest {
    Manifest {
        schema_version: 1,
        layers: vec![Layer {
            name: "runtime".into(),
            containers: vec![
                Container {
                    name: "permissions".into(),
                    path: "runtime/permissions".into(),
                    owns: vec!["CheckedPath".into(), "#[op2".into()],
                    escapes: vec!["CheckedPath::unsafe_new".into()],
                    ..Default::default()
                },
                Container {
                    name: "fs".into(),
                    path: "ext/fs".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn run(files: &[(&str, &str)]) -> Vec<Violation> {
    let srcs: Vec<(String, String)> = files
        .iter()
        .map(|(p, s)| (p.to_string(), s.to_string()))
        .collect();
    let mut out = Vec::new();
    check(&manifest(), &srcs, &mut out);
    out
}

/// deno study B1/B2/B5 (0.3.45): only the permission layer builds a
/// CheckedPath; ops declared outside the owner are named; every
/// `unsafe_new` is counted so a baseline can ratchet it.
#[test]
fn only_the_owner_constructs_and_every_escape_is_counted() {
    let out = run(&[
        (
            "runtime/permissions/lib.rs",
            "pub struct CheckedPath { path: P }\n\
             impl CheckedPath { pub fn unsafe_new(p: P) -> Self { CheckedPath { path: p } } }\n\
             pub fn check(p: P) -> CheckedPath { CheckedPath { path: p } }\n\
             #[op2(fast)]\nfn op_ok() {}\n",
        ),
        (
            "ext/fs/ops.rs",
            "fn open(p: &CheckedPath) -> Result<File, E> { fs.open(p) }\n\
             fn forged(p: P) { let c = CheckedPath { path: p }; }\n\
             fn built(p: P) { let c = CheckedPath::new(p); }\n\
             fn trusted(p: P) { let c = CheckedPath::unsafe_new(p); }\n\
             #[op2]\nfn op_fs_open() {}\n\
             // CheckedPath { comment } is not code\n",
        ),
    ]);
    let lines = |check: &str, file: &str| -> Vec<usize> {
        out.iter()
            .filter(|v| v.check == check && v.file == file)
            .map(|v| v.line)
            .collect()
    };
    assert_eq!(
        lines("ownership", "ext/fs/ops.rs"),
        vec![2, 3, 5],
        "{out:?}"
    );
    assert!(lines("ownership", "runtime/permissions/lib.rs").is_empty());
    assert_eq!(lines("escape-hatch", "ext/fs/ops.rs"), vec![4]);
    assert!(
        lines("escape-hatch", "runtime/permissions/lib.rs").is_empty(),
        "the definition is not a call"
    );
    let forged = out.iter().find(|v| v.line == 2).unwrap();
    assert!(forged.message.contains("constructs `CheckedPath`"));
    assert!(forged.message.contains("container `permissions` owns"));
    let op = out.iter().find(|v| v.line == 5).unwrap();
    assert!(op.message.contains("declares the marker `#[op2`"));
}

#[test]
fn naming_returning_and_implementing_are_not_construction() {
    for src in [
        "fn f() -> CheckedPath { todo() }",
        "impl From<P> for CheckedPath { }",
        "struct Wrapper { inner: CheckedPath }",
        "fn g(x: Vec<CheckedPath>) {}",
        "let y = CheckedPathBuf { a };",
    ] {
        assert!(run(&[("ext/fs/x.rs", src)]).is_empty(), "{src}");
    }
}
