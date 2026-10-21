//! Component `hotspots`: git churn × digest size (0.3.14).
//!
//! The accretion advisory becomes data-driven: instead of a line-count
//! threshold asking "is this file big?", churn × items asks "does this
//! file keep changing while carrying weight?". A big stable file is done;
//! a small file touched every burst is where the design itches.
//!
//! Churn is commits touching the path (`git rev-list --count`); size is
//! the digest's own item count (signatures + inventory — "digest size"
//! from the roadmap, no file reads). Score = churn × items; modules past
//! `max(HOTSPOT_FLOOR, ceil(mean + 2σ))` are flagged (the 0.3.12 outlier
//! discipline, applied to accretion). No git history (or an untracked
//! file) scores 0 — without evidence nothing flags, never a guess.
//!
//! Unlike the line-count guidance, `mod.rs` orchestrators stay eligible:
//! evidence over exemption — a churning 400-line `mod.rs` itches exactly
//! like any other hotspot.
//!
//! The `hotspot` enforcement half lives in `validate/hotspot.rs`: this
//! module measures (search layer); the check turning measures into
//! violations lives in the integrity layer, which may reach search but
//! never the reverse (0.3.14 dogfood: the first cut put `check()` here
//! and `layer-boundary` fired on the search→integrity import — the
//! layering the tool enforces applies to the tool first).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use crate::schema::store::Store;
use crate::search::digest::Digest;

/// One module's accretion evidence: commits touching it × items it carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hotspot {
    pub path: String,
    pub commits: usize,
    pub items: usize,
    pub score: usize,
}

/// Smallest score that ever flags: below this a module is ordinary no
/// matter what the statistics say (5 commits × 10 items earns attention;
/// anything quieter is a calm file in a calm repo).
pub const HOTSPOT_FLOOR: usize = 50;

/// Commits a module needs before it can be a hotspot: churn is repeated
/// change, not the burst that created the file (0.3.50).
pub const MIN_CHURN: usize = 5;

/// Ranked accretion evidence, busiest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotspotReport {
    /// Top 10 by score (the check, not the display, decides flagging).
    pub hotspots: Vec<Hotspot>,
    pub threshold: usize,
    pub total_scored: usize,
    /// The clone has no history to measure churn from (0.3.46, deno study
    /// A4: on a depth-1 clone every module scored "1 commit").
    #[serde(default)]
    pub shallow: bool,
}

impl HotspotReport {
    /// Token-efficient rendering for humans and agents.
    pub fn render(&self) -> String {
        if self.shallow {
            return "hotspots: no churn evidence — shallow clone (`git fetch --unshallow` to rank)\n"
                .into();
        }
        if self.hotspots.is_empty() {
            return format!(
                "hotspots: none past the threshold ({}) across {} scored module(s)\n",
                self.threshold, self.total_scored
            );
        }
        let mut out = format!(
            "hotspots: top {} by churn × items (threshold {})\n",
            self.hotspots.len(),
            self.threshold
        );
        for h in &self.hotspots {
            out.push_str(&format!(
                "  {}: {} commits × {} items = {}\n",
                h.path, h.commits, h.items, h.score
            ));
        }
        out
    }
}

/// Rank every digest module by accretion evidence, busiest first.
pub fn rank(root: &Path, digest: &Digest) -> Vec<Hotspot> {
    if is_shallow(root) {
        return Vec::new(); // churn × items without churn is just size
    }
    score_modules(&churn_map(root, digest), digest)
}

