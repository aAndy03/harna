//! The architecture manifest: layers -> containers -> components.
//!
//! Components are `primitive` (leaf building blocks) or `compound`
//! (hierarchical compositions of other components). Compound components
//! nest via `children`, keeping the hierarchy explicit but standalone.

use serde::{Deserialize, Serialize};

/// Top-level manifest document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Data schema version; written by the tool, migrated by `harna migrate`.
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self { schema_version: crate::SCHEMA_VERSION, layers: Vec::new() }
    }
}

pub fn current_schema_version() -> u32 {
    crate::SCHEMA_VERSION
}

/// A horizontal architectural layer (e.g. "data-access", "domain").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Explicit boot/init order of the layer's containers. Empty = unordered.
    /// Declared explicitly rather than numeric — inserting never renumbers
    /// siblings; reordering is a one-line diff.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sequence: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub containers: Vec<Container>,
}

/// A container groups related components inside a layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Container {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    /// Explicit order of the container's components (same logic as layer.sequence).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sequence: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<Component>,
}

/// A backend component: primitive leaf or compound composition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub kind: ComponentKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Component>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentKind {
    Primitive,
    Compound,
}

impl Manifest {
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// Total number of components (including nested children).
    pub fn component_count(&self) -> usize {
        self.layers
            .iter()
            .flat_map(|l| l.containers.iter())
            .map(|c| count_components(&c.components))
            .sum()
    }

    pub fn container_count(&self) -> usize {
        self.layers.iter().map(|l| l.containers.len()).sum()
    }
}

fn count_components(comps: &[Component]) -> usize {
    comps.iter().map(|c| 1 + count_components(&c.children)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        let toml_src = r#"
[[layers]]
name = "data-access"

[[layers.containers]]
name = "users"
path = "src/data/users"

[[layers.containers.components]]
name = "user-repo"
kind = "primitive"
path = "src/data/users/repo.rs"

[[layers.containers.components]]
name = "user-store"
kind = "compound"
path = "src/data/users/store"

[[layers.containers.components.children]]
name = "connection-pool"
kind = "primitive"
"#;
        toml::from_str(toml_src).unwrap()
    }

    #[test]
    fn roundtrip() {
        let m = sample();
        let s = toml::to_string(&m).unwrap();
        let back: Manifest = toml::from_str(&s).unwrap();
        assert_eq!(m.component_count(), back.component_count());
    }

    #[test]
    fn counts_nested_children() {
        assert_eq!(sample().component_count(), 3);
        assert_eq!(sample().container_count(), 1);
    }
}
