//! Component `reference` (0.9.3): harna's public library, documented from
//! the data that defines it. Each page is rendered from its source: checks
//! from the registry, tools from the toolbox catalog, profiles and plugins
//! from the packs shipped with harna. The test below fails when a page in
//! `docs/reference/` no longer matches, so the documentation cannot rot
//! (rule `flow-and-maintenance`); `HARNA_BLESS=1 cargo test` rewrites it.

use std::fmt::Write as _;

/// (file name under docs/reference/, its markdown).
pub fn pages() -> Vec<(&'static str, String)> {
    vec![
        ("checks.md", checks()),
        ("tools.md", tools()),
        ("profiles.md", profiles()),
        ("plugins.md", plugins()),
    ]
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

pub fn checks() -> String {
    let all = crate::enforcement::checks::all();
    let mut out = format!(
        "# Checks\n\nGenerated from the check registry (`crates/harna-core/src/enforcement/checks.toml`) — do not edit.\n\n\
         {} checks. A rule binds one with `harna add-rule --check <id>`; `limit`, `values` and `pattern` say what the binding may carry. \
         *Advises until bound*: the check reports as info until a live rule binds it.\n\n\
         | Check | Default | Takes | What it holds |\n|---|---|---|---|\n",
        all.len()
    );
    for c in all {
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
        let default = if c.adopted_by_binding {
            format!("{} (advises until bound)", c.severity_word())
        } else {
            c.severity_word().to_string()
        };
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} |",
            c.id,
            default,
            if takes.is_empty() {
                "—".to_string()
            } else {
                takes.join(", ")
            },
            cell(&c.description)
        );
    }
    out
}

pub fn tools() -> String {
    let all = crate::composition::briefing::toolbox::catalog();
    let mut out = format!(
        "# The toolbox\n\nGenerated from `crates/harna-core/src/composition/briefing/toolbox.toml` — do not edit.\n\n\
         {} tools. Every one is optional: harna shows them, the engineer or agent chooses (`harna tools`, MCP `harna://tools`; \
         `place` and `plan` offer the ones that fit).\n",
        all.len()
    );
    for t in all {
        let _ = write!(
            out,
            "\n## {}\n\n{}.\n\n- **Reach for it:** {}\n- **How:** `{}`\n",
            t.title, t.does, t.reach_for, t.how
        );
        if let Some(e) = &t.enforces {
            let _ = writeln!(out, "- **Enforced by:** check `{e}`");
        }
    }
    out
}

