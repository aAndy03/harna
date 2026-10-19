//! Component `whereis`: the symbol-level locator.
//!
//! "Where does this symbol live, and who reaches it?" — answered straight
//! from the digest inventory (pub *and* private declarations), not by
//! rescanning files. Every declared item is locatable, even below the
//! public surface; the reverse edge (referencing modules) comes from the
//! dependents query so the answer is one graph view, consistently resolved.

use serde::Serialize;

use crate::schema::store::Store;
use crate::search::digest::Module;

/// How many definitions a busy name may render before truncation.
const MAX_DEFS: usize = 20;

#[derive(Debug, Clone, Serialize)]
pub struct WhereIsReport {
    pub symbol: String,
    /// Exact declaration matches: the file, the declaration text and the
    /// item's visibility/kind from the inventory.
    pub definitions: Vec<SymbolDef>,
    /// Substring matches, when nothing exact matched (spelling rescue).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub near_matches: Vec<SymbolDef>,
    /// Referencing modules per definition (dependents ring 1, deduped).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub referenced_by: Vec<String>,
    /// True when matched only by substring, not by exact name.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fuzzy: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SymbolDef {
    pub file: String,
    /// The declaration line as written, e.g. "pub struct Bindings".
    pub declaration: String,
    /// fn | struct | enum | trait | const | static | type | mod | impl
    pub kind: String,
    /// "pub" (any cross-module Rust visibility) | "priv".
    pub visibility: String,
}

impl WhereIsReport {
    pub fn render(&self) -> String {
        if self.definitions.is_empty() && self.near_matches.is_empty() {
            return format!(
                "whereis `{}`: no symbol matches. Spell it as declared (Rust names are case-sensitive), \
                 qualify it (`module::symbol`), or run `harna scan` if the map is stale",
                self.symbol
            );
        }
        let mut out = String::new();
        if self.definitions.is_empty() {
            out.push_str(&format!(
                "whereis `{}`: no exact match — name-containing symbols:\n",
                self.symbol
            ));
            self.render_defs(&self.near_matches, &mut out);
        } else {
            out.push_str(&format!(
                "whereis `{}`: {} definition(s)\n",
                self.symbol,
                self.definitions.len()
            ));
            self.render_defs(&self.definitions, &mut out);
            if !self.referenced_by.is_empty() {
                out.push_str(&format!(
                    "referenced by {} module(s): {}\n",
                    self.referenced_by.len(),
                    self.referenced_by.join(", ")
                ));
            }
        }
        out
    }

    fn render_defs(&self, defs: &[SymbolDef], out: &mut String) {
        for d in defs {
            out.push_str(&format!(
                "  - {} — {} (`{}`, {})\n",
                d.file, d.declaration, d.kind, d.visibility
            ));
        }
        let hidden = defs.len().saturating_sub(MAX_DEFS);
        if hidden > 0 {
            out.push_str(&format!(
                "  … and {hidden} more matches (narrow with `module::symbol`)\n"
            ));
        }
    }
}

