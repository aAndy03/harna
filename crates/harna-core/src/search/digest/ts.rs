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

/// A TS/JS source read as code (0.6.5): `code` has comments removed and
/// template-literal text blanked (an `import … from` inside a template is
/// not an import); `shape` also blanks every string, template and regex
/// literal, so brace counting never trips on `'{'` or `"api/*"`. Both keep
/// the source's line structure.
pub struct Lexed {
    pub code: Vec<String>,
    pub shape: Vec<String>,
}

pub fn lex(src: &str) -> Lexed {
    #[derive(Clone, Copy, PartialEq)]
    enum S {
        Code,
        Line,
        Block,
        Str(char),
        Tmpl,
        /// inside a template's `${…}`: its brace depth (blanked, 0.7.7)
        TmplExpr(u32),
        Regex(bool), // inside a character class?
    }
    // after these words a `/` starts a regex, not a division (0.7.7)
    const REGEX_AFTER: &[&str] = &[
        "return",
        "typeof",
        "case",
        "in",
        "of",
        "delete",
        "void",
        "throw",
        "new",
        "else",
        "yield",
        "await",
        "instanceof",
        "do",
    ];
    let mut word = String::new();
    let chars: Vec<char> = src.chars().collect();
    let mut code = String::with_capacity(src.len());
    let mut shape = String::with_capacity(src.len());
    let mut s = S::Code;
    // the last significant code char: a `/` after one of these opens a regex
    let mut prev = '\n';
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied().unwrap_or('\0');
        if c == '\n' {
            code.push('\n');
            shape.push('\n');
            if matches!(s, S::Line) {
                s = S::Code;
            }
            if matches!(s, S::Str(_) | S::Regex(_)) {
                s = S::Code; // an unterminated literal ends at the line
            }
            i += 1;
            continue;
        }
        match s {
            S::Code => {
                if c == '/' && next == '/' {
                    s = S::Line;
                    i += 2;
                    continue;
                }
                if c == '/' && next == '*' {
                    s = S::Block;
                    i += 2;
                    continue;
                }
                // JSX: `</p>` closes a tag and `/>` ends one (0.7.7)
                let jsx = c == '/' && (prev == '<' || next == '>');
                let after_word = prev.is_alphanumeric() || prev == '_' || prev == '$';
                if c == '/'
                    && !jsx
                    && ("(,=:[!&|?{};+-*%<>~^\n".contains(prev)
                        || (after_word && REGEX_AFTER.contains(&word.as_str())))
                {
                    s = S::Regex(false);
                    code.push(c);
                    shape.push(' ');
                    i += 1;
                    continue;
                }
                if c == '\'' || c == '"' {
                    s = S::Str(c);
                    code.push(c);
                    shape.push(c);
                    i += 1;
                    continue;
                }
                if c == '`' {
                    s = S::Tmpl;
                    code.push(c);
                    shape.push(c);
                    i += 1;
                    continue;
                }
                code.push(c);
                shape.push(c);
                if c.is_alphanumeric() || c == '_' || c == '$' {
                    if !(prev.is_alphanumeric() || prev == '_' || prev == '$') {
                        word.clear();
                    }
                    word.push(c);
                }
                if !c.is_whitespace() {
                    prev = c;
                }
            }
            S::Line => {}
            S::Block => {
                if c == '*' && next == '/' {
                    s = S::Code;
                    i += 2;
                    code.push(' ');
                    shape.push(' ');
                    continue;
                }
            }
            S::Str(q) => {
                if c == '\\' {
                    code.push(c);
                    if next != '\n' && next != '\0' {
                        code.push(next);
                        shape.push(' ');
                        i += 1;
                    }
                    shape.push(' ');
                    i += 1;
                    continue;
                }
                code.push(c);
                if c == q {
                    shape.push(c);
                    s = S::Code;
                    prev = 'a'; // a string is an operand
                } else {
                    shape.push(' ');
                }
            }
            S::Tmpl => {
                if c == '\\' {
                    code.push(' ');
                    shape.push(' ');
                    if next != '\n' && next != '\0' {
                        code.push(' ');
                        shape.push(' ');
                        i += 1;
                    }
                    i += 1;
                    continue;
                }
                if c == '$' && next == '{' {
                    code.push(' ');
                    shape.push(' ');
                    code.push(' ');
                    shape.push(' ');
                    s = S::TmplExpr(1);
                    i += 2;
                    continue;
                }
                if c == '`' {
                    code.push(c);
                    shape.push(c);
                    s = S::Code;
                    prev = 'a';
                } else {
                    code.push(' ');
                    shape.push(' ');
                }
            }
            S::TmplExpr(depth) => {
                code.push(' ');
                shape.push(' ');
                match c {
                    '{' => s = S::TmplExpr(depth + 1),
                    '}' if depth == 1 => s = S::Tmpl,
                    '}' => s = S::TmplExpr(depth - 1),
                    _ => {}
                }
            }
            S::Regex(in_class) => {
                code.push(c);
                shape.push(' ');
                if c == '\\' {
                    if next != '\n' && next != '\0' {
                        code.push(next);
                        shape.push(' ');
                        i += 1;
                    }
                } else if c == '[' {
                    s = S::Regex(true);
                } else if c == ']' && in_class {
                    s = S::Regex(false);
                } else if c == '/' && !in_class {
                    s = S::Code;
                    prev = 'a';
                }
            }
        }
        i += 1;
    }
    Lexed {
        code: code.split('\n').map(str::to_string).collect(),
        shape: shape.split('\n').map(str::to_string).collect(),
    }
}

