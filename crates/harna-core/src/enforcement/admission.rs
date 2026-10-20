//! Component `admission` (0.5.9): whether a rule may join the project —
//! the one gate the CLI and MCP edges (and plugin packs) share. Split from
//! the check registry so admission may read the engines' compilers while
//! the engines read the registry (no cycle).

use crate::enforcement::checks::{
    compile_pattern, known, known_ids_listed, limit_ids_listed, parse_severity, takes_limit,
    takes_pattern, VALID_SEVERITIES,
};

/// Admission of a new rule (0.4.9): the one gate the CLI and MCP edges
/// share, so a rule refused on one is refused on the other in the same
/// words. `edge` names the flag spelling (`--` on the CLI, none on MCP).
pub fn admit(
    rules: &crate::schema::model::rules::Rules,
    rule: &crate::schema::model::rules::Rule,
    edge: &str,
) -> anyhow::Result<()> {
    if rule.id.trim().is_empty() {
        anyhow::bail!("a rule needs an id");
    }
    if rule.id.contains('/') {
        anyhow::bail!(
            "rule id `{}` contains `/`, which names a plugin's rules (<plugin>/<id>) — to switch one off: harna deprecate-rule {}",
            rule.id,
            rule.id
        );
    }
    if rules.rules.iter().any(|r| r.id == rule.id) {
        anyhow::bail!("rule id '{}' already exists", rule.id);
    }
    if let Some(c) = &rule.check {
        if !known(c) {
            anyhow::bail!(
                "unknown check `{c}` — known (details: `harna checks`): {}",
                known_ids_listed()
            );
        }
    }
    if let Some(s) = &rule.severity {
        if rule.check.is_none() {
            anyhow::bail!("{edge}severity overrides a bound check — add {edge}check first");
        }
        if parse_severity(s).is_none() {
            anyhow::bail!("unknown severity `{s}` — valid: {VALID_SEVERITIES}");
        }
    }
    if let Some(p) = &rule.pattern {
        if let Some(check) = rule.check.as_ref() {
            if check != "pattern" && !takes_pattern(check) {
                anyhow::bail!(
                    "{edge}pattern implies the `pattern` check — the explicit {edge}check `{check}` disagrees"
                );
            }
        }
        if let Err(e) = compile_pattern(p) {
            anyhow::bail!("{edge}pattern is not a valid regex (`{p}`): {e}");
        }
    }
    if rule.limit.is_some() && !rule.check.as_deref().is_some_and(takes_limit) {
        anyhow::bail!(
            "{edge}limit only names budgets — bind with {edge}check one of: {}",
            limit_ids_listed()
        );
    }
    if let Some(why) = crate::enforcement::validate::meaning::problem(rule) {
        anyhow::bail!("{edge}scope-signature: {why}");
    }
    engine_values(rule)
}

/// The engines plugins bind (0.5.9): their values must compile at add
/// time, not first fail at validate.
fn engine_values(rule: &crate::schema::model::rules::Rule) -> anyhow::Result<()> {
    match rule.check.as_deref() {
        Some("code-pattern") => {
            let mut one = crate::schema::model::rules::Rules::default();
            one.rules.push(rule.clone());
            let mut problems = Vec::new();
            crate::enforcement::validate::content::pattern::code::compile(
                &one,
                None,
                &mut problems,
            );
            if let Some(p) = problems.first() {
                anyhow::bail!("{}", p.message);
            }
        }
        Some("registry") => {
            let mut has_engine = false;
            for v in &rule.values {
                let Some((kind, re)) = v.split_once(':') else {
                    anyhow::bail!(
                        "rule `{}`: registry value `{v}` must be declare:/use:/bind:<regex>",
                        rule.id
                    );
                };
                if !matches!(kind, "declare" | "use" | "bind") {
                    anyhow::bail!(
                        "rule `{}`: registry value `{v}` must be declare:/use:/bind:<regex>",
                        rule.id
                    );
                }
                has_engine |= kind != "use";
                let probe = if kind == "use" {
                    re.replace("NAME", "x")
                } else {
                    re.to_string()
                };
                if let Err(e) = regex::Regex::new(&probe) {
                    anyhow::bail!("rule `{}`: {kind}: regex does not compile: {e}", rule.id);
                }
            }
            if !has_engine {
                anyhow::bail!(
                    "rule `{}`: registry needs a declare:<regex> or bind:<regex> value",
                    rule.id
                );
            }
        }
        _ => {}
    }
    Ok(())
}

/// A plugin's rules (0.5.0), admitted before the pack is installed: each
/// bound check must exist in this harna and each meaning scope must work.
pub fn admit_pack(rules: &crate::schema::model::rules::Rules) -> anyhow::Result<()> {
    for r in &rules.rules {
        if let Some(c) = r.check.as_deref() {
            if !known(c) {
                anyhow::bail!(
                    "rule `{}` binds check `{c}`, which this harna does not have — upgrade harna or the plugin",
                    r.id
                );
            }
        }
        if let Some(why) = crate::enforcement::validate::meaning::problem(r) {
            anyhow::bail!("rule `{}`: {why}", r.id);
        }
        if r.id.trim().is_empty() || r.id.contains('/') {
            anyhow::bail!("rule id `{}` must be a plain, non-empty word", r.id);
        }
        if let Some(p) = &r.pattern {
            compile_pattern(p).map_err(|e| anyhow::anyhow!("rule `{}`: pattern: {e}", r.id))?;
        }
        engine_values(r)?;
    }
    let mut ids: Vec<&str> = rules.rules.iter().map(|r| r.id.as_str()).collect();
    ids.sort();
    if let Some(w) = ids.windows(2).find(|w| w[0] == w[1]) {
        anyhow::bail!("rule id `{}` appears twice in the pack", w[0]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0.5.0: every shipped plugin binds only checks this harna has.
    #[test]
    fn shipped_plugins_are_admitted() {
        for pack in crate::generation::structure::plugin::shipped() {
            admit_pack(&pack.rules()).unwrap_or_else(|e| panic!("{}: {e}", pack.meta.name));
        }
        assert!(!crate::generation::structure::plugin::shipped().is_empty());
    }
}
