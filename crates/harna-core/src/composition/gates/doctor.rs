//! Component `doctor`: the installation self-audit.
//!
//! One command answers "is harna healthy in this repo?" — schema currency,
//! data-file parseability, map freshness, rules, git, hook gate, agent
//! wiring. Every check says one word and offers the next action; FAIL only
//! when the installation genuinely blocks work (scripts can gate on it).

use std::path::Path;

use serde::Serialize;

use crate::schema::store::Store;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Grade {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckReport {
    /// short check id, e.g. "schema".
    pub check: String,
    pub grade: Grade,
    pub message: String,
    /// Suggested next action when grade != Ok.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub action: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub version: String,
    pub checks: Vec<CheckReport>,
}

impl DoctorReport {
    pub fn fail_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.grade == Grade::Fail)
            .count()
    }

    pub fn warn_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.grade == Grade::Warn)
            .count()
    }

    pub fn render(&self) -> String {
        let mut out = format!("harna doctor (tool {})\n", self.version);
        for c in &self.checks {
            let mark = match c.grade {
                Grade::Ok => "ok  ",
                Grade::Warn => "warn",
                Grade::Fail => "FAIL",
            };
            out.push_str(&format!("  {mark}  {:14} {}\n", c.check, c.message));
            if !c.action.is_empty() {
                out.push_str(&format!("      -> {}\n", c.action));
            }
        }
        out.push_str(&format!(
            "\nsummary: {} ok, {} warn, {} fail\n",
            self.checks.iter().filter(|c| c.grade == Grade::Ok).count(),
            self.warn_count(),
            self.fail_count()
        ));
        out
    }
}

fn ok(check: &str, message: &str) -> CheckReport {
    CheckReport {
        check: check.into(),
        grade: Grade::Ok,
        message: message.into(),
        action: String::new(),
    }
}

fn warn(check: &str, message: &str, action: &str) -> CheckReport {
    CheckReport {
        check: check.into(),
        grade: Grade::Warn,
        message: message.into(),
        action: action.into(),
    }
}

fn fail(check: &str, message: &str, action: &str) -> CheckReport {
    CheckReport {
        check: check.into(),
        grade: Grade::Fail,
        message: message.into(),
        action: action.into(),
    }
}

/// Binding coherence (doctor level): every bound check id resolves in the
/// registry and every severity value is one of the accepted words. A
/// deprecated rule is dead and owes nothing.
fn rules_coherence(rules: &crate::schema::model::rules::Rules) -> CheckReport {
    use crate::enforcement::checks as registry;

    let mut problems = Vec::new();
    let mut bindings = 0usize;
    for r in &rules.rules {
        if r.is_deprecated() {
            continue;
        }
        if let Some(c) = &r.check {
            bindings += 1;
            if !registry::known(c) {
                problems.push(format!("rule `{}` binds unknown check `{c}`", r.id));
            }
        }
        if let Some(s) = &r.severity {
            if registry::parse_severity(s).is_none() {
                problems.push(format!("rule `{}` sets unknown severity `{s}`", r.id));
            } else if r.check.is_none() && r.pattern.is_none() {
                problems.push(format!("rule `{}` sets severity with no bound check", r.id));
            }
        }
        if let Some(why) = crate::enforcement::validate::meaning::problem(r) {
            problems.push(format!("rule `{}`: {why}", r.id));
        }
        if let Some(p) = &r.pattern {
            if let Err(e) = registry::compile_pattern(p) {
                problems.push(format!(
                    "rule `{}` has an un-compilable pattern `{p}`: {e}",
                    r.id
                ));
            }
            if let Some(c) = &r.check {
                // a check that takes a pattern reads it (1.0.1, proof: code-pattern
                // with its documented pattern was warned about)
                if c != "pattern" && !crate::enforcement::checks::takes_pattern(c) {
                    problems.push(format!(
                        "rule `{}` carries a pattern but binds check `{c}` (pattern rules run the `pattern` check)",
                        r.id
                    ));
                }
            }
        }
    }
    if problems.is_empty() && bindings > 0 {
        ok(
            "rule-bindings",
            &format!("{} check binding(s) resolve", bindings),
        )
    } else if problems.is_empty() {
        ok("rule-bindings", "no bindings (all rules advisory)")
    } else {
        warn(
            "rule-bindings",
            &format!(
                "{} binding problem(s): {}",
                problems.len(),
                problems.join("; ")
            ),
            &format!(
                "fix the check ids / severity values in .harna/rules.toml (registry: {})",
                registry::known_ids_listed()
            ),
        )
    }
}

/// Run every installation check. `root` is the repo root.
pub fn build(store: &Store, root: &Path) -> anyhow::Result<DoctorReport> {
    let mut checks = Vec::new();

    // 1) .harna present; manifest + rules parseable; schema currency.
    if !store.exists() {
        checks.push(fail(
            ".harna",
            "missing — this repo has no harna memory",
            "run `harna init`, then `harna scan`",
        ));
        return Ok(DoctorReport {
            version: env!("CARGO_PKG_VERSION").into(),
            checks,
        });
    }
    checks.push(ok(".harna", "present"));

    // Manifest: parse WITHOUT the schema guard so old/new data speaks here.
    let manifest_raw = std::fs::read_to_string(store.manifest_path()).unwrap_or_default();
    let manifest: Option<crate::schema::model::manifest::Manifest> =
        toml::from_str(&manifest_raw).ok();
    match manifest {
        Some(m) => {
            if m.schema_version > crate::SCHEMA_VERSION {
                checks.push(fail(
                    "schema",
                    &format!(
                        "repo data is schema v{} but this harna knows v{}",
                        m.schema_version,
                        crate::SCHEMA_VERSION
                    ),
                    "update harna before touching this repository",
                ));
            } else if m.schema_version < m.required_schema() {
                checks.push(warn(
                    "schema",
                    &format!(
                        "manifest uses schema v{} fields but is stamped v{} — an older harna would strip them",
                        m.required_schema(),
                        m.schema_version
                    ),
                    "run `harna migrate`",
                ));
            } else {
                checks.push(ok("schema", "current"));
            }
        }
        None => checks.push(fail(
            "manifest",
            "manifest.toml is missing or unparseable",
            "restore the file from git, or re-run `harna init`",
        )),
    }

    // Rules parseable — plus binding coherence against the check registry:
    // the same canonical source the validators, the doctor and the CLI/MCP
    // add-rule edges read, so a drifting hand-edit is visible at session
    // start, not only at the next validate.
    match store.load_rules() {
        Ok(r) => {
            checks.push(ok("rules", &format!("{} recorded", r.rules.len())));
            checks.push(rules_coherence(&r));
        }
        Err(e) => checks.push(fail(
            "rules",
            &format!("rules.toml unparseable: {e:#}"),
            "restore from git or fix the syntax",
        )),
    }

    registers::registers(store, &mut checks)?;
    formats::formats(store, &mut checks)?;
    environment::environment(store, root, &mut checks)?;

    Ok(DoctorReport {
        version: env!("CARGO_PKG_VERSION").into(),
        checks,
    })
}

mod environment;
mod formats;
mod registers;

#[cfg(test)]
mod tests;
