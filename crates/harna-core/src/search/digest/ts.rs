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
fn code_lines(src: &str) -> Vec<String> {
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

pub fn scan_file(root: &Path, rel: &str) -> Module {
    let src = std::fs::read_to_string(root.join(rel)).unwrap_or_default();
    let p = patterns();
    let mut deps = Vec::new();
    let mut signatures = Vec::new();
    let mut inventory = Vec::new();
    let mut reexports = Vec::new();
    let add_dep = |spec: &str, deps: &mut Vec<String>| {
        let dep = if spec.starts_with('.') {
            match resolve(root, rel, spec) {
                Some(f) => format!("file:{f}"),
                None => format!("file?:{spec}"),
            }
        } else {
            format!("npm:{}", package_of(spec))
        };
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
        let m = scan_file(&root, "web/src/app.ts");
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
}
