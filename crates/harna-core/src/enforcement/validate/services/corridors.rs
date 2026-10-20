//! Corridors under `service-coherence` (0.8.1): each corridor joins
//! declared services, has a sender and a receiver, and every message's
//! shape is somewhere: a type the map holds in the named file, or a schema
//! file on disk.

use std::collections::BTreeMap;
use std::path::Path;

use super::super::Violation;
use crate::schema::store::services::Services;
use crate::search::digest::Digest;

/// Kinds that declare a shape in code.
const SHAPE_KINDS: &[&str] = &["struct", "enum", "type", "interface", "class", "trait"];

pub fn check(root: &Path, services: &Services, digest: Option<&Digest>, out: &mut Vec<Violation>) {
    let mut push = |message: String, hint: &str| {
        out.push(Violation {
            check: "service-coherence".into(),
            file: ".harna/services.toml".into(),
            line: 0,
            severity: crate::enforcement::checks::severity_of("service-coherence"),
            message,
            hint: hint.into(),
        });
    };
    let mut names: BTreeMap<&str, usize> = BTreeMap::new();
    for c in &services.corridors {
        *names.entry(c.name.as_str()).or_default() += 1;
        for s in c.from.iter().chain(&c.to) {
            if services.get(s).is_none() {
                push(
                    format!(
                        "corridor `{}` joins `{s}`, which is not a declared service",
                        c.name
                    ),
                    "declare it (harna service add) or fix the corridor's --from/--to",
                );
            }
        }
        if c.from.is_empty() || c.to.is_empty() {
            push(
                format!(
                    "corridor `{}` needs a sender and a receiver (from: {}, to: {})",
                    c.name,
                    c.from.len(),
                    c.to.len()
                ),
                "harna service corridor add <name> --from <service> --to <service>",
            );
        }
        for m in &c.messages {
            match m.type_ref() {
                Some((file, ty)) => {
                    let Some(d) = digest else {
                        continue; // no map: nothing to resolve against
                    };
                    let found = d.modules.iter().find(|x| x.path == file).map(|x| {
                        x.inventory
                            .iter()
                            .any(|i| i.name == ty && SHAPE_KINDS.contains(&i.kind.as_str()))
                    });
                    match found {
                        None => push(
                            format!("corridor `{}`: message `{}` names `{file}`, which the map does not hold", c.name, m.name),
                            "point the shape at a source file (<file>::<Type>), or run harna scan",
                        ),
                        Some(false) => push(
                            format!("corridor `{}`: message `{}` names `{ty}`, which `{file}` does not declare", c.name, m.name),
                            "declare the type there, or point the shape where it lives",
                        ),
                        Some(true) => {}
                    }
                }
                None => {
                    if !root.join(&m.shape).is_file() {
                        push(
                            format!(
                                "corridor `{}`: message `{}` shape `{}` is not a file",
                                c.name, m.name, m.shape
                            ),
                            "a shape is <file>::<Type> or a schema file that exists",
                        );
                    }
                }
            }
        }
    }
    for (n, count) in names {
        if count > 1 {
            push(
                format!("corridor `{n}` is declared {count} times"),
                "one entry per corridor: merge them",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::map::Item;
    use crate::schema::store::services::{Corridor, Message, Service};
    use crate::search::digest::Module;

    /// 0.8.1: a corridor joining an undeclared service, one without a
    /// receiver, a shape the map lacks or a type the file does not declare,
    /// a schema file that is missing — each named; a good corridor passes.
    #[test]
    fn corridors_join_services_and_carry_known_shapes() {
        let svc = |n: &str| Service {
            name: n.into(),
            ..Default::default()
        };
        let d = Digest {
            modules: vec![Module {
                path: "crates/events/src/lib.rs".into(),
                inventory: vec![Item {
                    vis: "pub".into(),
                    kind: "struct".into(),
                    name: "PostingCreated".into(),
                    text: String::new(),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let msg = |s: &str| Message::parse(s).unwrap();
        let good = Services {
            services: vec![svc("api"), svc("worker")],
            corridors: vec![Corridor {
                name: "postings".into(),
                kind: "event".into(),
                from: vec!["api".into()],
                to: vec!["worker".into()],
                messages: vec![msg("crates/events/src/lib.rs::PostingCreated")],
            }],
            ..Default::default()
        };
        assert_eq!(good.corridors[0].messages[0].name, "PostingCreated");
        let root = std::env::temp_dir();
        let mut out = Vec::new();
        check(&root, &good, Some(&d), &mut out);
        assert!(out.is_empty(), "{out:?}");
        let mut bad = good.clone();
        bad.corridors.push(Corridor {
            name: "sync".into(),
            from: vec!["ghost".into()],
            to: vec![],
            messages: vec![
                msg("Gone=crates/events/src/lib.rs::PostingGone"),
                msg("Elsewhere=crates/nope/src/lib.rs::X"),
                msg("Schema=schemas/missing.json"),
            ],
            ..Default::default()
        });
        let mut out = Vec::new();
        check(&root, &bad, Some(&d), &mut out);
        let msgs: Vec<&str> = out.iter().map(|v| v.message.as_str()).collect();
        assert_eq!(msgs.len(), 5, "{msgs:#?}");
        assert!(msgs[0].contains("`ghost`"));
        assert!(msgs[1].contains("sender and a receiver"));
        assert!(msgs[2].contains("does not declare"));
        assert!(msgs[3].contains("does not hold"));
        assert!(msgs[4].contains("is not a file"));
        assert!(Message::parse("=x").is_err());
    }
}
