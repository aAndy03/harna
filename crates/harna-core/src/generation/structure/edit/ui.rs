//! Component `ui-catalog-edit` (1.1.5): a UI component's states and
//! variants, declared in the manifest. The gallery shows each one; a
//! variant lives in its original component, never in a copy of it.

use anyhow::bail;

use crate::schema::model::manifest::{Component, Manifest};

/// Find a component by name anywhere in the manifest (children included).
fn find<'m>(m: &'m mut Manifest, name: &str) -> Option<&'m mut Component> {
    fn walk<'c>(cs: &'c mut [Component], name: &str) -> Option<&'c mut Component> {
        for c in cs {
            if c.name == name {
                return Some(c);
            }
            if let Some(hit) = walk(&mut c.children, name) {
                return Some(hit);
            }
        }
        None
    }
    m.layers
        .iter_mut()
        .flat_map(|l| l.containers.iter_mut())
        .find_map(|c| walk(&mut c.components, name))
}

/// Words a state or variant may be: lowercase, digits, `-`.
fn valid(word: &str) -> bool {
    !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Set a component's states and/or variants; `None` leaves a list as it
/// is, an empty list clears it.
pub fn set(
    m: &mut Manifest,
    component: &str,
    states: Option<Vec<String>>,
    variants: Option<Vec<String>>,
) -> anyhow::Result<Vec<String>> {
    for w in states.iter().chain(variants.iter()).flatten() {
        if !valid(w) {
            bail!("`{w}`: a state or variant is a plain word (lowercase letters, digits, -)");
        }
    }
    let Some(c) = find(m, component) else {
        bail!(
            "no component named `{component}` (register it first: harna container register <container> <file>)"
        );
    };
    let mut changes = Vec::new();
    if let Some(s) = states.filter(|s| *s != c.states) {
        changes.push(format!("{component}: states = [{}]", s.join(", ")));
        c.states = s;
    }
    if let Some(v) = variants.filter(|v| *v != c.variants) {
        changes.push(format!("{component}: variants = [{}]", v.join(", ")));
        c.variants = v;
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::{Container, Layer};

    #[test]
    fn states_and_variants_are_set_on_the_component() {
        let mut m = Manifest {
            layers: vec![Layer {
                name: "ui".into(),
                containers: vec![Container {
                    name: "screens".into(),
                    path: "web/src".into(),
                    components: vec![Component {
                        name: "calendar".into(),
                        path: "web/src/Calendar.tsx".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let words = |s: &[&str]| Some(s.iter().map(|w| w.to_string()).collect());
        let c = set(
            &mut m,
            "calendar",
            words(&["default", "error"]),
            words(&["mini"]),
        )
        .unwrap();
        assert_eq!(c.len(), 2);
        assert!(set(&mut m, "calendar", words(&["default", "error"]), None)
            .unwrap()
            .is_empty());
        assert!(set(&mut m, "calendar", words(&["Bad Word"]), None).is_err());
        assert!(set(&mut m, "ghost", words(&["x"]), None).is_err());
        assert_eq!(m.required_schema(), 7);
    }
}
