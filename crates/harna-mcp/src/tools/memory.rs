//! The memory tools: rules (read, add, lifecycle) and the decision log —
//! what the user agreed, persisted so every later session inherits it.

use super::*;

#[tool_router(router = memory_router, vis = "pub(super)")]
impl HarnaTools {
    /// Rules for one scope, or all rules when no scope is given.
    #[tool(
        description = "Rules (with rationale and origin), optionally for one scope path, e.g. 'src/engine'."
    )]
    async fn harna_rules(
        &self,
        Parameters(RulesRequest { scope, env }): Parameters<RulesRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let rules = store.load_rules()?;
            Ok(rules.render_for_agent(scope.as_deref(), env.as_deref()))
        })
    }

    /// Persist a new rule; every future session inherits it.
    #[tool(
        description = "Persist a rule the user stated, so later sessions inherit it. Optional: check id + severity, limit, values, or a regex pattern."
    )]
    async fn harna_add_rule(
        &self,
        Parameters(AddRuleRequest {
            id,
            text,
            rationale,
            scope,
            env,
            check,
            severity,
            pattern,
            limit,
            values,
            command,
        }): Parameters<AddRuleRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let mut rules = store.load_rules()?;
            if rules.rules.iter().any(|r| r.id == id) {
                anyhow::bail!("rule id '{id}' already exists");
            }
            if let Some(c) = &check {
                if !harna_core::enforcement::checks::known(c) {
                    anyhow::bail!(
                        "unknown check `{c}` — known: {}",
                        harna_core::enforcement::checks::known_ids_listed()
                    );
                }
            }
            if let Some(s) = &severity {
                if check.is_none() {
                    anyhow::bail!("severity overrides a bound check — pass check first");
                }
                if harna_core::enforcement::checks::parse_severity(s).is_none() {
                    anyhow::bail!(
                        "unknown severity `{s}` — valid: {}",
                        harna_core::enforcement::checks::VALID_SEVERITIES
                    );
                }
            }
            if let Some(p) = &pattern {
                if let Some(check_id) = check.as_ref() {
                    if check_id != "pattern"
                        && !harna_core::enforcement::checks::takes_pattern(check_id)
                    {
                        anyhow::bail!(
                            "pattern implies the `pattern` check — the explicit check `{check_id}` disagrees"
                        );
                    }
                }
                if let Err(e) = harna_core::enforcement::checks::compile_pattern(p) {
                    anyhow::bail!("pattern is not a valid regex (`{p}`): {e}");
                }
            }
            if limit.is_some()
                && !check.as_deref().is_some_and(harna_core::enforcement::checks::takes_limit)
            {
                anyhow::bail!(
                    "limit only names budgets — bind with check one of: {}",
                    harna_core::enforcement::checks::limit_ids_listed()
                );
            }
            rules.rules.push(Rule {
                id: id.clone(),
                text,
                rationale,
                scope,
                tags: vec![],
                origin: format!("user via harna MCP, {}", today()),
                examples: vec![],
                env,
                check,
                severity,
                pattern,
                limit,
                values,
                command,
                status: None,
            });
            store.save_rules(&rules)?;
            Ok(format!("rule '{id}' saved; total rules: {}", rules.rules.len()))
        })
    }

    /// Rule lifecycle: deprecate, activate, or remove a rule.
    #[tool(
        description = "Rule lifecycle: 'deprecate' (visibly dead, give a reason), 'activate' (undo), 'remove' (git keeps history)."
    )]
    async fn harna_rule_lifecycle(
        &self,
        Parameters(RuleLifecycleRequest { id, action, reason }): Parameters<RuleLifecycleRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let mut rules = store.load_rules()?;
            match action.as_str() {
                "deprecate" => {
                    rules.deprecate(&id, reason.as_deref().unwrap_or(""))?;
                    store.save_rules(&rules)?;
                    Ok(format!(
                        "rule '{id}' deprecated (visible in the file, no longer followed)"
                    ))
                }
                "activate" => {
                    rules.activate(&id)?;
                    store.save_rules(&rules)?;
                    Ok(format!("rule '{id}' re-activated"))
                }
                "remove" => {
                    rules.remove(&id)?;
                    store.save_rules(&rules)?;
                    Ok(format!("rule '{id}' removed (history lives in git)"))
                }
                other => anyhow::bail!("unknown action '{other}': deprecate | activate | remove"),
            }
        })
    }

    /// Persist a decision to .harna/decisions/ as ADR-style markdown.
    #[tool(
        description = "Record a decision (why it was built this way) in .harna/decisions/. supersedes = an older decision file this replaces."
    )]
    async fn harna_log_decision(
        &self,
        Parameters(DecisionRequest {
            title,
            body,
            supersedes,
        }): Parameters<DecisionRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let path = store.save_decision(&title, &body, supersedes.as_deref())?;
            Ok(format!("decision saved to {}", path.display()))
        })
    }
}
