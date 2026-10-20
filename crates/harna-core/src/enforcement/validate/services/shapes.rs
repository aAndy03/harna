//! Check `corridor-shape` (0.8.2; 0.8.4 after the review and field test):
//! both ends of a corridor speak the same message.
//!
//! For a message whose shape is a type in the code, every service on the
//! corridor either names that type (it links the shared shape) or declares
//! its own type of the same name — a TS interface mirroring a Rust struct,
//! say — whose fields must be the shape's fields. Names compare without
//! case or `_` (`posted_at` = `postedAt`); field types compare too when
//! both sides are the same language. A JSON Schema shape compares its
//! `properties` with each side's own type. What cannot be compared (an
//! enum, another schema format) says so (info), as does a service on the
//! corridor whose code never names the message, and a service that names
//! a corridor's message without being on the corridor.

use std::collections::HashMap;
use std::path::Path;

use super::super::{Severity, Violation};
use super::fields::{fields_of_with, Field};
use crate::enforcement::parity::policy::{uses_word, without_comments, word_regex};
use crate::schema::model::manifest::Manifest;
use crate::schema::store::services::Services;
use crate::search::digest::{Digest, Module};

const DECLARES: &[&str] = &["struct", "type", "interface", "class"];

/// Sources read once per check (0.8.4: every module was re-read per
/// message and per service).
struct Sources<'r> {
    root: &'r Path,
    raw: HashMap<String, Option<String>>,
    stripped: HashMap<String, String>,
}

impl Sources<'_> {
    fn raw(&mut self, p: &str) -> Option<&str> {
        let root = self.root;
        self.raw
            .entry(p.to_string())
            .or_insert_with(|| std::fs::read_to_string(root.join(p)).ok())
            .as_deref()
    }

    fn stripped(&mut self, p: &str) -> &str {
        if !self.stripped.contains_key(p) {
            let s = self.raw(p).map(without_comments).unwrap_or_default();
            self.stripped.insert(p.to_string(), s);
        }
        &self.stripped[p]
    }
}

fn is_rust(p: &str) -> bool {
    p.ends_with(".rs")
}

