//! Rule arms: add (with the check-registry validation both edges share),
//! deprecate, activate, remove.

use harna_core::schema::model::rules::Rule;
use harna_core::schema::store::Store;

/// Add a rule; `rule.origin` is stamped here. Validation reads the same
/// check registry as the MCP edge (checks.toml via enforcement::checks).
pub fn add(store: &Store, mut rule: Rule, replace: bool) -> anyhow::Result<()> {
    let mut rules = store.load_rules()?;
    rule.origin = format!(
        "user via harna CLI, {}",
        chrono::Local::now().format("%Y-%m-%d")
    );
    let replaced = harna_core::enforcement::admission::place(&mut rules, rule, "--", replace)?;
    store.save_rules(&rules)?;
    println!(
        "rule {}; total: {}",
        if replaced { "replaced" } else { "added" },
        rules.rules.len()
    );
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

/// `harna checks` (0.3.49; summary first 0.9.5): one line per check —
/// id, severity, what it takes; `harna checks <id>` adds what it
/// enforces, `harna checks all` lists every one in full.
pub fn checks(id: Option<&str>) -> anyhow::Result<()> {
    let all = harna_core::enforcement::checks::all();
    let Some(id) = id else {
        println!(
            "{} checks a rule can bind (`--check`); `harna checks <id>` says what one enforces:",
            all.len()
        );
        for c in all {
            println!("  {:<26} [{}]{}", c.id, c.severity_word(), takes_of(c));
        }
        return Ok(());
    };
    let picked: Vec<_> = all.iter().filter(|c| id == "all" || c.id == id).collect();
    if picked.is_empty() {
        anyhow::bail!("no check `{id}` — `harna checks` lists them");
    }
    for c in picked {
        let takes = takes_of(c);
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
    Ok(())
}

fn takes_of(c: &harna_core::enforcement::checks::CheckSpec) -> String {
    let takes: Vec<&str> = [
        (c.takes_limit, "limit"),
        (c.takes_values, "values"),
        (c.takes_pattern, "pattern"),
    ]
    .into_iter()
    .filter_map(|(on, w)| on.then_some(w))
    .collect();
    if takes.is_empty() {
        String::new()
    } else {
        format!(" (takes {})", takes.join(", "))
    }
}
