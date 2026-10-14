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
        self.checks.iter().filter(|c| c.grade == Grade::Fail).count()
    }

    pub fn warn_count(&self) -> usize {
        self.checks.iter().filter(|c| c.grade == Grade::Warn).count()
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
        return Ok(DoctorReport { version: env!("CARGO_PKG_VERSION").into(), checks });
    }
    checks.push(ok(".harna", "present"));

    // Manifest: parse WITHOUT the schema guard so old/new data speaks here.
    let manifest_raw = std::fs::read_to_string(store.manifest_path()).unwrap_or_default();
    let manifest: Option<crate::schema::manifest::Manifest> =
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
            } else if m.schema_version < crate::SCHEMA_VERSION {
                checks.push(warn(
                    "schema",
                    &format!(
                        "repo data at schema v{}, tool knows v{}",
                        m.schema_version,
                        crate::SCHEMA_VERSION
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

    // Rules parseable.
    match store.load_rules() {
        Ok(r) => checks.push(ok("rules", &format!("{} recorded", r.rules.len()))),
        Err(e) => checks.push(fail(
            "rules",
            &format!("rules.toml unparseable: {e:#}"),
            "restore from git or fix the syntax",
        )),
    }

    // Decisions directory.
    if store.decisions_dir().is_dir() {
        let count = std::fs::read_dir(store.decisions_dir())
            .map(|d| d.flatten().count())
            .unwrap_or(0);
        checks.push(ok("decisions", &format!("{count} recorded")));
    } else {
        checks.push(warn(
            "decisions",
            "directory missing",
            "log your first decision with `harna log-decision`",
        ));
    }

    // Digest freshness: present, parseable, not outrun by changed files.
    match store.load_digest() {
        Ok(d) => {
            let stamp = chrono::DateTime::parse_from_rfc3339(&d.generated_at).ok();
            let stale_files = stamp
                .map(|st| {
                    crate::search::digest::respected_rust_files(root, 50_000)
                        .into_iter()
                        .filter(|rel| {
                            std::fs::metadata(root.join(rel))
                                .and_then(|m| m.modified())
                                .map(|mt| {
                                    let utc: chrono::DateTime<chrono::Utc> = mt.into();
                                    utc > st.with_timezone(&chrono::Utc)
                                })
                                .unwrap_or(false)
                        })
                        .count()
                })
                .unwrap_or(0);
            if stale_files == 0 {
                checks.push(ok(
                    "digest",
                    &format!("{} modules, current", d.module_count()),
                ));
            } else {
                checks.push(warn(
                    "digest",
                    &format!("{} module(s) changed since the last scan", stale_files),
                    "run `harna scan` (incremental — only the changed files are re-parsed)",
                ));
            }
        }
        Err(_) => checks.push(warn(
            "digest",
            "missing or unparseable",
            "run `harna scan`",
        )),
    }

    // Git: parity, dependents, diff, and --changed all lean on it.
    let git_ok = std::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if git_ok {
        checks.push(ok("git", "repository present"));
    } else {
        checks.push(warn(
            "git",
            "no repository — parity events, dependents diff and --changed need it",
            "run `git init` and commit .harna/ with the code it describes",
        ));
    }

    // Commit gate: does the pre-commit hook invoke harna?
    if git_ok {
        let hooks_path = std::process::Command::new("git")
            .args(["config", "core.hooksPath"])
            .current_dir(root)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        let hook_dir = hooks_path.unwrap_or_default();
        let hook_file = if hook_dir.is_empty() {
            root.join(".git/hooks/pre-commit")
        } else {
            root.join(&hook_dir).join("pre-commit")
        };
        match std::fs::read_to_string(&hook_file) {
            Ok(body) if body.contains("harna") || body.contains("harna.exe") => {
                checks.push(ok("commit-gate", "pre-commit hook wired to harna"));
            }
            _ => checks.push(warn(
                "commit-gate",
                "no pre-commit hook invoking harna",
                "wire `.githooks/pre-commit` to `harna validate --changed` and set core.hooksPath",
            )),
        }
    }

    // Agent wiring: is harna referenced by a local MCP config?
    let wired = ["opencode.json", ".mcp.json"]
        .iter()
        .map(|f| root.join(f))
        .any(|p| {
            std::fs::read_to_string(p)
                .map(|body| body.contains("harna"))
                .unwrap_or(false)
        });
    if !wired {
        checks.push(warn(
            "agent-wiring",
            "no local MCP config mentions harna",
            "add the harna MCP server to opencode.json / .mcp.json so agents load the tools",
        ));
    } else {
        checks.push(ok("agent-wiring", "local MCP config mentions harna"));
    }

    Ok(DoctorReport { version: env!("CARGO_PKG_VERSION").into(), checks })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::store::Store;

    #[test]
    fn fresh_repo_is_healthy_with_expected_warnings() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!("harna-doctor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();

        let r = build(&store, &tmp)?;
        let find = |name: &str| r.checks.iter().find(|c| c.check == name).unwrap();
        assert_eq!(find(".harna").grade, Grade::Ok);
        assert_eq!(find("schema").grade, Grade::Ok);
        assert_eq!(find("rules").grade, Grade::Ok);
        assert_eq!(find("digest").grade, Grade::Warn, "no scan yet");
        assert_eq!(r.fail_count(), 0, "a fresh init must not fail");
        Ok(())
    }

    #[test]
    fn corrupted_rules_fail_but_do_not_stop_other_checks() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!("harna-doctor2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        std::fs::write(store.rules_path(), "not = [valid toml {{{{").unwrap();

        let r = build(&store, &tmp)?;
        assert_eq!(r.fail_count(), 1);
        assert_eq!(r.checks.iter().filter(|c| c.check == "rules").count(), 1);
        let rules_check = r.checks.iter().find(|c| c.check == "rules").unwrap();
        assert_eq!(rules_check.grade, Grade::Fail);
        // The doctor still reports the rest.
        assert!(r.checks.iter().any(|c| c.check == "schema"));
        Ok(())
    }

    #[test]
    fn future_schema_is_a_fail_with_update_hint() -> anyhow::Result<()> {
        let tmp = std::env::temp_dir().join(format!("harna-doctor3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let store = Store::open(&tmp).unwrap();
        store.init().unwrap();
        std::fs::write(store.manifest_path(), "schema_version = 999\n").unwrap();

        let r = build(&store, &tmp)?;
        let schema = r.checks.iter().find(|c| c.check == "schema").unwrap();
        assert_eq!(schema.grade, Grade::Fail);
        assert!(schema.action.contains("update harna"));
        Ok(())
    }
}