pub fn check(
    root: &Path,
    services: &Services,
    manifest: &Manifest,
    d: &Digest,
    out: &mut Vec<Violation>,
) {
    if services.corridors.is_empty() {
        return;
    }
    let mut src = Sources {
        root,
        raw: HashMap::new(),
        stripped: HashMap::new(),
    };
    // each service's modules, once
    let modules_of: HashMap<&str, Vec<&Module>> = services
        .services
        .iter()
        .map(|s| {
            let paths: Vec<&str> = manifest
                .layers
                .iter()
                .flat_map(|l| &l.containers)
                .filter(|x| s.containers.contains(&x.name))
                .map(|x| x.path.as_str())
                .collect();
            let mods = d
                .modules
                .iter()
                .filter(|m| {
                    paths
                        .iter()
                        .any(|p| crate::schema::model::scope::path_in_container(&m.path, p))
                })
                .collect();
            (s.name.as_str(), mods)
        })
        .collect();
    let mut push = |file: &str, severity: Severity, message: String, hint: &str| {
        out.push(Violation {
            check: "corridor-shape".into(),
            file: file.into(),
            line: 0,
            severity,
            message,
            hint: hint.into(),
        });
    };
    let error = crate::enforcement::checks::severity_of("corridor-shape");
    // a base declared in another file or package (0.9.2, TS field test:
    // `extends` across files read as missing fields)
    let lookup = |name: &str| -> Option<String> {
        d.modules
            .iter()
            .find(|m| {
                m.inventory
                    .iter()
                    .any(|i| i.name == name && DECLARES.contains(&i.kind.as_str()))
            })
            .and_then(|m| std::fs::read_to_string(root.join(&m.path)).ok())
    };
    let declares_in = |file: &str, ty: &str| {
        d.modules.iter().any(|m| {
            m.path == file
                && m.inventory.iter().any(|i| {
                    i.name == ty && (DECLARES.contains(&i.kind.as_str()) || i.kind == "enum")
                })
        })
    };
    for c in &services.corridors {
        for msg in &c.messages {
            // the shape's fields, and the type name each side declares
            let (wanted, ty, shape_file, shape_rust): (Vec<Field>, String, String, Option<bool>) =
                match msg.type_ref() {
                    Some((file, ty)) => {
                        let Some(raw) = src.raw(file).map(str::to_string) else {
                            continue; // coherence names a missing shape
                        };
                        match fields_of_with(&raw, ty, is_rust(file), &lookup) {
                            Some(f) => (f, ty.to_string(), file.to_string(), Some(is_rust(file))),
                            // a type the file does not declare is coherence's
                            // finding, not "an enum or alias" (0.9.2)
                            None if !declares_in(file, ty) => continue,
                            None => {
                                push(
                                    ".harna/services.toml",
                                    Severity::Info,
                                    format!(
                                        "corridor `{}`: `{ty}` ({file}) has no named fields to compare (an enum, a tuple or an alias) — its ends are not checked",
                                        c.name
                                    ),
                                    "a struct/interface shape lets corridor-shape hold both ends to it",
                                );
                                continue;
                            }
                        }
                    }
                    None if msg.shape.ends_with(".json") => {
                        let Some(raw) = src.raw(&msg.shape).map(str::to_string) else {
                            continue;
                        };
                        let props = match serde_json::from_str::<serde_json::Value>(&raw) {
                            Ok(v) => schema_properties(&v),
                            Err(e) => {
                                push(
                                    &msg.shape,
                                    error,
                                    format!(
                                        "corridor `{}`: schema `{}` is not valid JSON: {e}",
                                        c.name, msg.shape
                                    ),
                                    "fix the schema file",
                                );
                                continue;
                            }
                        };
                        match props {
                            Some(p) => (p, msg.name.clone(), msg.shape.clone(), None),
                            None => continue, // no properties: nothing to hold
                        }
                    }
                    None => {
                        push(
                            ".harna/services.toml",
                            Severity::Info,
                            format!(
                                "corridor `{}`: shape `{}` is a format harna does not read — its ends are not checked",
                                c.name, msg.shape
                            ),
                            "a type in the code or a JSON Schema file is compared",
                        );
                        continue;
                    }
                };
            let Some(word) = word_regex(&ty) else {
                continue;
            };
            // case, `_` and `-` aside: posted_at = postedAt = posted-at
            let key = |f: &str| {
                f.trim_start_matches('#')
                    .to_lowercase()
                    .replace(['_', '-'], "")
            };
            for s in c.from.iter().chain(&c.to) {
                let Some(mods) = modules_of.get(s.as_str()) else {
                    continue;
                };
                let own: Vec<&&Module> = mods
                    .iter()
                    .filter(|m| {
                        m.path != shape_file
                            && m.inventory
                                .iter()
                                .any(|i| i.name == ty && DECLARES.contains(&i.kind.as_str()))
                    })
                    .collect();
                for m in &own {
                    let Some(raw) = src.raw(&m.path).map(str::to_string) else {
                        continue;
                    };
                    let Some(theirs) = fields_of_with(&raw, &ty, is_rust(&m.path), &lookup) else {
                        continue;
                    };
                    let missing: Vec<&str> = wanted
                        .iter()
                        .filter(|(w, _)| !theirs.iter().any(|(t, _)| key(t) == key(w)))
                        .map(|(w, _)| w.as_str())
                        .collect();
                    let extra: Vec<&str> = theirs
                        .iter()
                        .filter(|(t, _)| !wanted.iter().any(|(w, _)| key(w) == key(t)))
                        .map(|(t, _)| t.as_str())
                        .collect();
                    // types, when both sides speak the same language
                    let typed: Vec<String> = if shape_rust == Some(is_rust(&m.path)) {
                        wanted
                            .iter()
                            .filter_map(|(w, wt)| {
                                let (_, tt) = theirs.iter().find(|(t, _)| key(t) == key(w))?;
                                // an untyped side (an initializer) says nothing
                                (!tt.is_empty() && !wt.is_empty() && tt != wt)
                                    .then(|| format!("{w}: {tt} here, {wt} in the shape"))
                            })
                            .collect()
                    } else {
                        // across languages (or against a JSON Schema, whose
                        // kinds are already on the wire), the wire decides
                        // (0.9.5): only two known kinds that differ count
                        let here_rust = is_rust(&m.path);
                        wanted
                            .iter()
                            .filter_map(|(w, wt)| {
                                let (_, tt) = theirs.iter().find(|(t, _)| key(t) == key(w))?;
                                let a = super::wire::wire_kind(tt, here_rust)?;
                                let b = match shape_rust {
                                    Some(r) => super::wire::wire_kind(wt, r)?,
                                    None => Some(wt.clone()).filter(|k| !k.is_empty())?,
                                };
                                super::wire::disagree(&a, &b)
                                    .then(|| format!("{w}: {tt} ({a}) here, {b} in the shape"))
                            })
                            .collect()
                    };
                    if !missing.is_empty() || !extra.is_empty() || !typed.is_empty() {
                        let mut parts = Vec::new();
                        // only the lists that hold something (0.9.2)
                        match (missing.is_empty(), extra.is_empty()) {
                            (false, false) => parts.push(format!(
                                "missing [{}], extra [{}]",
                                missing.join(", "),
                                extra.join(", ")
                            )),
                            (false, true) => {
                                parts.push(format!("missing [{}]", missing.join(", ")))
                            }
                            (true, false) => parts.push(format!("extra [{}]", extra.join(", "))),
                            (true, true) => {}
                        }
                        if !typed.is_empty() {
                            parts.push(format!("types differ: {}", typed.join("; ")));
                        }
                        push(
                            &m.path,
                            error,
                            format!(
                                "service `{s}`'s `{ty}` differs from corridor `{}`'s shape ({shape_file}): {}",
                                c.name,
                                parts.join("; ")
                            ),
                            "make the fields match, or have both sides link the one shape from shared code",
                        );
                    }
                }
                let names_it = shape_rust.is_some()
                    && mods.iter().any(|m| {
                        m.path == shape_file || {
                            let st = src.stripped(&m.path);
                            uses_word(st, &word, &ty)
                        }
                    });
                if own.is_empty() && !names_it {
                    push(
                        ".harna/services.toml",
                        Severity::Info,
                        format!(
                            "service `{s}` is on corridor `{}` but its code never names `{ty}`: its side of `{}` cannot be read",
                            c.name, msg.name
                        ),
                        "send or receive the message by its declared type, or fix the corridor's services",
                    );
                }
            }
            // a service reading the message without being on the corridor
            if shape_rust.is_some() {
                for s in &services.services {
                    if c.from.contains(&s.name) || c.to.contains(&s.name) {
                        continue;
                    }
                    let Some(mods) = modules_of.get(s.name.as_str()) else {
                        continue;
                    };
                    let user = mods.iter().find(|m| {
                        m.path != shape_file && {
                            let st = src.stripped(&m.path);
                            uses_word(st, &word, &ty)
                        }
                    });
                    if let Some(m) = user {
                        push(
                            &m.path,
                            Severity::Info,
                            format!(
                                "service `{}` names `{ty}`, a message of corridor `{}`, but is not on the corridor",
                                s.name, c.name
                            ),
                            "add the service to the corridor's --from/--to, or stop reading the message there",
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::map::Item;
    use crate::schema::model::manifest::{Container, Layer};
    use crate::schema::store::services::{Corridor, Message, Service};
    use crate::search::digest::Module;

    /// 0.8.2: the Rust sender links the shape; the TS receiver's own
    /// `Posted` lacks `amount` and adds `note` — named, case-insensitively
    /// matching `posted_at`/`postedAt`.
    #[test]
    fn corridor_ends_speak_the_same_shape() {
        let root = std::env::temp_dir().join(format!("harna-shapes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let w = |rel: &str, body: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        w(
            "crates/events/src/lib.rs",
            "pub struct Posted {\n    pub id: u64,\n    pub posted_at: String,\n    pub amount: i64,\n}\n",
        );
        w(
            "crates/api/src/lib.rs",
            "use events::Posted;\nfn send(p: Posted) {}\n",
        );
        w(
            "web/src/types.ts",
            "export interface Posted { id: number; postedAt: string; note: string }\n",
        );
        let c = |n: &str, p: &str| Container {
            name: n.into(),
            path: p.into(),
            ..Default::default()
        };
        let m = Manifest {
            layers: vec![Layer {
                name: "all".into(),
                containers: vec![
                    c("events", "crates/events/src"),
                    c("api", "crates/api/src"),
                    c("web", "web/src"),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let item = |n: &str, k: &str| Item {
            vis: "pub".into(),
            kind: k.into(),
            name: n.into(),
            text: String::new(),
        };
        let d = Digest {
            modules: vec![
                Module {
                    path: "crates/events/src/lib.rs".into(),
                    inventory: vec![item("Posted", "struct")],
                    ..Default::default()
                },
                Module {
                    path: "crates/api/src/lib.rs".into(),
                    ..Default::default()
                },
                Module {
                    path: "web/src/types.ts".into(),
                    inventory: vec![item("Posted", "interface")],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let svc = |n: &str| Service {
            name: n.into(),
            containers: vec![n.into()],
            ..Default::default()
        };
        let s = Services {
            services: vec![svc("api"), svc("web")],
            corridors: vec![Corridor {
                name: "postings".into(),
                from: vec!["api".into()],
                to: vec!["web".into()],
                messages: vec![Message::parse("crates/events/src/lib.rs::Posted").unwrap()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut out = Vec::new();
        check(&root, &s, &m, &d, &mut out);
        assert_eq!(out.len(), 1, "{out:#?}");
        assert_eq!(out[0].file, "web/src/types.ts");
        assert!(
            out[0].message.contains("missing [amount], extra [note]"),
            "{}",
            out[0].message
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 0.8.4: a Rust mirror whose field type drifts is named; a JSON
    /// Schema shape holds a TS mirror to its properties; a service off the
    /// corridor that names the message is told (info).
    #[test]
    fn types_schemas_and_undeclared_readers() {
        let root = std::env::temp_dir().join(format!("harna-shapes2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let w = |rel: &str, body: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        w(
            "crates/events/src/lib.rs",
            "pub struct Posted {\n    pub id: u64,\n}\n",
        );
        w(
            "crates/api/src/lib.rs",
            "pub struct Posted {\n    pub id: String,\n}\n",
        );
        w("crates/audit/src/lib.rs", "fn f(p: events::Posted) {}\n");
        w(
            "schemas/refund.json",
            "{\"properties\": {\"id\": {}, \"reason\": {}}}",
        );
        w(
            "web/src/refund.ts",
            "export interface Refund { id: number }\n",
        );
        let c = |n: &str, p: &str| Container {
            name: n.into(),
            path: p.into(),
            ..Default::default()
        };
        let m = Manifest {
            layers: vec![Layer {
                name: "all".into(),
                containers: vec![
                    c("events", "crates/events/src"),
                    c("api", "crates/api/src"),
                    c("audit", "crates/audit/src"),
                    c("web", "web/src"),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let item = |n: &str, k: &str| Item {
            vis: "pub".into(),
            kind: k.into(),
            name: n.into(),
            text: String::new(),
        };
        let module = |p: &str, items: Vec<Item>| Module {
            path: p.into(),
            inventory: items,
            ..Default::default()
        };
        let d = Digest {
            modules: vec![
                module("crates/events/src/lib.rs", vec![item("Posted", "struct")]),
                module("crates/api/src/lib.rs", vec![item("Posted", "struct")]),
                module("crates/audit/src/lib.rs", vec![]),
                module("web/src/refund.ts", vec![item("Refund", "interface")]),
            ],
            ..Default::default()
        };
        let svc = |n: &str| Service {
            name: n.into(),
            containers: vec![n.into()],
            ..Default::default()
        };
        let s = Services {
            services: vec![svc("api"), svc("audit"), svc("web")],
            corridors: vec![
                Corridor {
                    name: "postings".into(),
                    from: vec!["api".into()],
                    to: vec!["web".into()],
                    messages: vec![Message::parse("crates/events/src/lib.rs::Posted").unwrap()],
                    ..Default::default()
                },
                Corridor {
                    name: "refunds".into(),
                    from: vec!["api".into()],
                    to: vec!["web".into()],
                    messages: vec![Message::parse("schemas/refund.json").unwrap()],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(s.corridors[1].messages[0].name, "refund");
        let mut s = s;
        s.corridors[1].messages[0].name = "Refund".into();
        let mut out = Vec::new();
        check(&root, &s, &m, &d, &mut out);
        let msgs: Vec<String> = out.iter().map(|v| v.message.clone()).collect();
        assert!(
            msgs.iter()
                .any(|m| m.contains("types differ: id: String here, u64 in the shape")),
            "{msgs:#?}"
        );
        assert!(
            msgs.iter()
                .any(|m| m.contains("`Refund`") && m.contains("missing [reason]")),
            "{msgs:#?}"
        );
        assert!(
            msgs.iter()
                .any(|m| m.contains("service `audit` names `Posted`")),
            "{msgs:#?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// A JSON Schema's top-level property names: its own `properties`, those
/// of every `allOf` part, and a local `$ref` (`#/definitions/X`,
/// `#/$defs/X`) followed (0.9.2, TS field test). None when it names none.
fn schema_properties(root: &serde_json::Value) -> Option<Vec<Field>> {
    fn collect(
        v: &serde_json::Value,
        root: &serde_json::Value,
        out: &mut Vec<Field>,
        depth: usize,
    ) {
        if depth > 8 {
            return;
        }
        if let Some(r) = v.get("$ref").and_then(|r| r.as_str()) {
            if let Some(path) = r.strip_prefix("#/") {
                let target = path.split('/').try_fold(root, |node, seg| node.get(seg));
                if let Some(t) = target {
                    collect(t, root, out, depth + 1);
                }
            }
        }
        if let Some(props) = v.get("properties").and_then(|p| p.as_object()) {
            for (k, p) in props {
                if !out.iter().any(|(n, _)| n == k) {
                    // the property's wire kind (0.9.5), or nothing known
                    out.push((k.clone(), super::wire::json_kind(p).unwrap_or_default()));
                }
            }
        }
        for part in v
            .get("allOf")
            .and_then(|a| a.as_array())
            .into_iter()
            .flatten()
        {
            collect(part, root, out, depth + 1);
        }
    }
    let mut out = Vec::new();
    collect(root, root, &mut out, 0);
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod schema_tests {
    /// 0.9.2: properties come from the schema itself, `allOf` parts and
    /// local `$ref`s.
    #[test]
    fn schema_properties_follow_all_of_and_refs() {
        let v: serde_json::Value = serde_json::from_str(
            r##"{"allOf": [{"$ref": "#/$defs/Stamped"}, {"properties": {"amount": {}}}],
                "properties": {"note": {}},
                "$defs": {"Stamped": {"properties": {"id": {}, "at": {}}}}}"##,
        )
        .unwrap();
        let mut names: Vec<String> = super::schema_properties(&v)
            .unwrap()
            .into_iter()
            .map(|f| f.0)
            .collect();
        names.sort();
        assert_eq!(names, vec!["amount", "at", "id", "note"]);
    }
}
