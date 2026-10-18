//! Component `parity-data` (0.3.22): the parity TEMPLATES' data structs
//! live in the memory layer — same discipline as `digest-data`: the store
//! persists them, and data owned by the store belongs next to it, while
//! the behavior (matching, reporting) stays with the parity component in
//! enforcement.
//!
//! A parity template declares a repeating change type's fan-out shape: a
//! scope of module paths and a ceiling on how many targets an event over
//! that scope should carry. Templates are data, not verdicts — an event
//! that exceeds a declared shape is an orientation (Info by default), and
//! a bound rule may promote the severity.

use serde::{Deserialize, Serialize};

/// Version of the template document (`.harna/templates.toml`).
fn current() -> u32 {
    crate::SCHEMA_VERSION
}

/// Top-level templates document (`.harna/templates.toml`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParityTemplates {
    #[serde(default = "current")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<ParityTemplate>,
}

/// One declared fan-out shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParityTemplate {
    /// Stable id, e.g. "schema-shard-change".
    pub id: String,
    /// Module-path prefix this change type touches. Empty = every event.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scope: String,
    /// The shape's ceiling: an event over this scope shouldn't carry more
    /// targets than this without somebody asking why.
    pub max_targets: usize,
    /// Why this shape is declared — the orientation's reason, not just its id.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rationale: String,
}

impl ParityTemplates {
    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    /// Templates whose scope prefixes the path (empty scope = every path).
    pub fn matching<'a>(&'a self, module_path: &str) -> Vec<&'a ParityTemplate> {
        self.templates
            .iter()
            .filter(|t| {
                t.scope.is_empty()
                    || module_path.starts_with(&format!("{}/", t.scope))
                    || module_path == t.scope
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(id: &str, scope: &str, max: usize) -> ParityTemplate {
        ParityTemplate {
            id: id.into(),
            scope: scope.into(),
            max_targets: max,
            rationale: "test".into(),
        }
    }

    #[test]
    fn scope_prefixes_match_inside_only() {
        let t = ParityTemplates {
            schema_version: crate::SCHEMA_VERSION,
            templates: vec![
                template("schema", "crates/x/src/schema", 6),
                template("all", "", 50),
            ],
        };
        // Inside the prefix, and the module deeper in it.
        assert!(t.matching("crates/x/src/schema/store.rs").len() == 2);
        // The scope exactly as written.
        assert!(t.matching("crates/x/src/schema").len() == 2);
        // A SIBLING whose name shares the prefix does not match
        // ("schema2/" is not under "schema/").
        assert!(!t
            .matching("crates/x/src/schema2/store.rs")
            .contains(&&t.templates[0]));
        // Elsewhere: only the empty-scope template.
        assert_eq!(t.matching("crates/y/src/main.rs").len(), 1);
    }
}
