//! Component `ts-reader` (0.6.0): TypeScript and JavaScript into the same
//! map as Rust — a module per file, its exports as signatures and
//! inventory, its imports as edges.
//!
//! Text, not a compiler: `import … from '…'`, `import '…'`,
//! `export … from '…'`, `require('…')` and `import('…')` name the edges;
//! `export` declarations, `export { a, b }` and `export default` name the
//! surface. A relative specifier resolves to the repository file it means
//! (`./x` → `x.ts`, `x.tsx`, `x/index.ts`, …) and becomes `file:<path>`;
//! a bare one is a package, `npm:<name>` (`@scope/pkg` kept whole). The
//! Rust resolver never sees either: they are a different edge kind.

use std::path::Path;

use regex::Regex;

use super::{hash_body, Item, Module};

/// File extensions the reader takes, and the order a specifier without
/// one is resolved in.
pub const EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

pub fn is_source(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    !p.split('/').any(|s| s == "node_modules")
        && EXTENSIONS.iter().any(|e| p.ends_with(&format!(".{e}")))
}

fn lang_of(path: &str) -> &'static str {
    let p = path.to_ascii_lowercase();
    if [".ts", ".tsx", ".mts", ".cts"]
        .iter()
        .any(|e| p.ends_with(e))
    {
        "typescript"
    } else {
        "javascript"
    }
}

struct Patterns {
    from: Regex,
    bare: Regex,
    call: Regex,
    decl: Regex,
    list: Regex,
}

fn patterns() -> &'static Patterns {
    static P: std::sync::OnceLock<Patterns> = std::sync::OnceLock::new();
    P.get_or_init(|| Patterns {
        from: Regex::new(r#"\bfrom\s*['"]([^'"]+)['"]"#).expect("static"),
        bare: Regex::new(r#"^\s*import\s*['"]([^'"]+)['"]"#).expect("static"),
        call: Regex::new(r#"\b(?:require|import)\s*\(\s*['"]([^'"]+)['"]\s*\)"#).expect("static"),
        decl: Regex::new(
            r"^\s*(export\s+)?(default\s+)?(declare\s+)?(abstract\s+)?(async\s+)?(function\*?|class|interface|type|enum|const|let|var|namespace)\s+([A-Za-z_$][\w$]*)",
        )
        .expect("static"),
        list: Regex::new(r"^\s*export\s+(type\s+)?\{([^}]*)\}").expect("static"),
    })
}

/// Comment-stripped lines: `//` tails and `/* … */` blocks removed (strings
/// containing `//` are rare enough in import/export lines to accept).
pub fn code_lines(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_block = false;
    for raw in src.lines() {
        let mut line = String::new();
        let mut rest = raw;
        loop {
            if in_block {
                match rest.find("*/") {
                    Some(end) => {
                        in_block = false;
                        rest = &rest[end + 2..];
                    }
                    None => break,
                }
            } else {
                let block = rest.find("/*");
                let tail = rest.find("//");
                match (block, tail) {
                    (Some(b), t) if t.is_none_or(|t| b < t) => {
                        line.push_str(&rest[..b]);
                        in_block = true;
                        rest = &rest[b + 2..];
                    }
                    (_, Some(t)) => {
                        line.push_str(&rest[..t]);
                        break;
                    }
                    _ => {
                        line.push_str(rest);
                        break;
                    }
                }
            }
        }
        out.push(line);
    }
    out
}

/// The repository file a relative specifier means, if it exists.
pub fn resolve(root: &Path, importer: &str, spec: &str) -> Option<String> {
    let dir = Path::new(importer).parent().unwrap_or(Path::new(""));
    let joined = normalize(&dir.join(spec).to_string_lossy().replace('\\', "/"));
    let exists = |p: &str| root.join(p).is_file();
    if Path::new(&joined).extension().is_some() && exists(&joined) {
        return Some(joined);
    }
    // TypeScript's ESM convention: `./x.js` written for `x.ts`
    if let Some(stem) = joined
        .strip_suffix(".js")
        .or_else(|| joined.strip_suffix(".jsx"))
    {
        for e in ["ts", "tsx"] {
            let c = format!("{stem}.{e}");
            if exists(&c) {
                return Some(c);
            }
        }
    }
    for e in EXTENSIONS {
        let c = format!("{joined}.{e}");
        if exists(&c) {
            return Some(c);
        }
    }
    for e in EXTENSIONS {
        let c = format!("{joined}/index.{e}");
        if exists(&c) {
            return Some(c);
        }
    }
    None
}

fn normalize(p: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

/// The package a bare specifier names: `react-dom/client` → `react-dom`,
/// `@tauri-apps/api/core` → `@tauri-apps/api`; `node:fs` → `node:fs`.
pub fn package_of(spec: &str) -> String {
    if spec.starts_with("node:") {
        return spec.split('/').next().unwrap_or(spec).to_string();
    }
    let mut parts = spec.split('/');
    match parts.next() {
        Some(scope) if scope.starts_with('@') => {
            format!("{scope}/{}", parts.next().unwrap_or(""))
        }
        Some(name) => name.to_string(),
        None => spec.to_string(),
    }
}

/// What a bare specifier can mean inside the repository (0.6.2): the root
/// `tsconfig.json`'s `paths` aliases (`@/*` → `src/*`) and the workspace's
/// own packages (a `package.json` `name` → its folder). Read once per scan.
#[derive(Debug, Default, Clone)]
pub struct Context {
    /// (alias pattern, target patterns), `*` standing for the rest.
    aliases: Vec<(String, Vec<String>)>,
    /// package name -> its folder (repo-relative)
    packages: Vec<(String, String)>,
}

impl Context {
    pub fn load(root: &Path) -> Context {
        let mut ctx = Context::default();
        if let Some(cfg) = read_jsonc(&root.join("tsconfig.json")) {
            let opts = cfg.get("compilerOptions");
            let base = opts
                .and_then(|o| o.get("baseUrl"))
                .and_then(|b| b.as_str())
                .unwrap_or(".")
                .to_string();
            if let Some(paths) = opts
                .and_then(|o| o.get("paths"))
                .and_then(|p| p.as_object())
            {
                for (alias, targets) in paths {
                    let targets: Vec<String> = targets
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|t| t.as_str())
                                .map(|t| normalize(&format!("{base}/{t}")))
                                .collect()
                        })
                        .unwrap_or_default();
                    ctx.aliases.push((alias.clone(), targets));
                }
            }
        }
        let files = super::respected_files_where(root, 100_000, |p| {
            p.ends_with("package.json") && !p.split('/').any(|s| s == "node_modules")
        });
        for f in files {
            let Some(pkg) = read_jsonc(&root.join(&f)) else {
                continue;
            };
            if let Some(name) = pkg.get("name").and_then(|n| n.as_str()) {
                let dir = Path::new(&f)
                    .parent()
                    .map(|d| d.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                ctx.packages.push((name.to_string(), dir));
            }
        }
        // the longest name wins (`@app/ui-kit` before `@app/ui`)
        ctx.packages
            .sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));
        ctx
    }
}

