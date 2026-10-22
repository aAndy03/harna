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

mod lex;
mod resolve;

pub use lex::{code_lines, lex, Lexed};
pub use resolve::{dep_of, package_of, resolve, Context};

/// File extensions the reader takes, and the order a specifier without
/// one is resolved in.
pub const EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

pub fn is_source(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    !p.split('/').any(|s| s == "node_modules")
        && EXTENSIONS.iter().any(|e| p.ends_with(&format!(".{e}")))
}

/// A web source that ships, as the text checks read it (review of 1.1.2):
/// not a test (`*.test.*`, `*.spec.*`, `__tests__/`, `__mocks__/`,
/// `test/`, `e2e/`), not a declaration file (`.d.ts`), not minified, not
/// build output (`dist/`, `build/`).
pub fn is_shipped(path: &str) -> bool {
    let p = path.to_ascii_lowercase().replace('\\', "/");
    let name = p.rsplit('/').next().unwrap_or(&p);
    is_source(&p)
        && !name.contains(".test.")
        && !name.contains(".spec.")
        && !name.contains(".min.")
        && !name.ends_with(".d.ts")
        && !p.split('/').any(|s| {
            matches!(
                s,
                "__tests__" | "__mocks__" | "test" | "tests" | "e2e" | "dist" | "build"
            )
        })
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
        // over the joined code: an import may span lines
        from: Regex::new(r#"\b(?:import|export)\b[^;'"`]*?\bfrom\s*['"]([^'"\n]+)['"]"#).expect("static"),
        bare: Regex::new(r#"(?m)^\s*import\s*['"]([^'"\n]+)['"]"#).expect("static"),
        call: Regex::new(r#"\b(?:require|import)\s*\(\s*['"]([^'"\n]+)['"]\s*\)"#).expect("static"),
        decl: Regex::new(
            r"^\s*(export\s+)?(default\s+)?(declare\s+)?(abstract\s+)?(async\s+)?(const\s+enum|function|class|interface|type|enum|const|let|var|namespace)(?:\s*\*\s*|\s+)([A-Za-z_$][\w$]*)",
        )
        .expect("static"),
        list: Regex::new(r"\bexport\s+(type\s+)?\{([^}]*)\}").expect("static"),
    })
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
    let lexed = lex(&src);
    let joined = lexed.code.join("\n");
    for cap in p.from.captures_iter(&joined) {
        add_dep(&cap[1], &mut deps);
    }
    for cap in p.bare.captures_iter(&joined) {
        add_dep(&cap[1], &mut deps);
    }
    for cap in p.call.captures_iter(&joined) {
        add_dep(&cap[1], &mut deps);
    }
    // export lists, one or many lines: `export { a, b as c }`
    for cap in p.list.captures_iter(&joined) {
        signatures.push(cap[0].split_whitespace().collect::<Vec<_>>().join(" "));
        for part in cap[2].split(',') {
            let name = part.split(" as ").last().unwrap_or("").trim();
            if !name.is_empty() {
                reexports.push(name.to_string());
            }
        }
    }
    static EXTRA: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let extra = EXTRA
        .get_or_init(|| Regex::new(r",\s*([A-Za-z_$][\w$]*)\s*(?::[^=,]*)?=").expect("static"));
    let mut depth: i32 = 0;
    for (line, shape) in lexed.code.iter().zip(&lexed.shape) {
        let t = line.trim();
        if depth == 0 {
            if let Some(cap) = p.decl.captures(t) {
                let exported = cap.get(1).is_some();
                let kind = match cap[6].split_whitespace().last().unwrap_or("") {
                    "function" => "fn",
                    "class" => "class",
                    "interface" => "interface",
                    "type" => "type",
                    "enum" => "enum",
                    "namespace" => "mod",
                    _ => "const",
                };
                let mut names = vec![cap[7].to_string()];
                // `export const a = 1, b = 2` declares both
                if kind == "const" {
                    let shape_t = shape.trim();
                    names.extend(extra.captures_iter(shape_t).map(|c| c[1].to_string()));
                }
                for name in names {
                    inventory.push(Item {
                        vis: if exported { "pub" } else { "priv" }.into(),
                        kind: kind.into(),
                        name,
                        text: t.trim_end_matches('{').trim().to_string(),
                    });
                }
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
            } else if t.starts_with("export *") {
                signatures.push(t.to_string());
                reexports.push("*".into());
            }
        }
        depth += shape.matches('{').count() as i32 - shape.matches('}').count() as i32;
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
        // `NAME` is a named group (0.6.5): a plugin's own groups no longer
        // shift which text is taken as the name
        let (Ok(marker), Ok(call)) = (
            Regex::new(&b.rust),
            Regex::new(
                &b.ts
                    .replace("NAME", "(?P<harna_name>[A-Za-z_$][A-Za-z0-9_$]*)"),
            ),
        ) else {
            continue; // admission names a broken bridge
        };
        // exposed name -> Rust file. The marker runs over the file's live
        // code (comments and tests masked); a `(?P<name>…)` group in it
        // names the fn directly (deno: `#\[op2[^\n]*\]\s*\n…fn\s+(?P<name>op_\w+)`),
        // else the first `fn` within six lines after it does.
        let mut exposed: std::collections::HashMap<String, String> = Default::default();
        for m in digest.modules.iter().filter(|m| m.lang == "rust") {
            let Ok(src) = std::fs::read_to_string(root.join(&m.path)) else {
                continue;
            };
            let live: Vec<&str> = super::live::live_code(&src)
                .into_iter()
                .map(|(_, l)| l)
                .collect();
            let text = live.join("\n");
            for cap in marker.captures_iter(&text) {
                let name = match cap.name("name") {
                    Some(n) => Some(n.as_str().to_string()),
                    None => {
                        let after = &text[cap.get(0).map_or(0, |m| m.start())..];
                        after
                            .lines()
                            .take(6)
                            .find_map(|x| fn_name.captures(x))
                            .map(|c| c[1].to_string())
                    }
                };
                if let Some(name) = name {
                    exposed.entry(name).or_insert(m.path.clone());
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
            // calls in comments do not count
            let code = lex(&src).code.join("\n");
            for cap in call.captures_iter(&code) {
                let Some(n) = cap.name("harna_name") else {
                    continue;
                };
                if let Some(target) = exposed.get(n.as_str()) {
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
mod tests;