/// Locate a symbol across the digest inventory (pub + private).
///
/// The query may be:
/// - a bare symbol (`Bindings`) — matched exactly against declared names;
/// - module-qualified (`schema::Manifest`) — the part before the last `::`
///   filters the defining module's path;
/// - suffixed with `()` — stripped; Rust names are the identifier only.
///
/// When nothing matches exactly, name-containing symbols are offered
/// (capped) so a typo'd or under-remembered name still lands somewhere.
///
/// Bounded (0.3.16): shards stream one container at a time — the pass
/// keeps matching declarations only, never the whole map. Up to three
/// passes (exact, case rescue, substring rescue), each a fresh stream;
/// memory stays proportional to the answer, not the codebase.
pub fn build(store: &Store, query: &str) -> anyhow::Result<WhereIsReport> {
    let (module_filter, symbol) = normalize(query);
    anyhow::ensure!(
        !symbol.is_empty(),
        "empty symbol — pass a declared name, optionally module-qualified"
    );

    let in_scope = |m: &Module| match &module_filter {
        None => true,
        Some(mf) => {
            let mf_norm = mf.replace('/', "::");
            let stem = m.crate_of().1.replace('/', "::");
            stem == mf_norm
                || stem.starts_with(&format!("{mf_norm}::"))
                || stem.ends_with(&format!("::{mf_norm}"))
                || stem.contains(&format!("::{mf_norm}::"))
        }
    };

    // Pass 1: exact declaration matches.
    let mut exact: Vec<SymbolDef> = Vec::new();
    let found = store.for_each_shard(|_, shard| {
        for m in &shard.modules {
            if !in_scope(m) {
                continue;
            }
            for item in &m.inventory {
                if item.name == symbol {
                    exact.push(SymbolDef {
                        file: m.path.clone(),
                        declaration: item.text.clone(),
                        kind: item.kind.clone(),
                        visibility: item.vis.clone(),
                    });
                }
            }
        }
        true
    })?;
    if !found {
        anyhow::bail!("no code map — run `harna scan` first; whereis reads the map, not the files");
    }

    // Case rescue, then substring rescue — a typo'd or half-remembered name
    // still has to land somewhere.
    let mut fuzzy = false;
    if exact.is_empty() {
        store.for_each_shard(|_, shard| {
            for m in &shard.modules {
                if !in_scope(m) {
                    continue;
                }
                for item in &m.inventory {
                    if item.name.eq_ignore_ascii_case(&symbol) {
                        exact.push(SymbolDef {
                            file: m.path.clone(),
                            declaration: item.text.clone(),
                            kind: item.kind.clone(),
                            visibility: item.vis.clone(),
                        });
                    }
                }
            }
            true
        })?;
        fuzzy = !exact.is_empty();
    }

    let near_matches: Vec<SymbolDef> = if exact.is_empty() {
        fuzzy = true;
        let mut defs: Vec<SymbolDef> = Vec::new();
        store.for_each_shard(|_, shard| {
            for m in &shard.modules {
                let in_near_scope = module_filter.as_deref().is_none_or(|mf| {
                    m.path.contains(mf) || m.path.contains(&mf.replace('/', "::"))
                });
                if !in_near_scope {
                    continue;
                }
                let low = symbol.to_lowercase();
                for item in &m.inventory {
                    if item.name.to_lowercase().contains(&low) {
                        defs.push(SymbolDef {
                            file: m.path.clone(),
                            declaration: item.text.clone(),
                            kind: item.kind.clone(),
                            visibility: item.vis.clone(),
                        });
                    }
                    if defs.len() >= MAX_DEFS {
                        return false;
                    }
                }
            }
            true
        })?;
        defs
    } else {
        Vec::new()
    };

    // Where does the outside reach in? Ring-1 importers of the defining
    // modules (deduped across shared homes).
    let mut referenced_by: Vec<String> = Vec::new();
    if !fuzzy && !exact.is_empty() {
        for def in exact.iter().take(MAX_DEFS) {
            if let Ok(report) = crate::search::dependents::build(store, &def.file) {
                for edge in report.direct {
                    if !referenced_by.contains(&edge.importer) {
                        referenced_by.push(edge.importer);
                    }
                }
            }
        }
        referenced_by.sort();
    }

    Ok(WhereIsReport {
        symbol,
        definitions: exact,
        near_matches,
        referenced_by,
        fuzzy,
    })
}

