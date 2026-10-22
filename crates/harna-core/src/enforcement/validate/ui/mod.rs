//! Checks group `ui` (1.1.5): the UI catalog holds what it declares.
//!
//! - `ui-states`: a component that declares states or variants has a
//!   gallery (its registered tests of the `gallery` kind, or a story or
//!   gallery file) that names each one. Rarely seen states (error, empty,
//!   disabled) are where looks regress unseen.
//! - `variant-home`: a file named after a declared variant of a component
//!   (`MiniCalendar.tsx` beside a calendar that declares `mini`) forks the
//!   component: the variant belongs in the original.
//!
//! Text only: harna reads names in files, it renders nothing.

use std::collections::BTreeSet;
use std::path::Path;

use super::Violation;
use crate::schema::model::manifest::{Component, Manifest};

/// The lowercase words of an identifier-ish text: split on anything not a
/// letter or digit, and on camel case (`MiniCalendarError` -> mini,
/// calendar, error).
pub fn words(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for ch in text.chars() {
        if !ch.is_alphanumeric() {
            if !cur.is_empty() {
                out.insert(std::mem::take(&mut cur));
            }
            prev_lower = false;
            continue;
        }
        if ch.is_uppercase() && prev_lower && !cur.is_empty() {
            out.insert(std::mem::take(&mut cur));
        }
        prev_lower = ch.is_lowercase() || ch.is_ascii_digit();
        cur.extend(ch.to_lowercase());
    }
    if !cur.is_empty() {
        out.insert(cur);
    }
    out
}

/// Does `have` hold every word of `name` (a state like `read-only`)?
fn names(have: &BTreeSet<String>, name: &str) -> bool {
    let want = words(name);
    !want.is_empty() && want.iter().all(|w| have.contains(w))
}

fn ui_components(m: &Manifest) -> Vec<&Component> {
    fn walk<'c>(cs: &'c [Component], out: &mut Vec<&'c Component>) {
        for c in cs {
            if c.is_ui() {
                out.push(c);
            }
            walk(&c.children, out);
        }
    }
    let mut out = Vec::new();
    for c in m.layers.iter().flat_map(|l| &l.containers) {
        walk(&c.components, &mut out);
    }
    out
}

/// A component's gallery files: its registered tests of the `gallery`
/// kind when the manifest declares one, else those named like a story or
/// a gallery.
fn gallery_of<'c>(m: &Manifest, c: &'c Component) -> Vec<&'c String> {
    let kind = m.test_kinds.iter().find(|k| k.name == "gallery");
    c.tests
        .iter()
        .filter(|t| match kind {
            Some(k) => k.covers(t),
            None => {
                let t = t.to_ascii_lowercase();
                t.contains("stories") || t.contains("story") || t.contains("gallery")
            }
        })
        .collect()
}

/// `files`: the repository's source files (the map's modules).
pub fn check(root: &Path, m: &Manifest, files: &[String], out: &mut Vec<Violation>) {
    let ui = ui_components(m);
    if ui.is_empty() {
        return;
    }
    for c in &ui {
        states(root, m, c, out);
        variant_home(m, c, files, out);
    }
}

