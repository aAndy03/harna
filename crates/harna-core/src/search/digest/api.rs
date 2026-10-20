//! Component `api-surface` (0.5.2): a crate's public surface, read from
//! its source — the module paths a consumer can name and the items each
//! one makes public (declared `pub` items and `pub use` re-exports).
//!
//! It feeds a plugin's `api.toml` (gpui-kit 0.8.0), so "never invent an
//! API" becomes checkable offline: the surface is generated once from the
//! released source and re-generated on upgrade, never fetched at the gate.
//! A glob re-export of a sibling module (`pub use button::*`) is followed;
//! one into another crate makes the module `open`: its names cannot be
//! listed without that crate, so the check trusts any name there.

use std::path::{Path, PathBuf};

pub use crate::schema::store::plugins::ApiModule;

use super::Item;

/// The public modules reachable from `src/lib.rs` under `src_dir`,
/// prefixed with `prefix` (`component` for a crate the facade mounts as
/// `gpui_kit::component`; "" for the facade's own root).
pub fn surface(src_dir: &Path, prefix: &str) -> Vec<ApiModule> {
    let mut out = Vec::new();
    walk(&src_dir.join("lib.rs"), prefix, &mut out);
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// What one module file declares: its public names, its `mod` children
/// (and which are `pub`), and the bases of its glob re-exports.
#[derive(Default)]
struct Scan {
    items: Vec<String>,
    open: bool,
    mods: Vec<String>,
    pub_mods: Vec<String>,
    globs: Vec<String>,
}

fn dir_of(file: &Path) -> PathBuf {
    // where `mod x;` files live: beside lib.rs/mod.rs, or under foo/ for foo.rs
    match file.file_name().and_then(|n| n.to_str()) {
        Some("lib.rs" | "mod.rs") => file.parent().map(Path::to_path_buf).unwrap_or_default(),
        _ => file.with_extension(""),
    }
}

fn child_file(file: &Path, child: &str) -> PathBuf {
    let dir = dir_of(file);
    let flat = dir.join(format!("{child}.rs"));
    if flat.is_file() {
        flat
    } else {
        dir.join(child).join("mod.rs")
    }
}

fn scan(src: &str) -> Scan {
    let mut s = Scan::default();
    let mut pending_use = String::new();
    let take = |stmt: &str, s: &mut Scan| {
        let body = stmt
            .trim()
            .trim_start_matches("pub use ")
            .trim_end_matches(';')
            .trim();
        if let Some(base) = body.strip_suffix("::*") {
            s.globs.push(base.trim_start_matches("self::").to_string());
        } else {
            take_use(stmt, &mut s.items, &mut s.open);
        }
    };
    for (_, line) in super::live::live_code(src) {
        if !pending_use.is_empty() {
            pending_use.push(' ');
            pending_use.push_str(line);
            if line.contains(';') {
                let stmt = std::mem::take(&mut pending_use);
                take(&stmt, &mut s);
            }
            continue;
        }
        if line.starts_with("pub use ") {
            if line.contains(';') {
                take(line, &mut s);
            } else {
                pending_use = line.to_string();
            }
            continue;
        }
        let Some(item) = Item::parse(line) else {
            continue;
        };
        if item.kind == "mod" && line.trim_end().ends_with(';') {
            s.mods.push(item.name.clone());
            if item.vis == "pub" {
                s.pub_mods.push(item.name.clone());
            }
        }
        if item.vis == "pub" {
            s.items.push(item.name);
        }
    }
    // exported macros (live_code masks their bodies: read the raw lines)
    let mut export = false;
    for line in src.lines().map(str::trim) {
        if line.starts_with("#[macro_export]") {
            export = true;
        } else if let Some(rest) = line.strip_prefix("macro_rules!") {
            let name: String = rest
                .trim()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if export && !name.is_empty() {
                s.items.push(name);
            }
            export = false;
        } else if !line.starts_with("#[") && !line.starts_with("//") && !line.is_empty() {
            export = false;
        }
    }
    s
}

/// A module's names with its local glob re-exports followed: `pub use
/// button::*` over a sibling `mod button;` brings that file's names (0.5.2:
/// gpui-component re-exports every widget that way). A glob into another
/// crate, or `super::`/`crate::`, stays open.
fn names(file: &Path, depth: usize) -> (Vec<String>, bool, Scan) {
    let Ok(src) = std::fs::read_to_string(file) else {
        return (Vec::new(), true, Scan::default());
    };
    let s = scan(&src);
    let mut items = s.items.clone();
    let mut open = s.open;
    for g in &s.globs {
        let first = g.split("::").next().unwrap_or("");
        if depth < 8 && !g.contains("::") && s.mods.iter().any(|m| m == first) {
            let (more, o, _) = names(&child_file(file, first), depth + 1);
            items.extend(more);
            open |= o;
        } else {
            open = true;
        }
    }
    (items, open, s)
}

fn walk(file: &Path, path: &str, out: &mut Vec<ApiModule>) {
    if !file.is_file() {
        return;
    }
    let (mut items, open, s) = names(file, 0);
    items.sort();
    items.dedup();
    out.push(ApiModule {
        path: path.to_string(),
        items,
        open,
    });
    for child in &s.pub_mods {
        let child_path = if path.is_empty() {
            child.clone()
        } else {
            format!("{path}::{child}")
        };
        walk(&child_file(file, child), &child_path, out);
    }
}

/// A `pub use` statement's names (brace groups, aliases).
fn take_use(stmt: &str, items: &mut Vec<String>, open: &mut bool) {
    for n in super::extract::reexported_names(stmt.trim()) {
        if n == "*" {
            *open = true;
        } else {
            items.push(n);
        }
    }
    // nested groups (`a::{b::{C, D}}`) are beyond one level: trust them
    if stmt.matches('{').count() > 1 {
        *open = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0.5.2: modules follow `pub mod`, items are pub declarations and
    /// re-exports (multi-line groups too), a glob opens the module, and a
    /// private module's items stay out unless re-exported.
    #[test]
    fn the_surface_follows_pub_mods_and_reexports() {
        let root = std::env::temp_dir().join(format!("harna-api-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let w = |rel: &str, body: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        w(
            "src/lib.rs",
            "pub mod button;\nmod hidden;\npub use hidden::{\n    Shown,\n    Also as Renamed,\n};\npub use theme::*;\n#[macro_export]\nmacro_rules! actions { () => {} }\n",
        );
        w(
            "src/button.rs",
            "pub struct Button;\npub fn helper() {}\nfn private() {}\npub mod variants;\n",
        );
        w("src/button/variants.rs", "pub enum ButtonVariant { A }\n");
        w(
            "src/hidden.rs",
            "pub struct Shown;\npub struct Also;\npub struct NotReexported;\n",
        );
        let s = surface(&root.join("src"), "component");
        let paths: Vec<&str> = s.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "component",
                "component::button",
                "component::button::variants"
            ]
        );
        let rootm = &s[0];
        assert!(rootm.open);
        assert!(
            rootm.items.contains(&"Shown".to_string())
                && rootm.items.contains(&"Renamed".to_string())
        );
        assert!(rootm.items.contains(&"actions".to_string()));
        assert!(!rootm.items.contains(&"NotReexported".to_string()));
        assert_eq!(s[1].items, vec!["Button", "helper", "variants"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// gpui-component re-exports each widget from a private sibling:
    /// the glob is followed, so the module is closed and complete.
    #[test]
    fn local_globs_are_followed() {
        let root = std::env::temp_dir().join(format!("harna-api-glob-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let w = |rel: &str, body: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        w(
            "src/lib.rs",
            "pub mod button;
",
        );
        w(
            "src/button/mod.rs",
            "mod button;
mod group;
pub use button::*;
pub use self::group::*;
",
        );
        w(
            "src/button/button.rs",
            "pub struct Button;
",
        );
        w(
            "src/button/group.rs",
            "pub struct ButtonGroup;
struct Hidden;
",
        );
        let s = surface(&root.join("src"), "");
        let b = s.iter().find(|m| m.path == "button").unwrap();
        assert!(!b.open);
        assert_eq!(b.items, vec!["Button", "ButtonGroup"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