/// JSON with the comments and trailing commas tsconfig allows.
fn read_jsonc(path: &Path) -> Option<serde_json::Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    // comments stripped outside strings only: `"@/*"` is an alias, not a
    // comment opener
    let mut text = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let (mut i, mut in_str) = (0, false);
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            text.push(c);
            if c == '\\' && i + 1 < chars.len() {
                text.push(chars[i + 1]);
                i += 1;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            text.push(c);
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        } else {
            text.push(c);
        }
        i += 1;
    }
    let no_trailing = Regex::new(r",(\s*[}\]])")
        .ok()?
        .replace_all(&text, "$1")
        .to_string();
    serde_json::from_str(&no_trailing).ok()
}

/// One specifier as an edge: a repository file (`file:`), an unresolved
/// relative or alias (`file?:`), or a package (`npm:`).
pub fn dep_of(root: &Path, ctx: &Context, importer: &str, spec: &str) -> String {
    if spec.starts_with('.') {
        return match resolve(root, importer, spec) {
            Some(f) => format!("file:{f}"),
            None => format!("file?:{spec}"),
        };
    }
    // a tsconfig alias: resolved against the repository root
    for (alias, targets) in &ctx.aliases {
        let (prefix, star) = match alias.strip_suffix('*') {
            Some(p) => (p, true),
            None => (alias.as_str(), false),
        };
        let rest = if star {
            spec.strip_prefix(prefix)
        } else {
            (spec == prefix).then_some("")
        };
        let Some(rest) = rest else { continue };
        for t in targets {
            let candidate = t.replace('*', rest);
            if let Some(f) = resolve(root, "x", &format!("./{candidate}")) {
                return format!("file:{f}");
            }
        }
        return format!("file?:{spec}");
    }
    // a workspace package: its folder, then the subpath or its entry
    for (name, dir) in &ctx.packages {
        let rest = if spec == name {
            Some("")
        } else {
            spec.strip_prefix(&format!("{name}/"))
        };
        let Some(rest) = rest else { continue };
        let base = if dir.is_empty() {
            ".".to_string()
        } else {
            dir.clone()
        };
        let tries = if rest.is_empty() {
            vec![format!("{base}/src/index"), format!("{base}/index")]
        } else {
            vec![format!("{base}/{rest}"), format!("{base}/src/{rest}")]
        };
        for t in tries {
            if let Some(f) = resolve(root, "x", &format!("./{t}")) {
                return format!("file:{f}");
            }
        }
        return format!("file?:{spec}");
    }
    format!("npm:{}", package_of(spec))
}