fn states(root: &Path, m: &Manifest, c: &Component, out: &mut Vec<Violation>) {
    let declared: Vec<&String> = c.states.iter().chain(&c.variants).collect();
    let gallery = gallery_of(m, c);
    let file = if c.path.is_empty() {
        ".harna/manifest.toml".to_string()
    } else {
        c.path.clone()
    };
    if gallery.is_empty() {
        out.push(Violation {
            check: "ui-states".into(),
            file,
            line: 0,
            severity: crate::enforcement::checks::severity_of("ui-states"),
            message: format!(
                "`{}` declares {} state(s) and {} variant(s), but no gallery shows them",
                c.name,
                c.states.len(),
                c.variants.len()
            ),
            hint: format!(
                "write a gallery entry per state and variant (a story file, a gallery screen) and register it: harna container tests <container> --component {} --add <file>",
                c.name
            ),
        });
        return;
    }
    let mut have = BTreeSet::new();
    for g in &gallery {
        if let Ok(text) = std::fs::read_to_string(root.join(g.as_str())) {
            have.extend(words(&text));
        }
    }
    let missing: Vec<&str> = declared
        .iter()
        .filter(|w| !names(&have, w))
        .map(|w| w.as_str())
        .collect();
    if missing.is_empty() {
        return;
    }
    out.push(Violation {
        check: "ui-states".into(),
        file: gallery[0].clone(),
        line: 0,
        severity: crate::enforcement::checks::severity_of("ui-states"),
        message: format!(
            "`{}`'s gallery never shows {}: a look nobody sees regresses unseen",
            c.name,
            missing
                .iter()
                .map(|w| format!("`{w}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        hint: "add one gallery entry per missing state or variant (its name in the entry, e.g. `export const Error`)".into(),
    });
}

fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.split('.').next().unwrap_or(name)
}

fn variant_home(m: &Manifest, c: &Component, files: &[String], out: &mut Vec<Violation>) {
    if c.variants.is_empty() || c.path.is_empty() {
        return;
    }
    let own = words(stem(&c.path));
    if own.is_empty() {
        return;
    }
    let gallery = gallery_of(m, c);
    for f in files {
        if *f == c.path || gallery.contains(&f) || crate::search::digest::is_test_path(f) {
            continue;
        }
        let theirs = words(stem(f));
        if !own.iter().all(|w| theirs.contains(w)) {
            continue;
        }
        let rest: BTreeSet<String> = theirs.difference(&own).cloned().collect();
        let Some(v) = c.variants.iter().find(|v| {
            let vw = words(v);
            !vw.is_empty() && vw == rest
        }) else {
            continue;
        };
        out.push(Violation {
            check: "variant-home".into(),
            file: f.clone(),
            line: 0,
            severity: crate::enforcement::checks::severity_of("variant-home"),
            message: format!(
                "looks like the `{v}` variant of `{}` in a file of its own: a copy forks the component, and fixes land in one of them",
                c.name
            ),
            hint: format!(
                "render `{v}` from {} (a prop or a variant parameter) and remove this file",
                c.path
            ),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::{Container, Layer};

    fn manifest(tests: &[&str]) -> Manifest {
        Manifest {
            layers: vec![Layer {
                name: "ui".into(),
                containers: vec![Container {
                    name: "screens".into(),
                    path: "web/src".into(),
                    components: vec![Component {
                        name: "calendar".into(),
                        path: "web/src/Calendar.tsx".into(),
                        tests: tests.iter().map(|s| s.to_string()).collect(),
                        states: vec!["default".into(), "error".into(), "read-only".into()],
                        variants: vec!["mini".into()],
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn words_split_identifiers_and_camel_case() {
        let w: Vec<String> = words("export const MiniReadOnly = 1; // error-state")
            .into_iter()
            .collect();
        assert_eq!(
            w,
            vec!["1", "const", "error", "export", "mini", "only", "read", "state"]
        );
    }

    #[test]
    fn the_gallery_names_every_state_and_variant() {
        let root = std::env::temp_dir().join(format!("harna-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("web/src")).unwrap();
        std::fs::write(
            root.join("web/src/Calendar.stories.tsx"),
            "export const Default = {};\nexport const Error = {};\nexport const Mini = {};\n",
        )
        .unwrap();
        let mut out = Vec::new();
        check(&root, &manifest(&[]), &[], &mut out);
        assert!(out[0].message.contains("no gallery"), "{out:?}");
        let mut out = Vec::new();
        check(
            &root,
            &manifest(&["web/src/Calendar.stories.tsx"]),
            &[],
            &mut out,
        );
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].message.contains("`read-only`"), "{}", out[0].message);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_named_after_a_variant_is_a_fork() {
        let files: Vec<String> = [
            "web/src/Calendar.tsx",
            "web/src/MiniCalendar.tsx",
            "web/src/CalendarHeader.tsx",
            "web/src/calendar_mini.rs",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let mut out = Vec::new();
        check(
            Path::new("."),
            &manifest(&["web/src/Calendar.stories.tsx"]),
            &files,
            &mut out,
        );
        let forks: Vec<&str> = out
            .iter()
            .filter(|v| v.check == "variant-home")
            .map(|v| v.file.as_str())
            .collect();
        assert_eq!(
            forks,
            vec!["web/src/MiniCalendar.tsx", "web/src/calendar_mini.rs"]
        );
    }
}
