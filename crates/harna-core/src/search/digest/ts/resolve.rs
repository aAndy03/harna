//! Specifiers to files and packages (split from ts.rs, 1.1.8): relative
//! paths, tsconfig `paths` and workspace packages.

use std::path::Path;

use regex::Regex;

use super::EXTENSIONS;

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
        let files: Vec<String> = super::super::respected_files_named(root, &["package.json"])
            .into_iter()
            .filter(|p| !p.split('/').any(|s| s == "node_modules"))
            .collect();
        // workspace manifests read on all cores (0.9.1: 500 read one by one)
        let name_of = |f: &String| -> Option<(String, String)> {
            let pkg = read_jsonc(&root.join(f))?;
            let name = pkg.get("name").and_then(|n| n.as_str())?.to_string();
            let dir = Path::new(f)
                .parent()
                .map(|d| d.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            Some((name, dir))
        };
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(files.len().div_ceil(16).max(1));
        if threads <= 1 {
            ctx.packages.extend(files.iter().filter_map(name_of));
        } else {
            let chunk = files.len().div_ceil(threads);
            let name_of = &name_of;
            let found: Vec<(String, String)> = std::thread::scope(|sc| {
                let handles: Vec<_> = files
                    .chunks(chunk)
                    .map(|c| sc.spawn(move || c.iter().filter_map(name_of).collect::<Vec<_>>()))
                    .collect();
                handles
                    .into_iter()
                    .flat_map(|h| h.join().unwrap_or_default())
                    .collect()
            });
            ctx.packages.extend(found);
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
