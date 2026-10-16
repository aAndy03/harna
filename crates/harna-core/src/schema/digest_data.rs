//! Component `digest-data` (0.3.9): the digest's DATA structs live in the
//! memory layer — `schema/store.rs` persists them, and data owned by the
//! store belongs next to it. The parsing/query logic stays in
//! `search/digest.rs` (search may reach memory; memory may not reach
//! search — this move ends the memory↔search layer cycle).
//!
//! Only definitions ride here: any behavior (parsing, hashing, rendering)
//! lives in the search container via impl blocks on these types.

use serde::{Deserialize, Serialize};

/// Version of the parser/extractor machinery that produces a map; the
/// incremental scanner rebuilds maps from older extractors in full.
pub const EXTRACTOR_VERSION: u32 = 2;

/// Compressed code map (`.harna/digest.json`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Digest {
    pub generated_at: String,
    /// Version of the parser/extractor that produced the map (see above).
    #[serde(default)]
    pub extractor_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<Module>,
}

/// One source module (file or folder) reduced to its essentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    /// Path relative to repo root.
    pub path: String,
    /// Language hint, e.g. "rust".
    pub lang: String,
    /// Public signatures (functions, structs, traits) — no bodies. The
    /// propagation surface: parity fan-out and public diff classes key on
    /// this, because Rust visibility IS the cross-module boundary.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signatures: Vec<String>,
    /// Direct internal dependencies (module paths).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<String>,
    /// Hash of the file body: makes body-only changes visible to `harna diff`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body_hash: String,
    /// Full inventory: every declared item (pub and private) with visibility
    /// tag and kind — the map, not just the fan-out substrate.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inventory: Vec<Item>,
    /// Declared submodules — the codebase's own hierarchy, as written.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mods: Vec<String>,
}

/// One declared item in the inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    /// "pub" | "priv" (everything else in Rust is pub(crate)/pub(in ..) —
    /// both count as "pub" for the cross-module boundary, noted in the tag).
    pub vis: String,
    /// fn | struct | enum | trait | const | static | type | mod | impl
    pub kind: String,
    pub name: String,
    /// The declaration text (first line), for diff messages and search.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The data structs are the wire protocol between tool versions: a
    /// pre-0.3.9 digest (missing extractor_version) must still load, and
    /// the zero-value fields must stay invisible on write.
    #[test]
    fn old_maps_load_and_new_maps_write_snugly() {
        // The pre-extractor-era serialized shape: no extractor_version.
        let old = r#"{"generated_at":"then","modules":[{"path":"src/a.rs","lang":"rust"}]}"#;
        let d: Digest = serde_json::from_str(old).unwrap();
        assert_eq!(d.extractor_version, 0);
        assert_eq!(d.modules[0].path, "src/a.rs");

        // An empty-typed module roundtrips without the empty vectors.
        let ours = Digest {
            generated_at: "now".into(),
            extractor_version: EXTRACTOR_VERSION,
            modules: vec![Module {
                path: "src/b.rs".into(),
                lang: "rust".into(),
                signatures: vec![],
                deps: vec![],
                body_hash: String::new(),
                inventory: vec![Item {
                    vis: "priv".into(),
                    kind: "impl".into(),
                    name: "B".into(),
                    text: String::new(),
                }],
                mods: vec![],
            }],
        };
        let text = serde_json::to_string(&ours).unwrap();
        assert!(!text.contains("signatures"), "empty fields stay invisible");
        assert!(text.contains("priv"));
    }
}
