use super::*;

fn files(spec: &[(&str, usize)]) -> Vec<(String, usize)> {
    spec.iter().map(|(p, n)| (p.to_string(), *n)).collect()
}

/// deno (0.3.44): 247 files past the default 600 lines on day one.
/// Calibrated at the repo's p95, the budget names only the outliers.
#[test]
fn big_repos_get_budgets_from_their_own_distribution() {
    let mut spec: Vec<(String, usize)> = (0..100)
        .map(|i| (format!("ext/m{}/ops_{i}.rs", i % 5), 300 + i * 12))
        .collect();
    spec.push(("runtime/permissions/lib.rs".into(), 11_756));
    spec.push(("cli/tests/huge_test.rs".into(), 20_000));
    let c = calibrate(&spec);
    let gran = c
        .iter()
        .find(|c| c.check == "component-granularity")
        .unwrap();
    assert!(
        gran.limit > DEFAULT_LINES && gran.limit % 100 == 0,
        "{gran:?}"
    );
    assert!(gran.limit < 11_756, "the giant stays an outlier: {gran:?}");
    assert!(
        gran.basis.contains("101 production files"),
        "test file excluded"
    );
    let folder = c.iter().find(|c| c.check == "folder-size").unwrap();
    assert_eq!(folder.limit, 20, "5 folders of 20 files");

    let cmd = gran.command();
    assert!(
        cmd.starts_with("harna add-rule --id calibrated-component-granularity"),
        "{cmd}"
    );
    assert!(cmd.contains(&format!("--limit {}", gran.limit)));
}

#[test]
fn small_tidy_repos_keep_the_defaults() {
    let c = calibrate(&files(&[
        ("src/a.rs", 120),
        ("src/b.rs", 300),
        ("src/c/mod.rs", 40),
    ]));
    assert!(c.is_empty(), "nothing above the defaults: {c:?}");
    assert!(calibrate(&[]).is_empty());
}