pub fn scan_file(root: &Path, ctx: &Context, rel: &str) -> Module {
    let src = std::fs::read_to_string(root.join(rel)).unwrap_or_default();
    let p = patterns();
    let mut deps = Vec::new();
    let mut signatures = Vec::new();
    let mut inventory = Vec::new();
    let mut reexports = Vec::new();
    let add_dep = |spec: &str, deps: &mut Vec<String>| {
        let dep = dep_of(root, ctx, rel, spec);
        if !deps.contains(&dep) {
            deps.push(dep);
        }
    };
    let lines = code_lines(&src);
    let mut depth: i32 = 0;
    for line in &lines {
        let t = line.trim();
        for cap in p.from.captures_iter(t) {
            add_dep(&cap[1], &mut deps);
        }
        if let Some(cap) = p.bare.captures(t) {
            add_dep(&cap[1], &mut deps);
        }
        for cap in p.call.captures_iter(t) {
            add_dep(&cap[1], &mut deps);
        }
        if depth == 0 {
            if let Some(cap) = p.decl.captures(t) {
                let exported = cap.get(1).is_some();
                let kind = match &cap[6] {
                    "function" | "function*" => "fn",
                    "class" => "class",
                    "interface" => "interface",
                    "type" => "type",
                    "enum" => "enum",
                    "namespace" => "mod",
                    _ => "const",
                };
                inventory.push(Item {
                    vis: if exported { "pub" } else { "priv" }.into(),
                    kind: kind.into(),
                    name: cap[7].to_string(),
                    text: t.trim_end_matches('{').trim().to_string(),
                });
                if exported {
                    signatures.push(t.trim_end_matches('{').trim().to_string());
                }
            } else if t.starts_with("export default") {
                signatures.push(t.trim_end_matches('{').trim().to_string());
                inventory.push(Item {
                    vis: "pub".into(),
                    kind: "const".into(),
                    name: "default".into(),
                    text: t.to_string(),
                });
            } else if let Some(cap) = p.list.captures(t) {
                signatures.push(t.to_string());
                for part in cap[2].split(',') {
                    let name = part.split(" as ").last().unwrap_or("").trim();
                    if !name.is_empty() {
                        reexports.push(name.to_string());
                    }
                }
            } else if t.starts_with("export *") {
                signatures.push(t.to_string());
                reexports.push("*".into());
            }
        }
        depth += t.matches('{').count() as i32 - t.matches('}').count() as i32;
        depth = depth.max(0);
    }
    Module {
        path: rel.to_string(),
        lang: lang_of(rel).into(),
        signatures,
        deps,
        body_hash: hash_body(src.as_bytes()),
        inventory,
        mods: Vec::new(),
        crate_dir: None,
        src_root: None,
        reexports,
    }
}

/// Cross-language edges (0.6.1): a plugin's bridges say how Rust exposes a
/// function (`#[tauri::command]` before a `fn`) and how TypeScript calls
/// it (`invoke('NAME')`). Each call becomes a `bridge:<rust file>` edge on
/// the TS module, so layers, dependents and parity see across the
/// language line. Recomputed whole on every scan (the edges of a reused
/// module may be stale); returns how many edges were linked.
pub fn link_bridges(
    root: &Path,
    digest: &mut super::Digest,
    bridges: &[crate::schema::store::plugins::Bridge],
) -> usize {
    for m in &mut digest.modules {
        m.deps.retain(|d| !d.starts_with("bridge:"));
    }
    if bridges.is_empty() {
        return 0;
    }
    let fn_name = Regex::new(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)").expect("static");
    let mut linked = 0;
    for b in bridges {
        let (Ok(marker), Ok(call)) = (
            Regex::new(&b.rust),
            Regex::new(&b.ts.replace("NAME", "([A-Za-z_][A-Za-z0-9_]*)")),
        ) else {
            continue; // the doctor/admission name a broken bridge
        };
        // exposed name -> Rust file
        let mut exposed: std::collections::HashMap<String, String> = Default::default();
        for m in digest.modules.iter().filter(|m| m.lang == "rust") {
            let Ok(src) = std::fs::read_to_string(root.join(&m.path)) else {
                continue;
            };
            let lines: Vec<&str> = src.lines().collect();
            for (i, l) in lines.iter().enumerate() {
                if !marker.is_match(l) {
                    continue;
                }
                // the fn the marker stands before (attributes may stack)
                if let Some(cap) = lines
                    .iter()
                    .skip(i)
                    .take(6)
                    .find_map(|x| fn_name.captures(x))
                {
                    exposed.entry(cap[1].to_string()).or_insert(m.path.clone());
                }
            }
        }
        if exposed.is_empty() {
            continue;
        }
        for m in digest
            .modules
            .iter_mut()
            .filter(|m| m.lang == "typescript" || m.lang == "javascript")
        {
            let Ok(src) = std::fs::read_to_string(root.join(&m.path)) else {
                continue;
            };
            for cap in call.captures_iter(&src) {
                if let Some(target) = exposed.get(&cap[1]) {
                    let dep = format!("bridge:{target}");
                    if !m.deps.contains(&dep) {
                        m.deps.push(dep);
                        linked += 1;
                    }
                }
            }
        }
    }
    linked
}

#[cfg(test)]
mod tests {
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
}
