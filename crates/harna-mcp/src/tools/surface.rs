//! The advertised tool surface: what `tools/list` costs an agent.
//!
//! Every session pays for the tool catalog in context before it does any
//! work, so the catalog is shaped once at startup — the logic behind each
//! tool is untouched:
//! - schema boilerplate (`$schema`, `title`) is stripped: clients never
//!   need it, and it repeated on every tool;
//! - `HARNA_TOOLS` optionally narrows what is advertised: unset or `full`
//!   = every tool (the default), `core` = the session ritual, or a
//!   comma-separated list of names (`harna_` prefix optional).

use rmcp::handler::server::router::tool::ToolRouter;

/// The ritual an agent walks every session: orient, plan, scaffold, check,
/// propagate, hand off. Everything else stays reachable with `full`.
pub const CORE: &[&str] = &[
    "harna_onboarding",
    "harna_status",
    "harna_rules",
    "harna_add_rule",
    "harna_log_decision",
    "harna_place",
    "harna_scaffold",
    "harna_dependents",
    "harna_scan",
    "harna_validate",
    "harna_parity_open",
    "harna_parity_resolve",
    "harna_parity_auto",
    "harna_preflight",
    "harna_handoff",
];

/// Which tools a profile advertises; `None` = all of them.
pub fn selection(profile: Option<&str>) -> Option<Vec<String>> {
    let p = profile.map(str::trim).filter(|p| !p.is_empty())?;
    match p {
        "full" | "all" => None,
        "core" => Some(CORE.iter().map(|s| s.to_string()).collect()),
        list => Some(
            list.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| {
                    if s.starts_with("harna_") {
                        s.to_string()
                    } else {
                        format!("harna_{s}")
                    }
                })
                .collect(),
        ),
    }
}

/// Shape a router for advertising: strip schema boilerplate from every
/// tool and keep only the selected ones. Unknown names are reported on
/// stderr (stdout is the protocol) and ignored.
pub fn shape<S: Send + Sync + 'static>(
    router: ToolRouter<S>,
    keep: Option<Vec<String>>,
) -> ToolRouter<S> {
    if let Some(keep) = &keep {
        for name in keep {
            if !router.has_route(name) {
                eprintln!("harna-mcp: HARNA_TOOLS names unknown tool `{name}` — ignored");
            }
        }
    }
    let mut out = ToolRouter::new();
    for mut route in router {
        if keep
            .as_ref()
            .is_some_and(|k| !k.iter().any(|n| n == route.attr.name.as_ref()))
        {
            continue;
        }
        let mut schema = (*route.attr.input_schema).clone();
        schema.remove("$schema");
        schema.remove("title");
        route.attr.input_schema = std::sync::Arc::new(schema);
        out.add_route(route);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_select_without_dropping_logic() {
        assert_eq!(selection(None), None, "default advertises everything");
        assert_eq!(selection(Some("full")), None);
        assert_eq!(selection(Some("  ")), None);
        assert_eq!(selection(Some("core")).unwrap().len(), CORE.len());
        assert_eq!(
            selection(Some("validate, harna_status")).unwrap(),
            vec!["harna_validate".to_string(), "harna_status".to_string()]
        );
    }
}