pub fn profiles() -> String {
    let all = crate::generation::structure::profile::shipped();
    let mut out = format!(
        "# Profiles\n\nGenerated from the profiles shipped with harna (`crates/harna-core/profiles/`) — do not edit.\n\n\
         A profile is a starting shape for a kind of project: layers, rules, qualities and vocabulary to prune \
         (`harna init --profile <name>`). Your own go in `~/.harna/profiles/`. {} shipped.\n",
        all.len()
    );
    for p in all {
        let _ = write!(
            out,
            "\n## `{}` {}\n\n{}\n",
            p.meta.name,
            if p.meta.version.is_empty() {
                String::new()
            } else {
                format!("v{}", p.meta.version)
            },
            p.meta.description
        );
        if !p.meta.mirrors.is_empty() {
            let m: Vec<String> = p
                .meta
                .mirrors
                .iter()
                .map(|(k, v)| format!("{k} {v}"))
                .collect();
            let _ = writeln!(out, "\n- **Mirrors:** {}", m.join(", "));
        }
        if !p.meta.plugins.is_empty() {
            let _ = writeln!(out, "- **Installs plugins:** {}", p.meta.plugins.join(", "));
        }
        let file = |name: &str| {
            p.files
                .iter()
                .find(|(f, _)| f == name)
                .map(|(_, c)| c.as_str())
        };
        if let Some(m) = file("manifest.toml")
            .and_then(|raw| toml::from_str::<crate::schema::model::manifest::Manifest>(raw).ok())
        {
            let _ = writeln!(out, "\n| Layer | May import | Containers |\n|---|---|---|");
            for l in &m.layers {
                let _ = writeln!(
                    out,
                    "| `{}` | {} | {} |",
                    l.name,
                    if l.depends_on.is_empty() {
                        "—".to_string()
                    } else {
                        l.depends_on.join(", ")
                    },
                    l.containers
                        .iter()
                        .map(|c| format!("`{}`", c.name))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        if let Some(r) = file("rules.toml")
            .and_then(|raw| toml::from_str::<crate::schema::model::rules::Rules>(raw).ok())
        {
            let _ = writeln!(out, "\n**Rules:**\n");
            for rule in &r.rules {
                let _ = writeln!(out, "- `{}` — {}", rule.id, rule.text);
            }
        }
        if !p.meta.next.is_empty() {
            let _ = writeln!(out, "\n**After init:**\n");
            for (i, n) in p.meta.next.iter().enumerate() {
                let _ = writeln!(out, "{}. {}", i + 1, n);
            }
        }
    }
    out
}

pub fn plugins() -> String {
    let all = crate::generation::structure::plugin::shipped();
    let mut out = format!(
        "# Plugins\n\nGenerated from the packs shipped with harna (`crates/harna-core/plugins/`) — do not edit.\n\n\
         A plugin is a framework's conventions as a versioned data pack (`harna plugin add <name>`), vendored into \
         `.harna/plugins/` and applied as `<plugin>/<id>`. It binds harna's generic checks; it ships no code. {} shipped.\n",
        all.len()
    );
    for p in all {
        let _ = write!(
            out,
            "\n## `{}` {}\n\n{}\n",
            p.meta.name,
            if p.meta.version.is_empty() {
                String::new()
            } else {
                format!("v{}", p.meta.version)
            },
            p.meta.description
        );
        if !p.meta.mirrors.is_empty() {
            let m: Vec<String> = p
                .meta
                .mirrors
                .iter()
                .map(|(k, v)| format!("{k} {v}"))
                .collect();
            let _ = writeln!(
                out,
                "\n- **Mirrors:** {} (the doctor warns when `Cargo.lock` outruns it)",
                m.join(", ")
            );
        }
        if !p.meta.capabilities.is_empty() {
            let caps: Vec<String> = p
                .meta
                .capabilities
                .keys()
                .map(|c| format!("`{}:{c}`", p.meta.name))
                .collect();
            let _ = writeln!(
                out,
                "- **Capabilities** (one container owns each, `--owns`): {}",
                caps.join(", ")
            );
        }
        if !p.meta.bridges.is_empty() {
            let b: Vec<String> = p
                .meta
                .bridges
                .iter()
                .map(|b| format!("`{}`", b.name))
                .collect();
            let _ = writeln!(
                out,
                "- **Bridges** (Rust↔TS edges in the map): {}",
                b.join(", ")
            );
        }
        let rules = p
            .files
            .iter()
            .find(|(f, _)| f == "rules.toml")
            .and_then(|(_, raw)| toml::from_str::<crate::schema::model::rules::Rules>(raw).ok());
        if let Some(r) = rules {
            let _ = writeln!(out, "\n| Rule | Check | What it says |\n|---|---|---|");
            for rule in &r.rules {
                let _ = writeln!(
                    out,
                    "| `{}/{}` | {} | {} |",
                    p.meta.name,
                    rule.id,
                    rule.check
                        .as_deref()
                        .map(|c| format!("`{c}`"))
                        .unwrap_or_else(|| "—".into()),
                    cell(&rule.text)
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    /// 0.9.3: docs/reference/ is what the data says. `HARNA_BLESS=1 cargo
    /// test` rewrites the pages after a deliberate change.
    #[test]
    fn reference_pages_match_their_data() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference");
        let bless = std::env::var_os("HARNA_BLESS").is_some();
        for (name, text) in super::pages() {
            let path = dir.join(name);
            if bless {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(&path, &text).unwrap();
                continue;
            }
            let on_disk = std::fs::read_to_string(&path)
                .unwrap_or_default()
                .replace("\r\n", "\n");
            assert!(
                on_disk == text,
                "docs/reference/{name} is stale — run `HARNA_BLESS=1 cargo test -p harna-core reference` and commit"
            );
        }
    }
}
