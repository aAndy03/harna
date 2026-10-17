//! Component `intent` (0.3.30): truthfulness reconciliation between
//! claims and state.
//!
//! harna accumulates *claims* — the package version, the changelog's top
//! entry, the roadmap's NOW and DONEs, the parity ledger's silence. Each
//! is cheap to write and easy to let drift from what IS. `validate-intent`
//! cross-checks the claims that are mechanically verifiable and names
//! every DRIFT with both sides (what is claimed, what the state says),
//! so a release never asserts coherence it does not have:
//!
//! 1. package-vs-changelog — `Cargo.toml` agrees with `CHANGELOG.md`.
//! 2. package-vs-roadmap — a version-shaped roadmap NOW agrees with the
//!    package version (non-version ids are destinations, not releases,
//!    and are skipped, not forced).
//! 3. done-milestones-tagged — a version-shaped DONE milestone has the
//!    release tag to prove it shipped (outside git: SKIP, unverifiable).
//! 4. release-hygiene — DONEs on the roadmap while parity debts stand
//!    open means shipped-with-unaccounted-change.
//!
//! Skips are reported, never silent: a check that cannot see its signal
//! says SKIP out loud (the preflight precedent). DRIFTs exit 2 —
//! mid-burst drift is normal, claiming coherence with drift is not.