/// `schema::Manifest` -> (Some("schema"), "Manifest"); `fn go()` -> (None, "go").
fn normalize(query: &str) -> (Option<String>, String) {
    let q = query.trim().trim_end_matches("()").trim();
    match q.rsplit_once("::") {
        Some((mod_path, sym)) => (Some(mod_path.trim().to_string()), sym.trim().to_string()),
        None => (None, q.trim().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::digest::{Digest, Item};

    fn fixture_store(tag: &str) -> Store {
        let tmp = std::env::temp_dir().join(format!("harna-whereis-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(tmp.join(".harna"));
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        let digest = Digest {
            orphans: Vec::new(),
            generated_at: "now".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![
                Module {
                    reexports: Vec::new(),
                    crate_dir: None,
                    src_root: None,
                    path: "crates/t/src/schema/store.rs".into(),
                    lang: "rust".into(),
                    signatures: vec![],
                    deps: vec![],
                    body_hash: "x".into(),
                    inventory: vec![
                        Item {
                            vis: "pub".into(),
                            kind: "struct".into(),
                            name: "StoreSchema".into(),
                            text: "pub struct StoreSchema".into(),
                        },
                        Item {
                            vis: "priv".into(),
                            kind: "fn".into(),
                            name: "seal".into(),
                            text: "fn seal() {}".into(),
                        },
                    ],
                    mods: vec![],
                },
                Module {
                    reexports: Vec::new(),
                    crate_dir: None,
                    src_root: None,
                    path: "crates/t/src/api.rs".into(),
                    lang: "rust".into(),
                    signatures: vec![],
                    deps: vec!["crate::schema::store".into()],
                    body_hash: "x".into(),
                    inventory: vec![Item {
                        vis: "pub".into(),
                        kind: "fn".into(),
                        name: "seal_entry".into(),
                        text: "pub fn seal_entry()".into(),
                    }],
                    mods: vec![],
                },
            ],
        };
        store
            .save_digest(&store.load_manifest().unwrap_or_default(), &digest)
            .unwrap();
        store
    }

    #[test]
    fn finds_definitions_pub_and_private() {
        let store = fixture_store("defs");
        let r = build(&store, "StoreSchema").unwrap();
        assert_eq!(r.definitions.len(), 1);
        assert_eq!(r.definitions[0].file, "crates/t/src/schema/store.rs");
        assert_eq!(r.definitions[0].visibility, "pub");
        assert!(!r.fuzzy);

        // Private symbols are locatable too — inventory is the full map.
        let r = build(&store, "seal").unwrap();
        assert_eq!(r.definitions.len(), 1);
        assert_eq!(r.definitions[0].visibility, "priv");
    }

    #[test]
    fn referencing_modules_come_from_the_import_graph() {
        let store = fixture_store("refs");
        let r = build(&store, "StoreSchema").unwrap();
        assert!(
            r.referenced_by.iter().any(|m| m.ends_with("api.rs")),
            "api.rs imports the defining module — report was {:#?}",
            r.referenced_by
        );
    }

    #[test]
    fn qualified_queries_filter_by_module() {
        let store = fixture_store("qualified");
        // Different modules both know a name containing `seal`; only the
        // qualified one counts.
        let r = build(&store, "structure::seal").unwrap();
        assert_eq!(r.definitions.len(), 0, "no structure module here");
        let r = build(&store, "schema::StoreSchema").unwrap();
        assert_eq!(r.definitions.len(), 1);

        // Trailing `()` is decoration, not the name.
        let r = build(&store, "seal()").unwrap();
        assert_eq!(r.definitions.len(), 1);
    }

    #[test]
    fn miss_falls_back_to_name_contains() {
        let store = fixture_store("fuzzy");
        let r = build(&store, "Store").unwrap(); // exact name is StoreSchema
        assert!(r.fuzzy);
        assert!(r.definitions.is_empty());
        assert!(
            r.near_matches.iter().any(|d| d.file.ends_with("store.rs")),
            "spelling rescue found the near name"
        );
        let text = r.render();
        assert!(text.contains("no exact match"));
    }

    #[test]
    fn stencil_unknown_reports_honestly_and_empty_map_is_refused() {
        let store = fixture_store("stencil");
        let r = build(&store, "no_such_symbol_anywhere").unwrap();
        assert!(r.definitions.is_empty() && r.near_matches.is_empty());
        assert!(r.render().contains("no symbol matches"));

        let empty =
            std::env::temp_dir().join(format!("harna-whereis-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(empty.join(".harna"));
        let store2 = Store::open(&empty).unwrap();
        store2.init().unwrap();
        assert!(
            build(&store2, "go").is_err(),
            "no map -> refused with the scan hint"
        );
    }
}
