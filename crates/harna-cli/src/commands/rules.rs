//! Rule arms: add (with the check-registry validation both edges share),
//! deprecate, activate, remove.

use harna_core::schema::model::rules::Rule;
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
                "unknown check `{c}` — known (details: `harna checks`): {}",
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
            if check != "pattern" && !harna_core::enforcement::checks::takes_pattern(check) {
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
        && !rule
            .check
            .as_deref()
            .is_some_and(harna_core::enforcement::checks::takes_limit)
    {
        anyhow::bail!(
            "--limit only names budgets — bind with --check one of: {}",
            harna_core::enforcement::checks::limit_ids_listed()
        );
    }
    rule.origin = format!(
        "user via harna CLI, {}",
        chrono::Local::now().format("%Y-%m-%d")
    );
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

/// `harna checks` (0.3.49): the check registry, as the binding reference
/// `add-rule --check` needs (it was only in an error message).
pub fn checks() {
    for c in harna_core::enforcement::checks::all() {
        let mut takes = Vec::new();
        if c.takes_limit {
            takes.push("limit");
        }
        if c.takes_values {
            takes.push("values");
        }
        if c.takes_pattern {
            takes.push("pattern");
        }
        let takes = if takes.is_empty() {
            String::new()
        } else {
            format!(" (takes {})", takes.join(", "))
        };
        let adopted = if c.adopted_by_binding {
            ", advisory until bound"
        } else {
            ""
        };
        println!(
            "{} [{}{adopted}]{takes}
    {}",
            c.id,
            c.severity_word(),
            c.description
        );
    }
}