use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::schema::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Verdict {
    Hold,
    Drift,
    Skip,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntentCheck {
    pub check: String,
    pub verdict: Verdict,
    /// What is claimed (the record, the file, the milestone state).
    pub claim: String,
    /// What the state says (the other record, the tag, the ledger).
    pub reality: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntentReport {
    pub checks: Vec<IntentCheck>,
    pub pass: bool,
}

impl IntentReport {
    pub fn render(&self) -> String {
        let mut out = String::from("validate-intent (claims vs state):\n");
        for c in &self.checks {
            let mark = match c.verdict {
                Verdict::Hold => "HOLD",
                Verdict::Drift => "DRIFT",
                Verdict::Skip => "SKIP",
            };
            out.push_str(&format!("  [{mark}] {}: ", c.check));
            match c.verdict {
                Verdict::Hold => out.push_str(&format!("{} ✓\n", c.claim)),
                Verdict::Drift => out.push_str(&format!(
                    "claims {}; state says {}\n",
                    c.claim, c.reality
                )),
                Verdict::Skip => out.push_str(&format!("{} — {}\n", c.claim, c.reality)),
            }
        }
        let drifts = self.checks.iter().filter(|c| c.verdict == Verdict::Drift).count();
        if drifts == 0 {
            out.push_str("validate-intent: all held — the records agree with the state\n");
        } else {
            out.push_str(&format!(
                "validate-intent: {drifts} DRIFT(s) — reconcile before claiming coherence\n"
            ));
        }
        out
    }
}

pub fn build(store: &Store, root: &Path) -> anyhow::Result<IntentReport> {
    let mut checks = Vec::new();

    let package = package_version(root);
    let changelog = changelog_top(root);
    let roadmap = store.load_roadmap().unwrap_or_default();

    // 1) Package vs changelog: the two places a version is asserted.
    match (&package, &changelog) {
        (Some(p), Some(c)) => checks.push(if p == c {
            hold("package-vs-changelog", format!("version {p} in Cargo.toml and CHANGELOG.md"))
        } else {
            drift(
                "package-vs-changelog",
                format!("version {p} (Cargo.toml)"),
                format!("CHANGELOG.md top entry is {c}"),
            )
        }),
        _ => checks.push(skip(
            "package-vs-changelog",
            "no Cargo.toml/CHANGELOG.md pair",
            "nothing to reconcile",
        )),
    }

    // 2) Package vs roadmap NOW, when NOW is version-shaped.
    match (roadmap.current(), &package) {
        (Some(now), Some(p)) if looks_like_version(&now.id) => checks.push(if now.id == *p {
            hold("package-vs-roadmap", format!("roadmap NOW {} matches package {p}", now.id))
        } else {
            drift(
                "package-vs-roadmap",
                format!("roadmap NOW is {} ({})", now.id, now.title),
                format!("package version is {p}"),
            )
        }),
        (Some(now), _) if !looks_like_version(&now.id) => checks.push(skip(
            "package-vs-roadmap",
            format!("roadmap NOW `{}` is a destination, not a release", now.id),
            "non-version ids are not releases",
        )),
        _ => checks.push(skip(
            "package-vs-roadmap",
            "no current milestone (or no package version)",
            "nothing to reconcile",
        )),
    }

    // 3) DONE milestones carry release tags — done means shipped.
    let done_versions: Vec<_> = roadmap
        .milestones
        .iter()
        .filter(|m| m.state == "done" && looks_like_version(&m.id))
        .collect();
    if done_versions.is_empty() {
        checks.push(skip(
            "done-milestones-tagged",
            "no version-shaped DONE milestones",
            "nothing to prove",
        ));
    } else if !in_git(root) {
        checks.push(skip(
            "done-milestones-tagged",
            format!("{} DONE milestone(s), outside git", done_versions.len()),
            "tags unverifiable without a repository",
        ));
    } else {
        for m in done_versions {
            checks.push(if has_tag(root, &m.id) {
                hold("done-milestones-tagged", format!("{} shipped (tag present)", m.id))
            } else {
                drift(
                    "done-milestones-tagged",
                    format!("milestone {} is DONE ({})", m.id, m.title),
                    format!("no v{0}/{0} tag — done without a release tag is a claim", m.id),
                )
            });
        }
    }

    // 4) Release hygiene: DONEs beside open parity debts.
    let done_count = roadmap.milestones.iter().filter(|m| m.state == "done").count();
    if done_count == 0 {
        checks.push(skip(
            "release-hygiene",
            "no DONE milestones",
            "nothing shipped to keep clean",
        ));
    } else {
        let open: usize = crate::enforcement::parity::list_events(store)
            .unwrap_or_default()
            .iter()
            .map(|e| e.open_targets().len())
            .sum();
        checks.push(if open == 0 {
            hold(
                "release-hygiene",
                format!("{done_count} DONE milestone(s), parity ledger silent"),
            )
        } else {
            drift(
                "release-hygiene",
                format!("{done_count} milestone(s) DONE"),
                format!("{open} open parity target(s) — shipped with unaccounted change"),
            )
        });
    }

    let pass = checks.iter().all(|c| c.verdict != Verdict::Drift);
    Ok(IntentReport { checks, pass })
}

fn hold(check: &str, claim: String) -> IntentCheck {
    IntentCheck {
        check: check.into(),
        verdict: Verdict::Hold,
        claim,
        reality: String::new(),
    }
}

fn drift(check: &str, claim: String, reality: String) -> IntentCheck {
    IntentCheck {
        check: check.into(),
        verdict: Verdict::Drift,
        claim,
        reality,
    }
}

fn skip(check: &str, claim: impl Into<String>, reality: impl Into<String>) -> IntentCheck {
    IntentCheck {
        check: check.into(),
        verdict: Verdict::Skip,
        claim: claim.into(),
        reality: reality.into(),
    }
}

fn package_version(root: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    let v: toml::Value = toml::from_str(&raw).ok()?;
    // Crate root ([package]) first, workspace root ([workspace.package])
    // second — harna itself versions from the workspace.
    v.get("package")
        .and_then(|p| p.get("version"))
        .or_else(|| v.get("workspace")?.get("package")?.get("version"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn changelog_top(root: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(root.join("CHANGELOG.md")).ok()?;
    raw.lines().find_map(|l| {
        let t = l.trim().strip_prefix("##")?.trim();
        if t.starts_with('#') {
            return None; // ### sub-headers are not releases
        }
        t.split_whitespace().next().map(str::to_string)
    })
}

/// "0.4" / "0.3.28": two or three dot-separated digit runs. Anything
/// else is a destination id, never a release.
fn looks_like_version(s: &str) -> bool {
    let parts: Vec<&str> = s.trim().split('.').collect();
    (parts.len() == 2 || parts.len() == 3)
        && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

fn in_git(root: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn has_tag(root: &Path, id: &str) -> bool {
    for cand in [format!("v{id}"), id.to_string()] {
        let out = Command::new("git")
            .args(["tag", "--list", &cand])
            .current_dir(root)
            .output();
        if let Ok(o) = out {
            let text = String::from_utf8_lossy(&o.stdout);
            if text.lines().any(|l| l.trim() == cand) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_with(tag: &str, cargo: Option<&str>, changelog: Option<&str>) -> (std::path::PathBuf, Store) {
        let dir = std::env::temp_dir().join(format!("harna-intent-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if let Some(c) = cargo {
            std::fs::write(dir.join("Cargo.toml"), c).unwrap();
        }
        if let Some(c) = changelog {
            std::fs::write(dir.join("CHANGELOG.md"), c).unwrap();
        }
        let store = Store::open(&dir).unwrap();
        store.init().unwrap();
        (dir, store)
    }

    const CARGO_30: &str = "[package]\nname = \"demo\"\nversion = \"0.3.30\"\n";
    const LOG_30: &str = "# Changelog\n\n## 0.3.30 - 2026-10-17\n\n### Added\n\n- things\n";
    const WORKSPACE_CARGO_30: &str =
        "[workspace]\nresolver = \"2\"\nmembers = [\"demo\"]\n\n[workspace.package]\nversion = \"0.3.30\"\n";

    #[test]
    fn coherent_records_hold() {
        let (dir, store) = repo_with("hold", Some(CARGO_30), Some(LOG_30));
        let r = build(&store, &dir).unwrap();
        assert!(r.pass, "{}", r.render());
        let vc = r.checks.iter().find(|c| c.check == "package-vs-changelog").unwrap();
        assert_eq!(vc.verdict, Verdict::Hold);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn workspace_roots_version_from_workspace_package() {
        // harna itself versions this way: [workspace.package], crates
        // inherit. The reconciliation must read the version where it is.
        let (dir, store) = repo_with("workspace", Some(WORKSPACE_CARGO_30), Some(LOG_30));
        let r = build(&store, &dir).unwrap();
        let vc = r.checks.iter().find(|c| c.check == "package-vs-changelog").unwrap();
        assert_eq!(vc.verdict, Verdict::Hold, "{}", r.render());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn version_mismatch_is_a_named_drift() {
        let (dir, store) = repo_with(
            "drift",
            Some(CARGO_30),
            Some(LOG_30.replace("0.3.30 - ", "0.3.29 - ").as_str()),
        );
        let r = build(&store, &dir).unwrap();
        assert!(!r.pass);
        let vc = r.checks.iter().find(|c| c.check == "package-vs-changelog").unwrap();
        assert_eq!(vc.verdict, Verdict::Drift);
        assert!(vc.claim.contains("0.3.30"), "{}", vc.claim);
        assert!(vc.reality.contains("0.3.29"), "{}", vc.reality);
        assert!(r.render().contains("[DRIFT] package-vs-changelog"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_records_skip_loudly() {
        let (dir, store) = repo_with("skip", None, None);
        let r = build(&store, &dir).unwrap();
        assert!(r.pass, "skips never block");
        assert!(r
            .checks
            .iter()
            .any(|c| c.verdict == Verdict::Skip && c.check == "package-vs-changelog"));
        // No DONE milestones either: both release checks skip.
        assert!(r.checks.iter().any(|c| c.check == "done-milestones-tagged" && c.verdict == Verdict::Skip));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn roadmap_now_must_agree_with_the_package_when_version_shaped() {
        let (dir, store) = repo_with("roadmap", Some(CARGO_30), Some(LOG_30));
        let mut roadmap = crate::schema::roadmap_data::Roadmap::default();
        roadmap.upsert(crate::schema::roadmap_data::Milestone {
            id: "0.3.31".into(),
            title: "seams".into(),
            state: "current".into(),
            notes: String::new(),
        });
        store.save_roadmap(&roadmap).unwrap();
        let r = build(&store, &dir).unwrap();
        let c = r.checks.iter().find(|c| c.check == "package-vs-roadmap").unwrap();
        assert_eq!(c.verdict, Verdict::Drift, "NOW 0.3.31 vs package 0.3.30");
        assert!(c.reality.contains("0.3.30"));

        // A destination id is not a release: skipped, never forced.
        roadmap.milestones[0].id = "phase-h".into();
        store.save_roadmap(&roadmap).unwrap();
        let r2 = build(&store, &dir).unwrap();
        assert_eq!(
            r2.checks.iter().find(|c| c.check == "package-vs-roadmap").unwrap().verdict,
            Verdict::Skip
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn version_shapes_are_pinned() {
        assert!(looks_like_version("0.3.30"));
        assert!(looks_like_version("0.4"));
        assert!(!looks_like_version("phase-h"));
        assert!(!looks_like_version("0.3.30-rc1"));
        assert!(!looks_like_version("0"));
        assert!(!looks_like_version(""));
        assert!(!looks_like_version("2026-10-17"), "dates are not releases");
    }
}
