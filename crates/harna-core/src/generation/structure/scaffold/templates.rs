//! Component `scaffold-templates` (0.4.10): the starter text scaffold
//! writes — split from `scaffold.rs` so the generator and its words
//! change apart. `--empty` writes the doc header alone.

/// `--empty` (0.4.10 field test: the TODO stub was deleted every time):
/// the doc header alone, for code the author writes themself.
pub(super) fn empty_template(name: &str, description: &str, scope: &str) -> String {
    let desc = if description.is_empty() {
        "(describe its one responsibility)"
    } else {
        description
    };
    format!(
        "//! `{name}` — {desc}\n//!\n//! Scope: `{scope}`. Rules that apply here live in `.harna/rules.toml`.\n"
    )
}

pub(super) fn pascal(name: &str) -> String {
    name.split(['-', '_'])
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut cs = s.chars();
            match cs.next() {
                Some(first) => first.to_uppercase().collect::<String>() + cs.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

pub(super) fn primitive_template(name: &str, description: &str, scope: &str) -> String {
    let struct_name = pascal(name);
    format!(
        "//! `{name}` — primitive component.
//!
//! {desc}
//! Scope: `{scope}`. Rules that apply here live in `.harna/rules.toml`
//! (query them via the harna tools instead of assuming).

#[derive(Debug, Clone)]
pub struct {struct_name} {{
    // its fields
}}

impl {struct_name} {{
    /// Build it; the one responsibility follows as methods.
    pub fn new() -> Self {{
        Self {{ }}
    }}
}}

impl Default for {struct_name} {{
    fn default() -> Self {{
        Self::new()
    }}
}}
",
        name = name,
        desc = if description.is_empty() {
            "(say its one responsibility here)"
        } else {
            description
        },
        scope = scope,
        struct_name = struct_name,
    )
}

pub(super) fn compound_mod_template(name: &str, child_name: &str, description: &str) -> String {
    let child_mod = child_name.replace('-', "_");
    format!(
        "//! `{name}` — compound component.
//!
//! {desc}
//! Composition of primitive/compound children; children are declared here
//! and registered in `.harna/manifest.toml`.

pub mod {child_mod};
",
        name = name,
        desc = if description.is_empty() {
            "(say what this composition provides here)"
        } else {
            description
        },
        child_mod = child_mod,
    )
}

/// The data file is the single source of truth for a value-set
/// (constants-are-data). Granularity note included by design: value-sets
/// may be split into further data files as they diverge.
pub(super) fn data_toml_template(name: &str, description: &str) -> String {
    format!(
        "# `{name}` — data source (single point of truth; constants-are-data).\n\
# {desc}\n\
# Keep code reading this file through `{name}-loader.rs`; when the set\n\
# diverges, split into further data files and let loaders compose them.\n\n\
[[entries]]\n\
key = \"example-entry\"\n\
value = \"replace-me\"\n",
        name = name,
        desc = if description.is_empty() {
            "(say what facts this value-set holds here)"
        } else {
            description
        },
    )
}

/// The loader is the one import consumers get; it owns the parsing stack so
/// callers stay data-source-agnostic.
pub(super) fn data_loader_template(
    name: &str,
    loader_name: &str,
    data_file: &str,
    loader_file: &str,
) -> String {
    let entry_struct = pascal(name);
    // 0.3.50 (household field test): the wiring compiles as pasted — a
    // snake_case fn, the data file named relative to the loader (they are
    // siblings), a Result instead of a panic, and the crates it needs named.
    let fn_name = super::super::linkage::module_name(name);
    // the data file relative to the loader: siblings, or a data folder
    // elsewhere in the repository (1.0.1)
    let file_name = relative_to(loader_file, data_file);
    format!(
        "//! `{loader_name}` — the single loader for the `{name}` data source ({data_file}).\n\
//!\n\
//! All consumers of `{name}` import this loader and nothing else; the toml\n\
//! file is the point of truth and stays code-free. Recommended wiring\n\
//! (needs `serde` with the `derive` feature and `toml` in this crate):\n\
//!\n\
//! ```ignore\n\
//! #[derive(Debug, Clone, serde::Deserialize)]\n\
//! pub struct {entry_struct} {{\n\
//!     pub key: String,\n\
//!     pub value: String,\n\
//! }}\n\
//!\n\
//! #[derive(serde::Deserialize)]\n\
//! struct File {{\n\
//!     entries: Vec<{entry_struct}>,\n\
//! }}\n\
//!\n\
//! pub fn load_{fn_name}() -> Result<Vec<{entry_struct}>, toml::de::Error> {{\n\
//!     const RAW: &str = include_str!(\"{file_name}\");\n\
//!     toml::from_str::<File>(RAW).map(|f| f.entries)\n\
//! }}\n\
//! ```\n\
//!\n\
//! Paste the wiring above into this file, then delete this line.\n\
//! Registered in `.harna/manifest.toml` as an owned component of `{name}`.\n"
    )
}

/// `target` as a path relative to the folder holding `from` (both
/// repository-relative).
fn relative_to(from: &str, target: &str) -> String {
    let from_dir: Vec<&str> = from.split('/').collect();
    let from_dir = &from_dir[..from_dir.len().saturating_sub(1)];
    let to: Vec<&str> = target.split('/').collect();
    let common = from_dir.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<&str> = vec![".."; from_dir.len() - common];
    parts.extend(&to[common..]);
    parts.join("/")
}
