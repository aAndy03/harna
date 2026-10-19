use super::*;
use crate::schema::model::rules::Rule;

fn rules(files: &[&str]) -> Rules {
    let mut r = Rules::default();
    r.rules.push(Rule {
        id: "extension-order".into(),
        text: "Extension registration order is identical in every worker".into(),
        rationale: "snapshot ops are indexed by registration order".into(),
        origin: "test".into(),
        check: Some("sync-group".into()),
        pattern: Some(r"(deno_\w+)::deno_\w+::(?:lazy_)?init".into()),
        values: files.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    });
    r
}

/// deno study D1 (0.3.45): four files keep the same extension list in
/// the same order, guarded only by a comment.
#[test]
fn every_site_yields_the_same_ordered_list() {
    let root = std::env::temp_dir().join(format!(
        "harna-sync-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("runtime")).unwrap();
    let write = |f: &str, body: &str| std::fs::write(root.join(f), body).unwrap();
    write("runtime/snapshot.rs", "deno_web::deno_web::lazy_init(),\ndeno_fs::deno_fs::lazy_init(),\ndeno_net::deno_net::lazy_init(),\n");
    write(
        "runtime/worker.rs",
        "deno_web::deno_web::init(a),\ndeno_fs::deno_fs::init(b),\ndeno_net::deno_net::init(c),\n",
    );
    write(
        "runtime/web_worker.rs",
        "deno_web::deno_web::init(a),\ndeno_net::deno_net::init(c),\ndeno_fs::deno_fs::init(b),\n",
    );
    write("runtime/snapshot_info.rs", "deno_web::deno_web::init(a),\ndeno_fs::deno_fs::init(b),\n// deno_net::deno_net::init(c),\n");

    let mut out = Vec::new();
    check(
        &root,
        &rules(&[
            "runtime/snapshot.rs",
            "runtime/worker.rs",
            "runtime/web_worker.rs",
            "runtime/snapshot_info.rs",
            "runtime/gone.rs",
        ]),
        None,
        &mut out,
    );
    let by_file = |f: &str| out.iter().find(|v| v.file == f).map(|v| v.message.clone());
    assert!(by_file("runtime/worker.rs").is_none(), "{out:?}");
    assert!(
        by_file("runtime/web_worker.rs")
            .unwrap()
            .contains("order differs"),
        "{out:?}"
    );
    assert!(by_file("runtime/snapshot_info.rs")
        .unwrap()
        .contains("missing deno_net"));
    assert!(by_file("runtime/gone.rs")
        .unwrap()
        .contains("does not exist"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_pattern_without_a_capture_group_is_named() {
    let mut r = rules(&["a.rs"]);
    r.rules[0].pattern = Some(r"deno_\w+".into());
    let mut out = Vec::new();
    check(Path::new("."), &r, None, &mut out);
    assert!(out[0].message.contains("one capture group"));
}
