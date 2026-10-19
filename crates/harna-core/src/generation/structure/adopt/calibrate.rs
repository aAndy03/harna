//! Adoption calibration (0.3.44): budgets proposed from the repo's own
//! distribution, not harna's defaults.
//!
//! harna's defaults (600 lines, 8 files per folder) were tuned on harna.
//! On deno they named 247 files on day one, which is a description of the
//! codebase, not a finding. The draft therefore proposes rules whose
//! limits sit at the repo's 95th percentile (never below the defaults).
//! Files past them are then the repo's real outliers, and the budgets
//! tighten later by choice. The proposals print as `harna add-rule`
//! commands; nothing is written.

use std::collections::BTreeMap;

/// One proposed budget rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calibration {
    pub check: &'static str,
    pub limit: usize,
    /// What the percentile was taken over, for the rule's rationale.
    pub basis: String,
}

impl Calibration {
    /// A ready-to-run command: adoption is one paste, not a hand edit.
    pub fn command(&self) -> String {
        format!(
            "harna add-rule --id calibrated-{check} --check {check} --limit {limit} \
             --text \"Budget calibrated at adoption\" \
             --rationale \"adopt calibration: {basis}; tighten by choice\"",
            check = self.check,
            limit = self.limit,
            basis = self.basis
        )
    }
}

pub const DEFAULT_LINES: usize = 600;
pub const DEFAULT_FOLDER_FILES: usize = 8;

/// Budgets from per-file line counts (production files) and the paths.
pub fn calibrate(files: &[(String, usize)]) -> Vec<Calibration> {
    let production: Vec<&(String, usize)> = files
        .iter()
        .filter(|(p, _)| !crate::search::digest::is_test_path(p))
        .collect();
    if production.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();

    let mut lines: Vec<usize> = production.iter().map(|(_, n)| *n).collect();
    let p95_lines = round_up(percentile(&mut lines, 95), 100);
    if p95_lines > DEFAULT_LINES {
        out.push(Calibration {
            check: "component-granularity",
            limit: p95_lines,
            basis: format!("p95 of {} production files", production.len()),
        });
    }

    let mut per_folder: BTreeMap<&str, usize> = BTreeMap::new();
    for (p, _) in &production {
        let (dir, file) = p.rsplit_once('/').unwrap_or((".", p.as_str()));
        if file != "mod.rs" && file != "tests.rs" {
            *per_folder.entry(dir).or_default() += 1;
        }
    }
    let mut counts: Vec<usize> = per_folder.values().copied().collect();
    let p95_files = percentile(&mut counts, 95);
    if p95_files > DEFAULT_FOLDER_FILES {
        out.push(Calibration {
            check: "folder-size",
            limit: p95_files,
            basis: format!("p95 of {} folders", per_folder.len()),
        });
    }
    out
}

fn percentile(values: &mut [usize], p: usize) -> usize {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let idx = ((values.len() * p).div_ceil(100)).clamp(1, values.len()) - 1;
    values[idx]
}

fn round_up(n: usize, step: usize) -> usize {
    n.div_ceil(step) * step
}

#[cfg(test)]
mod tests;
