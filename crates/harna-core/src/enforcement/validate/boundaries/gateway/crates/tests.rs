use super::*;
use crate::schema::model::manifest::{Container, Layer};

fn container(name: &str, path: &str, gateway: Option<&str>) -> Container {
    Container {
        name: name.into(),
        path: path.into(),
        gateway: gateway.map(str::to_string),
        ..Container::default()
    }
}

/// codex (0.3.48, R2): `deny.toml` wraps `reqwest` in `codex-http-client`,
/// "its intended owner". As a gateway: only the owner's crate depends on
/// it, only the owner's modules name it, and a rename still counts.
#[test]
fn only_the_gateway_crate_may_depend_on_or_import_an_owned_crate() {
    let root = std::env::temp_dir().join(format!(
        "harna-gw-crates-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let w = |rel: &str, body: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    w(
        "Cargo.toml",
        "[workspace]\nmembers = [\"http\", \"core\", \"cli\"]\n\
         [workspace.dependencies]\nnet = { package = \"reqwest\", version = \"0.12\" }\n",
    );
    w(
        "http/Cargo.toml",
        "[package]\nname = \"codex-http-client\"\n[dependencies]\nreqwest = \"0.12\"\n",
    );
    w(
        "core/Cargo.toml",
        "[package]\nname = \"codex-core\"\n[dependencies]\nnet.workspace = true\n\
         [dev-dependencies]\nreqwest = \"0.12\"\n",
    );
    w(
        "cli/Cargo.toml",
        "[package]\nname = \"codex-cli\"\n[dependencies]\nreqwest_middleware = \"0.4\"\n",
    );
    let manifest = Manifest {
        layers: vec![Layer {
            name: "l".into(),
            containers: vec![
                container("http", "http/src", Some("crate:reqwest")),
                container("core", "core/src", None),
            ],
            ..Layer::default()
        }],
        ..Manifest::default()
    };
    let srcs = vec![
        (
            "http/src/lib.rs".to_string(),
            "use reqwest::Client;\n".to_string(),
        ),
        (
            "core/src/fetch.rs".to_string(),
            "fn f() { let c = reqwest::Client::new(); }\n".to_string(),
        ),
        (
            "cli/src/main.rs".to_string(),
            "use reqwest_middleware::X;\nfn g() { crate::reqwest::y(); }\n".to_string(),
        ),
    ];
    let mut out = Vec::new();
    check(&root, &manifest, &srcs, &mut out);
    let got: Vec<(String, usize)> = out.iter().map(|v| (v.file.clone(), v.line)).collect();
    assert_eq!(
        got,
        vec![
            ("core/Cargo.toml".to_string(), 0),
            ("core/src/fetch.rs".to_string(), 1)
        ],
        "{out:?}"
    );
    assert!(
        out[0].message.contains("depends on `reqwest`"),
        "{}",
        out[0].message
    );
    assert!(out[0].message.contains("http/src"));

    // `harna deny` renders the owner as the only wrapper.
    let plan = crate::generation::views::deny::plan(&root, &manifest).unwrap();
    let rule = plan
        .rules
        .iter()
        .find(|r| r.crate_name == "reqwest")
        .unwrap();
    assert_eq!(rule.wrappers, vec!["codex-http-client".to_string()]);
    assert!(crate::generation::views::deny::render_toml(&plan)
        .contains("crate = \"reqwest\", wrappers = [\"codex-http-client\"], reason = \"gateway http/src only\""));
    let _ = std::fs::remove_dir_all(&root);
}