/// Comment-stripped lines (string-aware since 0.6.5: `"api/*"` or an URL
/// in a string no longer opens a comment).
pub fn code_lines(src: &str) -> Vec<String> {
    lex(src).code
}

/// The repository file a relative specifier means, if it exists.
pub fn resolve(root: &Path, importer: &str, spec: &str) -> Option<String> {
    let dir = Path::new(importer).parent().unwrap_or(Path::new(""));
    // a specifier climbing past the repository root means nothing here
    // (0.6.5: `../../../x` used to land on the root's `x`)
    let joined = normalize_within(&dir.join(spec).to_string_lossy().replace('\\', "/"))?;
    // the exact on-disk name: Windows answers `Button.tsx` for `button.tsx`
    // (0.6.5), which then dangled against the map's own path
    let exists = |p: &str| exact_case(root, p);
    if Path::new(&joined).extension().is_some() && exists(&joined) {
        return Some(joined);
    }
    // TypeScript's ESM convention: `./x.js` written for `x.ts` (and the
    // `.mjs`/`.cjs` pair for `.mts`/`.cts`)
    for (written, real) in [
        (".js", &["ts", "tsx"][..]),
        (".jsx", &["tsx"][..]),
        (".mjs", &["mts"][..]),
        (".cjs", &["cts"][..]),
    ] {
        if let Some(stem) = joined.strip_suffix(written) {
            for e in real {
                let c = format!("{stem}.{e}");
                if exists(&c) {
                    return Some(c);
                }
            }
        }
    }
    for e in EXTENSIONS.iter().chain(&["d.ts"]) {
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

/// Whether `rel` names a file under `root` with exactly that casing.
fn exact_case(root: &Path, rel: &str) -> bool {
    let full = root.join(rel);
    if !full.is_file() {
        return false;
    }
    let (Some(parent), Some(name)) = (full.parent(), full.file_name()) else {
        return false;
    };
    std::fs::read_dir(parent)
        .map(|rd| rd.flatten().any(|e| e.file_name() == name))
        .unwrap_or(false)
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

/// Like `normalize`, but None when `..` climbs above the start.
fn normalize_within(p: &str) -> Option<String> {
    let mut out: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            s => out.push(s),
        }
    }
    Some(out.join("/"))
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
        // the root tsconfig, what it extends and the projects it references
        // (0.6.5: Vite keeps `paths` in tsconfig.app.json)
        let mut queue = vec!["tsconfig.json".to_string()];
        let mut seen = std::collections::BTreeSet::new();
        while let Some(rel) = queue.pop() {
            if !seen.insert(rel.clone()) || seen.len() > 16 {
                continue;
            }
            let Some(cfg) = read_jsonc(&root.join(&rel)) else {
                continue;
            };
            let dir = Path::new(&rel)
                .parent()
                .map(|d| d.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let at = |p: &str| normalize(&format!("{dir}/{p}"));
            let config_file = |p: String| {
                if p.ends_with(".json") {
                    p
                } else {
                    format!("{p}/tsconfig.json")
                }
            };
            if let Some(e) = cfg.get("extends").and_then(|e| e.as_str()) {
                if e.starts_with('.') {
                    queue.push(config_file(at(e)));
                }
            }
            for r in cfg
                .get("references")
                .and_then(|r| r.as_array())
                .into_iter()
                .flatten()
            {
                if let Some(p) = r.get("path").and_then(|p| p.as_str()) {
                    queue.push(config_file(at(p)));
                }
            }
            let opts = cfg.get("compilerOptions");
            let base = opts
                .and_then(|o| o.get("baseUrl"))
                .and_then(|b| b.as_str())
                .unwrap_or(".");
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
                                .map(|t| at(&format!("{base}/{t}")))
                                .collect()
                        })
                        .unwrap_or_default();
                    if !ctx.aliases.iter().any(|(a, _)| a == alias) {
                        ctx.aliases.push((alias.clone(), targets));
                    }
                }
            }
        }
        // the most specific alias first: `@/components/*` before `@/*`,
        // and a bare `*` last
        ctx.aliases
            .sort_by_key(|(a, _)| std::cmp::Reverse(a.trim_end_matches('*').len()));
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
    static TRAILING: std::sync::OnceLock<Option<Regex>> = std::sync::OnceLock::new();
    let no_trailing = TRAILING
        .get_or_init(|| Regex::new(r",(\s*[}\]])").ok())
        .as_ref()?
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
    // a tsconfig alias: resolved against the repository root; an alias
    // that matches but resolves nothing lets the next one try (0.6.5), and
    // a catch-all `*` never turns a package into an unresolved file
    let mut matched = false;
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
        matched |= !prefix.is_empty();
    }
    if matched {
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
    // a URL import (deno, esm.sh) is external, never a package name (0.6.5);
    // `npm:x` / `jsr:@s/x` specifiers name their package
    if let Some(rest) = spec.strip_prefix("npm:") {
        // `npm:@scope/pkg@1.2/sub` -> `@scope/pkg`
        let pkg = package_of(rest);
        let pkg = match pkg.rsplit_once('@') {
            Some((name, _)) if !name.is_empty() => name.to_string(),
            _ => pkg,
        };
        return format!("npm:{pkg}");
    }
    // any other scheme (`https://`, `jsr:`, deno's `ext:deno_node/x.ts`)
    // is external and keeps its whole specifier; `node:` builtins stay
    // packages (0.6.5: `ext:` collapsed into one fake package)
    let scheme = spec.split_once(':').is_some_and(|(s, _)| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-')
    });
    if scheme && !spec.starts_with("node:") {
        return format!("url:{spec}");
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
}
