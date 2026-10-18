//! Rule arms: add (with the check-registry validation both edges share),
//! deprecate, activate, remove.

use harna_core::schema::rules::Rule;
use harna_core::schema::store::Store;

/// Add a rule; `rule.origin` is stamped here. Validation reads the same
/// check registry as the MCP edge (checks.toml via enforcement::checks).
pub fn add(store: &Store, mut rule: Rule) -> anyhow::Result<()> {
    let mut rules = store.load_rules()?;
    let id = rule.id.clone();
    if rules.rules.iter().any(|r| r.id == id) {
        anyhow::bail!("rule id '{id}' already exists");
    }
    if let Some(c) = &rule.check {
        if !harna_core::enforcement::checks::known(c) {
            anyhow::bail!(
                "unknown check `{c}` — known: {}",
                harna_core::enforcement::checks::known_ids_listed()
            );
        }
    }
    if let Some(s) = &rule.severity {
        if rule.check.is_none() {
            anyhow::bail!("--severity overrides a bound check — add --check first");
        }
        if harna_core::enforcement::checks::parse_severity(s).is_none() {
            anyhow::bail!(
                "unknown severity `{s}` — valid: {}",
                harna_core::enforcement::checks::VALID_SEVERITIES
            );
        }
    }
    if let Some(p) = &rule.pattern {
        if let Some(check) = rule.check.as_ref() {
            if check != "pattern" {
                anyhow::bail!(
                    "--pattern implies the `pattern` check — the explicit --check `{check}` disagrees"
                );
            }
        }
        if let Err(e) = harna_core::enforcement::checks::compile_pattern(p) {
            anyhow::bail!("--pattern is not a valid regex (`{p}`): {e}");
        }
    }
    if rule.limit.is_some()
        && !matches!(rule.check.as_deref(), Some("container-size") | Some("fan-out-cap"))
    {
        anyhow::bail!(
            "--limit only names budgets — bind with --check container-size or --check fan-out-cap"
        );
    }
    rule.origin = format!("user via harna CLI, {}", chrono::Local::now().format("%Y-%m-%d"));
    rules.rules.push(rule);
    store.save_rules(&rules)?;
    println!("rule added; total: {}", rules.rules.len());
    Ok(())
}

pub fn deprecate(store: &Store, id: &str, reason: Option<&str>) -> anyhow::Result<()> {
    let mut rules = store.load_rules()?;
    rules.deprecate(id, reason.unwrap_or(""))?;
    store.save_rules(&rules)?;
    println!("rule `{id}` deprecated (visible in the file, no longer enforced by agents)");
    Ok(())
}

pub fn activate(store: &Store, id: &str) -> anyhow::Result<()> {
    let mut rules = store.load_rules()?;
    rules.activate(id)?;
    store.save_rules(&rules)?;
    println!("rule `{id}` re-activated");
    Ok(())
}

pub fn remove(store: &Store, id: &str) -> anyhow::Result<()> {
    let mut rules = store.load_rules()?;
    rules.remove(id)?;
    store.save_rules(&rules)?;
    println!("rule `{id}` removed (history lives in git)");
    Ok(())
}
