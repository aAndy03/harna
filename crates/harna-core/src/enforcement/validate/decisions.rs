//! Component `decision-chain`: coherence of the decision log's supersedes
//! pointers. A decision stays referenced but visibly dead — as long as the
//! pointers into and along the chain actually resolve (unknown targets and
//! cycles turn the chain into fiction).

use std::collections::HashSet;

use crate::schema::store::Store;

use super::{Severity, Violation};

pub fn check(store: &Store, out: &mut Vec<Violation>) {
    let decisions = match crate::schema::decisions::list(store) {
        Ok(d) => d,
        // Unreadable log: doctor owns the data-file health checks; skip.
        Err(_) => return,
    };
    let files: HashSet<&str> = decisions.iter().map(|d| d.file.as_str()).collect();

    for d in &decisions {
        if let Some(target) = &d.supersedes {
            if target == d.file.as_str() {
                out.push(violation(
                    &d.file,
                    format!("decision `{}` supersedes itself", d.file),
                ));
            } else if !files.contains(target.as_str()) {
                out.push(violation(
                    &d.file,
                    format!(
                        "decision `{}` supersedes `{target}`, but no such decision exists",
                        d.file
                    ),
                ));
            }
        }
    }

    // Cycle detection: from every decision, follow supersedes pointers; a
    // revisit within one walk means the chain loops.
    for start in &decisions {
        if cycle_from(start, &decisions) {
            out.push(violation(
                &start.file,
                format!(
                    "supersedes chain through `{}` loops — a cycle makes every link in it fiction",
                    start.file
                ),
            ));
        }
    }
}

fn cycle_from(start: &crate::schema::decisions::Decision, decisions: &[crate::schema::decisions::Decision]) -> bool {
    let mut visited: HashSet<String> = HashSet::new();
    let mut cursor = start.supersedes.clone();
    while let Some(next) = cursor {
        if !visited.insert(next.clone()) {
            return true;
        }
        match decisions.iter().find(|d| d.file == next) {
            Some(step) => cursor = step.supersedes.clone(),
            None => break, // unknown target is reported separately
        }
    }
    false
}

fn violation(file: &str, message: String) -> Violation {
    Violation {
        check: "decision-chain".into(),
        file: format!(".harna/decisions/{file}"),
        line: 0,
        message,
        hint: "fix the supersedes pointer (exact decision filename), or remove it"
            .into(),
        severity: Severity::Error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::decisions;

    fn store_with(tag: &str, nodes: &[(&str, Option<&str>)]) -> (std::path::PathBuf, Store) {
        let dir = std::env::temp_dir()
            .join(format!("harna-dec-chain-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(dir.join(".harna"));
        let store = Store::open(&dir).unwrap();
        store.init().unwrap();
        for (name, supersedes) in nodes {
            let pointer: Option<String> = supersedes.map(|s| s.to_string());
            let body = decisions::markdown("T", "b", pointer.as_deref());
            std::fs::write(store.decisions_dir().join(format!("{name}.md")), body).unwrap();
        }
        (dir, store)
    }

    fn only(v: &[Violation]) -> Vec<Violation> {
        v.iter().filter(|x| x.check == "decision-chain").cloned().collect()
    }

    #[test]
    fn unknown_or_self_targets_are_errors() {
        let (dir, store) = store_with(
            "bad",
            &[
                ("ghost-pointer", Some("no-such-decision.md")),
                ("self-pointer", Some("self-pointer.md")),
                ("clean", None),
            ],
        );
        let mut v = Vec::new();
        check(&store, &mut v);
        let hits = only(&v);
        assert!(
            hits.iter().any(|x| x.message.contains("no such decision exists")),
            "ghost pointer is an error"
        );
        assert!(
            hits.iter().any(|x| x.message.contains("supersedes itself")),
            "self pointer is an error (and also routes the cycle check)"
        );
        assert!(hits.iter().all(|x| x.severity == Severity::Error));
        let _ = std::fs::remove_dir_all(dir.join(".harna"));
    }

    #[test]
    fn two_node_cycle_is_flagged_but_a_chain_is_not() {
        // Honest chain: 2 supersedes 1 — no cycle.
        let (dir, store) = store_with("loop", &[("1", None), ("2", Some("1.md"))]);
        let mut v = Vec::new();
        check(&store, &mut v);
        assert!(only(&v).is_empty());

        // Loop formed by hand-editing pointers: 2 -> 1 and 1 -> 2.
        std::fs::write(
            store.decisions_dir().join("1.md"),
            decisions::markdown("T", "b", Some("2.md")),
        )
        .unwrap();
        let mut v2 = Vec::new();
        check(&store, &mut v2);
        let hits = only(&v2);
        assert!(
            hits.iter().any(|x| x.message.contains("loops")),
            "2 -> 1 -> 2 must be caught"
        );
        let _ = std::fs::remove_dir_all(dir.join(".harna"));
    }
}
