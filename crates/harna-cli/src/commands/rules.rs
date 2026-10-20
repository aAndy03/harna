//! Rule arms: add (with the check-registry validation both edges share),
//! deprecate, activate, remove.

use harna_core::schema::model::rules::Rule;
use harna_core::schema::store::Store;

/// Add a rule; `rule.origin` is stamped here. Validation reads the same
/// check registry as the MCP edge (checks.toml via enforcement::checks).
pub fn add(store: &Store, mut rule: Rule) -> anyhow::Result<()> {
    let mut rules = store.load_rules()?;
    harna_core::enforcement::admission::admit(&rules, &rule, "--")?;
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
    // a plugin rule is switched off through a project override (0.5.9)
    store.adopt_plugin_rule(&mut rules, id);
    rules.deprecate(id, reason.unwrap_or(""))?;
    store.save_rules(&rules)?;
    println!("rule `{id}` deprecated (visible in the file, no longer enforced by agents)");
    Ok(())
}

pub fn activate(store: &Store, id: &str) -> anyhow::Result<()> {
    let mut rules = store.load_rules()?;
    store.adopt_plugin_rule(&mut rules, id);
    rules.activate(id)?;
    // an override that says nothing the pack does not is dropped (0.5.9+),
    // so the pack's later upgrades reach the rule again
    store.drop_redundant_override(&mut rules, id);
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
