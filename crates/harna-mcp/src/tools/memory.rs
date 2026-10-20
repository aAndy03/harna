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
            let rules = store.load_effective_rules()?;
            Ok(rules.render_for_agent(scope.as_deref(), env.as_deref()))
        })
    }

    /// Persist a new rule; every future session inherits it.
    #[tool(
        description = "Persist a rule the user stated. Optional: check + severity, limit, values, regex pattern, scope_signature."
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
            scope_signature,
        }): Parameters<AddRuleRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let mut rules = store.load_rules()?;
            let rule = Rule {
                scope_signature,
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
            };
            harna_core::enforcement::admission::admit(&rules, &rule, "")?;
            rules.rules.push(rule);
            store.save_rules(&rules)?;
            Ok(format!(
                "rule '{id}' saved; total rules: {}",
                rules.rules.len()
            ))
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
            // a plugin rule is edited through a project override (0.5.9)
            if action != "remove" {
                store.adopt_plugin_rule(&mut rules, &id);
            }
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
        description = "Record a decision (why it was built this way). supersedes = the older decision file it replaces."
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