/// A shallow clone carries a truncated history: churn would read as one
/// commit per file. No evidence, no ranking.
pub fn is_shallow(root: &Path) -> bool {
    std::process::Command::new("git")
        .stdin(std::process::Stdio::null())
        .args(["rev-parse", "--is-shallow-repository"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .is_some_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
}

/// The display report: top 10 plus the flagging threshold.
pub fn build(root: &Path, digest: &Digest) -> HotspotReport {
    let ranked = rank(root, digest);
    HotspotReport {
        threshold: threshold(&ranked.iter().map(|h| h.score).collect::<Vec<_>>()),
        total_scored: ranked.len(),
        hotspots: ranked.into_iter().take(10).collect(),
        shallow: is_shallow(root),
    }
}

/// Measure the committed map behind this store.
pub fn build_store(store: &Store) -> anyhow::Result<HotspotReport> {
    let digest = store
        .load_digest()
        .context("no code map — run `harna scan` first; hotspots read the map, not the files")?;
    Ok(build(&store.repo_root(), &digest))
}

/// Pure scoring: churn per path joined to digest items. Separated from
/// the git read so the ranking pins without a repository.
pub fn score_modules(churn: &BTreeMap<String, usize>, digest: &Digest) -> Vec<Hotspot> {
    let mut out: Vec<Hotspot> = digest
        .modules
        .iter()
        .map(|m| {
            let commits = churn.get(m.path.as_str()).copied().unwrap_or(0);
            let items = m.signatures.len() + m.inventory.len();
            Hotspot {
                path: m.path.clone(),
                commits,
                items,
                score: commits * items,
            }
        })
        .collect();
    out.sort_by(|a, b| b.score.cmp(&a.score).then(a.path.cmp(&b.path)));
    out
}

/// Flagging threshold over scores: statistics with a floor (the 0.3.12
/// outlier discipline — tiny repos must not manufacture hotspots out of
/// two-commit utilities).
pub fn threshold(scores: &[usize]) -> usize {
    if scores.is_empty() {
        return HOTSPOT_FLOOR;
    }
    let n = scores.len() as f64;
    let mean = scores.iter().sum::<usize>() as f64 / n;
    let var = scores
        .iter()
        .map(|s| (*s as f64 - mean).powi(2))
        .sum::<f64>()
        / n;
    (HOTSPOT_FLOOR as f64).max(mean + 2.0 * var.sqrt()).ceil() as usize
}

/// Commits touching each digest module path (all history). No git, or a
/// path git never saw, scores 0 — without evidence nothing flags.
///
/// One `git log --name-only` pass, counted per path (0.3.46, deno study
/// A9: one `git rev-list` per module was 32 of a 74-second validate on
/// 1052 modules).
/// The net line change of each file in its recent commits, newest first
/// (1.1.0, the `growth` check): one `git log --numstat` over the last
/// `commits` commits. Empty without git or history.
pub fn recent_growth(root: &Path, commits: usize) -> BTreeMap<String, Vec<i64>> {
    let mut map: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    let n = format!("-n{commits}");
    let Ok(out) = std::process::Command::new("git")
        .stdin(std::process::Stdio::null())
        .args(["log", &n, "--relative", "--format=", "--numstat", "HEAD"])
        .current_dir(root)
        .output()
    else {
        return map;
    };
    if !out.status.success() {
        return map;
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut parts = line.split('\t');
        let (Some(a), Some(d), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        // binary files show "-"; renames show "{old => new}": skipped
        let (Ok(a), Ok(d)) = (a.parse::<i64>(), d.parse::<i64>()) else {
            continue;
        };
        if path.contains("=>") {
            continue;
        }
        map.entry(path.to_string()).or_default().push(a - d);
    }
    map
}

fn churn_map(root: &Path, digest: &Digest) -> BTreeMap<String, usize> {
    let wanted: std::collections::BTreeSet<&str> =
        digest.modules.iter().map(|m| m.path.as_str()).collect();
    let mut map: BTreeMap<String, usize> = BTreeMap::new();
    let Ok(out) = std::process::Command::new("git")
        .stdin(std::process::Stdio::null())
        .args(["log", "--relative", "--format=", "--name-only", "HEAD"])
        .current_dir(root)
        .output()
    else {
        return map;
    };
    if !out.status.success() {
        return map;
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let path = line.trim();
        if wanted.contains(path) {
            *map.entry(path.to_string()).or_default() += 1;
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::digest::{self, Item, Module};

    fn module(path: &str, sigs: usize, inv: usize) -> Module {
        Module {
            reexports: Vec::new(),
            crate_dir: None,
            src_root: None,
            path: path.into(),
            lang: "rust".into(),
            signatures: (0..sigs).map(|i| format!("fn f{i}()")).collect(),
            deps: vec![],
            body_hash: "x".into(),
            inventory: (0..inv)
                .map(|i| Item {
                    vis: "priv".into(),
                    kind: "fn".into(),
                    name: format!("g{i}"),
                    text: String::new(),
                })
                .collect(),
            mods: vec![],
        }
    }

    fn digest_of(modules: Vec<Module>) -> Digest {
        Digest {
            orphans: Vec::new(),
            generated_at: "now".into(),
            extractor_version: digest::EXTRACTOR_VERSION,
            modules,
        }
    }

    #[test]
    fn scoring_multiplies_and_ranks_busiest_first() {
        let d = digest_of(vec![
            module("a.rs", 10, 0), // 3 commits × 10 items = 30
            module("b.rs", 4, 1),  // 7 commits × 5 items = 35
            module("c.rs", 2, 0),  // 0 commits × 2 items = 0
        ]);
        let churn: BTreeMap<String, usize> = [("a.rs".to_string(), 3), ("b.rs".to_string(), 7)]
            .into_iter()
            .collect();
        let ranked = score_modules(&churn, &d);
        assert_eq!(ranked[0].path, "b.rs");
        assert_eq!(
            (ranked[0].commits, ranked[0].items, ranked[0].score),
            (7, 5, 35)
        );
        assert_eq!(ranked[1].score, 30);
        assert_eq!(ranked[2].score, 0, "no history, no heat");
    }

    #[test]
    fn threshold_floors_and_flags_statistics() {
        assert_eq!(threshold(&[]), HOTSPOT_FLOOR, "empty: the floor");
        assert_eq!(
            threshold(&[0, 0, 3]),
            HOTSPOT_FLOOR,
            "quiet repo: the floor"
        );
        // 12 calm modules at 10, one furnace at 400: mean ≈ 40, σ ≈ 108 →
        // threshold ≈ 256 → only the furnace flags.
        let mut scores = vec![10usize; 12];
        scores.push(400);
        let t = threshold(&scores);
        assert!(t > 100 && t < 400, "statistics, was: {t}");
    }

    #[test]
    fn churn_counts_real_history_and_zeros_the_unknown() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "harna-hotspot-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root)?;
        git(&root, &["init", "-q"]);
        git(
            &root,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "config",
                "user.name",
                "t",
            ],
        );
        std::fs::write(root.join("a.rs"), "one\n")?;
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
                "one",
            ],
        );
        std::fs::write(root.join("a.rs"), "one\ntwo\n")?;
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
                "two",
            ],
        );
        let d = digest::Digest {
            orphans: Vec::new(),
            generated_at: "now".into(),
            extractor_version: digest::EXTRACTOR_VERSION,
            modules: vec![module("a.rs", 1, 0), module("never-committed.rs", 1, 0)],
        };
        let churn = churn_map(&root, &d);
        assert_eq!(churn.get("a.rs"), Some(&2), "one pass, counted per path");
        assert_eq!(churn.get("never-committed.rs"), None);
        assert!(
            churn_map(&root.join("no-repo-here"), &d).is_empty(),
            "no git, no heat"
        );
        assert!(!is_shallow(&root), "a full local history");
        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn empty_digest_scores_nothing() {
        let root = std::env::temp_dir();
        let r = build(&root, &digest_of(vec![]));
        assert_eq!((r.total_scored, r.threshold), (0, HOTSPOT_FLOOR));
        assert!(r.hotspots.is_empty());
        assert!(r.render().contains("none past the threshold"));
    }

    fn git(root: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .stdin(std::process::Stdio::null())
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
