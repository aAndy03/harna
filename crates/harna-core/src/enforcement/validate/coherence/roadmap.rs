//! Component `roadmap-coherence` (0.3.28): the declared roadmap must be
//! sound data, and the deferred register's milestone-keyed reviews must
//! point at something declared. A ghost milestone id turns a promised
//! review into fiction — exactly the ghost-pointer class the decision
//! chain check polices, one register over.

use crate::schema::registers::deferred::{review_kind, ReviewKind};
use crate::schema::store::Store;

use super::{Severity, Violation};

pub fn check(store: &Store, out: &mut Vec<Violation>) {
    let roadmap = match store.load_roadmap() {
        Ok(r) => r,
        // Unparseable roadmap: doctor owns the data-file health checks;
        // a verifier that cannot parse must not fabricate findings.
        Err(_) => return,
    };
    // Absent or empty roadmap: nothing declared, nothing to police.
    if roadmap.is_empty() {
        return;
    }

    for problem in roadmap.coherence() {
        out.push(violation(
            Severity::Error,
            problem,
            "fix .harna/roadmap.toml — one id per milestone, states spelled from declared|current|done|dropped",
        ));
    }

    // Deferred reviews keyed to milestones: pointers must resolve, and
    // a reached milestone's open reviews are due right now (Info: the
    // promise arrived, not a violation of anything).
    let deferred = store.load_deferred().unwrap_or_default();
    for e in &deferred.entries {
        if review_kind(&e.review) != ReviewKind::Milestone {
            continue;
        }
        match roadmap.find(&e.review) {
            None => out.push(violation(
                Severity::Error,
                format!(
                    "deferred `{}` reviews `{}`, but no such milestone is declared",
                    e.id, e.review
                ),
                "name a declared milestone id, or a date ISO / a version — ghosts are promises nobody can keep",
            )),
            Some(m) if m.state == "done" => out.push(violation(
                Severity::Info,
                format!(
                    "milestone `{}` is done: deferred `{}` keyed to it is due — collect or re-record (harna deferred remove/add)",
                    e.review, e.id
                ),
                "the review trigger arrived; close the entry or move it to a live trigger",
            )),
            Some(_) => {
                // Declared, not yet reached: the pointer resolves and
                // stays dormant. Silent by design.
            }
        }
    }
}

fn violation(severity: Severity, message: String, hint: &str) -> Violation {
    Violation {
        check: "roadmap".into(),
        file: ".harna/roadmap.toml".into(),
        line: 0,
        message,
        hint: hint.into(),
        severity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::registers::deferred::DeferredRegister;

    fn store_with(tag: &str) -> (std::path::PathBuf, Store) {
        let dir =
            std::env::temp_dir().join(format!("harna-roadmap-check-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(dir.join(".harna"));
        let store = Store::open(&dir).unwrap();
        store.init().unwrap();
        (dir, store)
    }

    fn only(v: &[Violation]) -> Vec<Violation> {
        v.iter().filter(|x| x.check == "roadmap").cloned().collect()
    }

    fn hit(v: &[Violation], needle: &str) -> bool {
        v.iter().any(|x| x.message.contains(needle))
    }

    #[test]
    fn absent_or_empty_roadmap_is_silence() -> anyhow::Result<()> {
        let (dir, store) = store_with("absent");
        let mut v = Vec::new();
        check(&store, &mut v);
        assert!(
            only(&v).is_empty(),
            "absence is a valid oath: {}",
            only(&v).len()
        );
        store.save_roadmap(&Default::default())?;
        let mut v2 = Vec::new();
        check(&store, &mut v2);
        assert!(only(&v2).is_empty());
        let _ = std::fs::remove_dir_all(dir.join(".harna"));
        Ok(())
    }

    #[test]
    fn sound_roadmap_resolves_ghosts_and_grades_dues() -> anyhow::Result<()> {
        let (dir, store) = store_with("sound");
        let mut r = crate::schema::registers::roadmap::Roadmap::default();
        r.upsert(m(roadmap_milestone("0.3.27"), "done"));
        r.upsert(m(roadmap_milestone("archive-viewer"), "done"));
        r.upsert(m(roadmap_milestone("0.4.0"), "declared"));
        store.save_roadmap(&r)?;

        let mut deferred = DeferredRegister::default();
        deferred.upsert(deferred_entry("clean", "archive-viewer"));
        deferred.upsert(deferred_entry("dormant", "0.4.0"));
        deferred.upsert(deferred_entry("ghost", "phase-x"));
        store.save_deferred(&deferred)?;

        let mut v = Vec::new();
        check(&store, &mut v);
        let hits = only(&v);
        assert!(
            hit(&hits, "is done: deferred `clean` keyed to it is due"),
            "{hits:#?}"
        );
        assert!(!hit(&hits, "dormant"), "declared-and-undone stays silent");
        assert!(hit(&hits, "no such milestone is declared"));
        assert!(hits
            .iter()
            .all(|x| x.severity == Severity::Error || x.severity == Severity::Info));
        let _ = std::fs::remove_dir_all(dir.join(".harna"));
        Ok(())
    }

    #[test]
    fn incoherent_roadmap_maps_to_its_own_errors() -> anyhow::Result<()> {
        let (dir, store) = store_with("incoherent");
        let mut r = crate::schema::registers::roadmap::Roadmap::default();
        r.milestones.push(roadmap_milestone("0.3.28"));
        r.milestones[0].state = "current".into();
        r.milestones.push(roadmap_milestone("0.3.28"));
        r.milestones.push(roadmap_milestone("0.3.29"));
        r.milestones.last_mut().unwrap().state = "sometime".into();
        store.save_roadmap(&r)?;

        let mut v = Vec::new();
        check(&store, &mut v);
        let hits = only(&v);
        assert!(hit(&hits, "duplicate milestone id `0.3.28`"));
        assert!(hit(&hits, "unrecognized state `sometime`"));
        assert!(hits.iter().all(|x| x.severity == Severity::Error));
        let _ = std::fs::remove_dir_all(dir.join(".harna"));
        Ok(())
    }

    fn roadmap_milestone(id: &str) -> crate::schema::registers::roadmap::Milestone {
        crate::schema::registers::roadmap::Milestone {
            id: id.into(),
            title: format!("deliver {id}"),
            state: "declared".into(),
            notes: String::new(),
        }
    }

    #[allow(unused, reason = "test helper kept for the next roadmap fixture")]
    fn m(
        base: crate::schema::registers::roadmap::Milestone,
        state: &str,
    ) -> crate::schema::registers::roadmap::Milestone {
        crate::schema::registers::roadmap::Milestone {
            state: state.into(),
            ..base
        }
    }

    fn deferred_entry(id: &str, review: &str) -> crate::schema::registers::deferred::DeferredEntry {
        crate::schema::registers::deferred::DeferredEntry {
            id: id.into(),
            component: "test".into(),
            reason: "recorded".into(),
            review: review.into(),
            recorded: "2026-10-17".into(),
        }
    }
}
