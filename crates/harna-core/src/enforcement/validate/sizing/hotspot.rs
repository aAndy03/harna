//! Component `validate-hotspot`: the `hotspot` check (0.3.14).
//!
//! The enforcement half of the hotspots view: modules past the churn ×
//! items threshold are accretion candidates with evidence attached. The
//! measurement (`rank`, `threshold`) lives in `search::hotspots`; this
//! component turns measures into violations — integrity may reach search,
//! never the reverse (the first 0.3.14 cut put this check in the search
//! module and `layer-boundary` fired on the upward import; the layering
//! the tool enforces applies to the tool first).

use std::path::Path;

use super::{Severity, Violation};
use crate::search::digest::Digest;
use crate::search::hotspots;

/// The `hotspot` check (Info): needs the digest (stale-data honesty like
/// every graph check) and git history; without either the ranking is
/// empty and nothing fires.
pub fn check(root: &Path, digest: Option<&Digest>, out: &mut Vec<Violation>) {
    let Some(d) = digest else { return };
    let ranked = hotspots::rank(root, d);
    if ranked.is_empty() {
        return;
    }
    let t = hotspots::threshold(&ranked.iter().map(|h| h.score).collect::<Vec<_>>());
    for h in &ranked {
        if h.score <= t {
            continue;
        }
        out.push(Violation {
            check: "hotspot".into(),
            file: h.path.clone(),
            line: 0,
            severity: severity_for(),
            message: format!(
                "hotspot: {} commits × {} items (score {}, threshold {}) — this module keeps changing while carrying weight",
                h.commits, h.items, h.score, t
            ),
            hint: "accretion candidate with evidence: consider promoting it to its own container of components".into(),
        });
    }
}

fn severity_for() -> Severity {
    crate::enforcement::checks::spec("hotspot")
        .map(|s| s.default_severity)
        .unwrap_or(Severity::Info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::digest::{self, Module};

    fn module(path: &str, sigs: usize) -> Module {
        Module {
            path: path.into(),
            lang: "rust".into(),
            signatures: (0..sigs).map(|i| format!("fn f{i}()")).collect(),
            deps: vec![],
            body_hash: "x".into(),
            inventory: vec![],
            mods: vec![],
        }
    }

    fn digest_of(modules: Vec<Module>) -> Digest {
        Digest {
            generated_at: "now".into(),
            extractor_version: digest::EXTRACTOR_VERSION,
            modules,
        }
    }

    #[test]
    fn check_flags_the_furnace_in_a_real_repo() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "harna-hotspot-check-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root)?;
        git(&root, &["init", "-q"]);
        // 12 calm files (10 items) + 1 furnace (40 items), all born together.
        let mut mods = vec![];
        for i in 0..12 {
            let p = format!("calm{i}.rs");
            std::fs::write(root.join(&p), "pub fn calm() {}\n")?;
            mods.push(module(&p, 10));
        }
        std::fs::write(root.join("furnace.rs"), "pub fn burn() {}\n")?;
        mods.push(module("furnace.rs", 40));
        git(&root, &["add", "."]);
        git(
            &root,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "-m",
                "born",
            ],
        );
        // The furnace keeps changing (9 more commits); the calm never do.
        for n in 0..9 {
            std::fs::write(root.join("furnace.rs"), format!("pub fn burn{n}() {{}}\n"))?;
            git(
                &root,
                &[
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "commit",
                    "-q",
                    "-am",
                    &format!("heat {n}"),
                ],
            );
        }
        let d = digest_of(mods);
        let mut v = Vec::new();
        check(&root, None, &mut v);
        assert!(v.is_empty(), "no digest, no heat");
        check(&root, Some(&d), &mut v);
        assert_eq!(v.len(), 1, "only the furnace, was: {v:?}");
        assert_eq!(v[0].check, "hotspot");
        assert_eq!(v[0].file, "furnace.rs");
        assert_eq!(v[0].severity, Severity::Info, "guidance, not a gate");
        assert!(v[0].message.contains("10 commits × 40 items"));
        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    fn git(root: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git should be available for tests");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
