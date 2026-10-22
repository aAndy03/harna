#[test]
fn tests_declarations_and_build_output_do_not_ship() {
    for p in ["src/App.tsx", "web/lib/api.ts"] {
        assert!(super::is_shipped(p), "{p}");
    }
    for p in [
        "src/App.test.tsx",
        "src/api.spec.ts",
        "src/__tests__/a.ts",
        "src/__mocks__/b.js",
        "e2e/login.ts",
        "types/global.d.ts",
        "vendor/x.min.js",
        "dist/index.js",
    ] {
        assert!(!super::is_shipped(p), "{p}");
    }
}

use super::*;

/// 0.6.0: imports in every spelling become edges — relative ones to
/// the file they mean (extension, `.js`-for-`.ts`, index), bare ones
/// to their package; exports become the surface; comments never count.
#[test]
fn a_ts_module_is_read_into_the_map() {
    let root = std::env::temp_dir().join(format!("harna-ts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let w = |rel: &str, body: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    w("web/src/money.ts", "export type Cents = number;\n");
    w("web/src/store/index.ts", "export const store = {};\n");
    w(
        "web/src/ui/Table.tsx",
        "export default function Table() {}\n",
    );
    w(
        "web/src/app.ts",
        concat!(
            "import { Cents } from './money.js';\n",
            "import { store } from \"./store\";\n",
            "import Table from './ui/Table';\n",
            "import React from 'react';\n",
            "import { invoke } from '@tauri-apps/api/core';\n",
            "import './side-effect.css';\n",
            "// import { nope } from './nope';\n",
            "const fs = require('node:fs');\n",
            "export async function load(): Promise<Cents> {\n",
            "  const inner = 1;\n",
            "  return import('./lazy');\n",
            "}\n",
            "export class Ledger {}\n",
            "interface Hidden {}\n",
            "export { load as boot };\n",
            "export * from './money';\n",
        ),
    );
    let m = scan_file(&root, &Context::default(), "web/src/app.ts");
    assert_eq!(m.lang, "typescript");
    assert_eq!(
        m.deps,
        vec![
            "file:web/src/money.ts",
            "file:web/src/store/index.ts",
            "file:web/src/ui/Table.tsx",
            "npm:react",
            "npm:@tauri-apps/api",
            "file?:./side-effect.css",
            "npm:node:fs",
            "file?:./lazy",
        ]
    );
    let names: Vec<(&str, &str)> = m
        .inventory
        .iter()
        .map(|i| (i.name.as_str(), i.vis.as_str()))
        .collect();
    assert_eq!(
        names,
        vec![
            ("fs", "priv"),
            ("load", "pub"),
            ("Ledger", "pub"),
            ("Hidden", "priv")
        ]
    );
    assert_eq!(m.reexports, vec!["boot", "*"]);
    assert!(is_source("web/src/app.tsx") && !is_source("node_modules/x/a.js"));
    let _ = std::fs::remove_dir_all(&root);
}

/// A mixed repo scans into one map: the Rust crate and the TS app side
/// by side, TS modules never dropped as crate orphans.
#[test]
fn a_mixed_repo_builds_one_map() {
    let root = std::env::temp_dir().join(format!("harna-ts-mixed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let w = |rel: &str, body: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    w("Cargo.toml", "[package]\nname = \"core\"\n");
    w("src/lib.rs", "pub fn total() {}\n");
    w(
        "web/src/app.ts",
        "import { fmt } from \"./fmt\";\nexport const app = fmt;\n",
    );
    w("web/src/fmt.ts", "export function fmt() {}\n");
    w("web/node_modules/x/index.js", "export const x = 1;\n");
    let d = crate::search::digest::build(&root, 1000).unwrap();
    let paths: Vec<&str> = d.modules.iter().map(|m| m.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["src/lib.rs", "web/src/app.ts", "web/src/fmt.ts"]
    );
    assert_eq!(d.modules[1].deps, vec!["file:web/src/fmt.ts"]);
    let _ = std::fs::remove_dir_all(&root);
}

/// 0.6.1: a Tauri command called from TS is an edge to its Rust file;
/// a call naming no exposed command is not; a rescan replaces, never
/// piles up, bridge edges.
#[test]
fn bridges_link_ts_calls_to_rust_commands() {
    let root = std::env::temp_dir().join(format!("harna-bridge-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let w = |rel: &str, body: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    w("Cargo.toml", "[package]\nname = \"app\"\n");
    w(
        "src/lib.rs",
        "#[tauri::command]\n#[allow(unused)]\npub async fn save_entry(x: u8) -> Result<(), String> { Ok(()) }\n",
    );
    w(
        "web/src/ipc.ts",
        "import { invoke } from '@tauri-apps/api/core';\nexport const save = () => invoke<void>('save_entry', {});\nexport const nope = () => invoke('missing');\n",
    );
    let mut d = crate::search::digest::build(&root, 1000).unwrap();
    let bridge = crate::schema::store::plugins::Bridge {
        name: "tauri-command".into(),
        rust: r"#\[tauri::command".into(),
        ts: r#"\binvoke(?:<[^>(]*>)?\(\s*['"`]NAME['"`]"#.into(),
    };
    assert_eq!(
        link_bridges(&root, &mut d, std::slice::from_ref(&bridge)),
        1
    );
    assert_eq!(link_bridges(&root, &mut d, &[bridge]), 1);
    let ipc = d
        .modules
        .iter()
        .find(|m| m.path == "web/src/ipc.ts")
        .unwrap();
    assert_eq!(
        ipc.deps
            .iter()
            .filter(|d| d.starts_with("bridge:"))
            .collect::<Vec<_>>(),
        vec!["bridge:src/lib.rs"]
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 0.6.2: a tsconfig alias (comments and trailing commas allowed) and a
/// workspace package resolve to the files they mean; other bare
/// specifiers stay packages.
#[test]
fn aliases_and_workspace_packages_resolve() {
    let root = std::env::temp_dir().join(format!("harna-ts-ctx-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let w = |rel: &str, body: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    w(
        "tsconfig.json",
        "{\n  // aliases\n  \"compilerOptions\": {\n    \"baseUrl\": \".\",\n    \"paths\": { \"@/*\": [\"web/src/*\"], },\n  },\n}\n",
    );
    w("web/src/money.ts", "export type Cents = number;\n");
    w("packages/ui/package.json", "{ \"name\": \"@app/ui\" }\n");
    w("packages/ui/src/index.ts", "export const Button = 1;\n");
    w("packages/ui/src/table.ts", "export const Table = 1;\n");
    let ctx = Context::load(&root);
    assert_eq!(
        dep_of(&root, &ctx, "web/src/app.ts", "@/money"),
        "file:web/src/money.ts"
    );
    assert_eq!(
        dep_of(&root, &ctx, "web/src/app.ts", "@app/ui"),
        "file:packages/ui/src/index.ts"
    );
    assert_eq!(
        dep_of(&root, &ctx, "web/src/app.ts", "@app/ui/table"),
        "file:packages/ui/src/table.ts"
    );
    assert_eq!(
        dep_of(&root, &ctx, "web/src/app.ts", "@/nope"),
        "file?:@/nope"
    );
    assert_eq!(dep_of(&root, &ctx, "web/src/app.ts", "react"), "npm:react");
    let _ = std::fs::remove_dir_all(&root);
}

/// 0.6.5 review H4/M3: strings, templates and regex literals never
/// open comments or blocks; a template's text is no import; multi-line
/// imports and export lists are read; `const enum`, `function *gen` and
/// `const a = 1, b = 2` are declarations.
#[test]
fn literals_never_derail_the_reader() {
    let root = std::env::temp_dir().join(format!("harna-ts-lex-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let w = |rel: &str, body: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    w("src/x.ts", "export const x = 1;\n");
    w(
        "src/a.ts",
        concat!(
            "const open = '{';\n",
            "const glob = \"api/*\";\n",
            "const re = /[}{]\\/*/g;\n",
            "const t = `import { evil } from './evil' ${1 + 2}`;\n",
            "import {\n",
            "  x,\n",
            "} from './x';\n",
            "import y from 'https://esm.sh/y';\n",
            "export const enum Dir { Up }\n",
            "export function *gen() {}\n",
            "export const a = 1, b = 2;\n",
            "export {\n",
            "  a as alpha,\n",
            "};\n",
        ),
    );
    let m = scan_file(&root, &Context::default(), "src/a.ts");
    assert_eq!(
        m.deps,
        vec!["file:src/x.ts", "url:https://esm.sh/y"],
        "{:?}",
        m.deps
    );
    let names: Vec<(&str, &str)> = m
        .inventory
        .iter()
        .filter(|i| i.vis == "pub")
        .map(|i| (i.name.as_str(), i.kind.as_str()))
        .collect();
    assert_eq!(
        names,
        vec![
            ("Dir", "enum"),
            ("gen", "fn"),
            ("a", "const"),
            ("b", "const")
        ]
    );
    assert_eq!(m.reexports, vec!["alpha"]);
    let _ = std::fs::remove_dir_all(&root);
}

/// 0.6.5 (deno field test): a `name` group in the Rust marker filters
/// the exposed fns, so a loose `\bNAME\b` on the JS side links only
/// real ops — destructured or not; a marker in a comment, or a call in
/// a JS comment, is no edge.
#[test]
fn a_named_marker_filters_what_a_loose_call_links() {
    let root = std::env::temp_dir().join(format!("harna-bridge-deno-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let w = |rel: &str, body: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    w("Cargo.toml", "[package]\nname = \"deno_fs\"\n");
    w(
        "src/lib.rs",
        concat!(
            "#[op2(fast)]\n",
            "pub fn op_fs_read(x: u8) {}\n",
            "#[op2]\n",
            "fn new() {}\n",
            "// #[op2]\n",
            "// fn op_fake() {}\n",
        ),
    );
    w(
        "ext/fs/30_fs.js",
        concat!(
            "const { op_fs_read, op_fake } = core.ops;\n",
            "// op_fs_read is called below\n",
            "const x = new Thing();\n",
        ),
    );
    let mut d = crate::search::digest::build(&root, 1000).unwrap();
    let bridge = crate::schema::store::plugins::Bridge {
        name: "deno-ops".into(),
        rust: r"#\[op2[^\n]*\]\s*\n\s*(?:pub\s+)?(?:async\s+)?fn\s+(?P<name>op_\w+)".into(),
        ts: r"\bNAME\b".into(),
    };
    assert_eq!(link_bridges(&root, &mut d, &[bridge]), 1);
    let js = d
        .modules
        .iter()
        .find(|m| m.path == "ext/fs/30_fs.js")
        .unwrap();
    assert!(
        js.deps.contains(&"bridge:src/lib.rs".to_string()),
        "{:?}",
        js.deps
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 0.6.5 review H3: an import of a file that appears later resolves on
/// the next incremental scan (TS modules are always re-read).
#[test]
fn a_later_file_resolves_on_rescan() {
    let root = std::env::temp_dir().join(format!("harna-ts-incr-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/d.ts"), "import { l } from './later';\n").unwrap();
    let first = crate::search::digest::build(&root, 1000).unwrap();
    assert_eq!(first.modules[0].deps, vec!["file?:./later"]);
    std::fs::write(root.join("src/later.ts"), "export const l = 1;\n").unwrap();
    let (second, _, _) =
        crate::search::digest::build_incremental(&root, 1000, Some(&first), false).unwrap();
    let d = second
        .modules
        .iter()
        .find(|m| m.path == "src/d.ts")
        .unwrap();
    assert_eq!(d.deps, vec!["file:src/later.ts"]);
    let _ = std::fs::remove_dir_all(&root);
}

/// 0.7.7 review: JSX closing tags, a regex after `return`, and braces
/// inside a template's `${…}` never derail the export reading.
#[test]
fn jsx_keyword_regexes_and_template_exprs() {
    let root = std::env::temp_dir().join(format!("harna-ts-jsx-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/v.tsx"),
        concat!(
            "export function View({ open }) { return <div>{open && <p>hi</p>}<br/></div>; }\n",
            "export function AfterJsx() {}\n",
            "export function test(s) { return /\\{/.test(s); }\n",
            "export const total = 1;\n",
            "export const msg = `a ${ { b: 1 }.b } c`;\n",
            "export function future() {}\n",
        ),
    )
    .unwrap();
    let m = scan_file(&root, &Context::default(), "src/v.tsx");
    let names: Vec<&str> = m.inventory.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["View", "AfterJsx", "test", "total", "msg", "future"]
    );
    let _ = std::fs::remove_dir_all(&root);
}
