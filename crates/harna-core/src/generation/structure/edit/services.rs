//! Component `services-edit` (0.8.4): every write to `.harna/services.toml`
//! goes through here, from the CLI and from MCP alike. The 0.8 review
//! found the two disagreeing on cleaning and validation, and the field test
//! found data validate would reject accepted at write time. Lists are
//! trimmed and de-duplicated, and what `service-coherence` would reject is
//! refused before it is written.

use std::path::Path;

use crate::schema::model::manifest::Manifest;
use crate::schema::store::services::{Corridor, Service, Services};

/// Trimmed, empties dropped, duplicates removed (first kept).
pub fn clean(v: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in v {
        let s = s.trim().to_string();
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

/// One service's fields to set; None leaves a field as it is.
#[derive(Debug, Default)]
pub struct ServiceEdit {
    pub description: Option<String>,
    pub containers: Option<Vec<String>>,
    pub owns: Option<Vec<String>>,
    pub owners: Option<Vec<String>>,
    pub runs: Option<String>,
}

/// Add (`create`) or change a service. Refuses unknown containers, a
/// container another service runs, a resource without `kind:`, a resource
/// another service owns (case-insensitively), and a deploy folder another
/// service deploys from.
pub fn put_service(
    s: &mut Services,
    manifest: &Manifest,
    name: &str,
    e: ServiceEdit,
    create: bool,
) -> anyhow::Result<String> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("a service needs a name");
    }
    let exists = s.get(name).is_some();
    if create && exists {
        anyhow::bail!("service `{name}` exists — change it with set");
    }
    if !create && !exists {
        anyhow::bail!("no service `{name}` — declare it with add");
    }
    let mut svc = s.get(name).cloned().unwrap_or(Service {
        name: name.to_string(),
        ..Default::default()
    });
    if let Some(d) = e.description {
        svc.description = d.trim().to_string();
    }
    if let Some(c) = e.containers {
        svc.containers = clean(c);
    }
    if let Some(o) = e.owns {
        svc.owns = clean(
            o.iter()
                .map(|r| crate::schema::store::services::normalize_resource(r))
                .collect(),
        );
    }
    if let Some(o) = e.owners {
        svc.owners = clean(o);
    }
    if let Some(r) = e.runs {
        // `.` is the repository itself, kept as written (0.9.2: it was
        // dropped silently)
        let r = r.trim();
        svc.runs = if r == "." || r == "./" {
            ".".into()
        } else {
            crate::schema::model::scope::normalize_scope(r)
        };
    }
    let known = |c: &str| {
        manifest
            .layers
            .iter()
            .flat_map(|l| &l.containers)
            .any(|x| x.name == c)
    };
    let others: Vec<&Service> = s.services.iter().filter(|x| x.name != name).collect();
    for c in &svc.containers {
        if !known(c) {
            anyhow::bail!("no container `{c}` in the manifest (harna container add first)");
        }
        if let Some(o) = others.iter().find(|o| o.containers.contains(c)) {
            anyhow::bail!(
                "container `{c}` already runs in service `{}` — shared code belongs to no service",
                o.name
            );
        }
    }
    for r in &svc.owns {
        match r.split_once(':') {
            Some((k, n)) if !k.trim().is_empty() && !n.trim().is_empty() => {}
            _ => anyhow::bail!("resource `{r}`: write it as kind:name (db:ledger, queue:sync)"),
        }
        if let Some(o) = others.iter().find(|o| {
            o.owns.iter().any(|x| {
                crate::schema::store::services::normalize_resource(x).eq_ignore_ascii_case(r)
            })
        }) {
            anyhow::bail!(
                "resource `{r}` is owned by service `{}` — one service writes it",
                o.name
            );
        }
    }
    if !svc.runs.is_empty() {
        if let Some(o) = others.iter().find(|o| o.runs == svc.runs) {
            anyhow::bail!("service `{}` already deploys from `{}`", o.name, svc.runs);
        }
    }
    let line = svc.line();
    match s.services.iter_mut().find(|x| x.name == name) {
        Some(slot) => *slot = svc, // in place: no reorder churn
        None => s.services.push(svc),
    }
    Ok(line)
}

/// Remove a service; refused while a corridor still names it.
pub fn remove_service(s: &mut Services, name: &str) -> anyhow::Result<String> {
    if s.get(name).is_none() {
        anyhow::bail!("no service `{name}`");
    }
    let joined: Vec<&str> = s
        .corridors
        .iter()
        .filter(|c| c.from.iter().chain(&c.to).any(|x| x == name))
        .map(|c| c.name.as_str())
        .collect();
    if !joined.is_empty() {
        anyhow::bail!(
            "corridor(s) {} still join `{name}` — change or remove them first (harna service corridor add/remove)",
            joined.join(", ")
        );
    }
    s.services.retain(|x| x.name != name);
    Ok(format!("removed service `{name}`"))
}

/// Add or replace a corridor. Refuses unknown services, a missing sender
/// or receiver, a corridor from a service only to itself, two messages of
/// one name, and a shape file that does not exist.
pub fn put_corridor(s: &mut Services, root: &Path, mut c: Corridor) -> anyhow::Result<String> {
    c.name = c.name.trim().to_string();
    c.kind = c.kind.trim().to_string();
    c.from = clean(c.from);
    c.to = clean(c.to);
    if c.name.is_empty() {
        anyhow::bail!("a corridor needs a name");
    }
    if let Some(x) = c.from.iter().chain(&c.to).find(|x| s.get(x).is_none()) {
        anyhow::bail!("no service `{x}` (harna service add first)");
    }
    if c.from.is_empty() || c.to.is_empty() {
        anyhow::bail!(
            "corridor `{}` needs a sender (--from) and a receiver (--to)",
            c.name
        );
    }
    if c.from.iter().all(|f| c.to.contains(f)) && c.to.iter().all(|t| c.from.contains(t)) {
        anyhow::bail!("corridor `{}` joins a service only to itself", c.name);
    }
    let mut names: Vec<&str> = Vec::new();
    for m in &c.messages {
        if names.contains(&m.name.as_str()) {
            anyhow::bail!("corridor `{}` carries `{}` twice", c.name, m.name);
        }
        names.push(&m.name);
        let file = m.type_ref().map(|(f, _)| f).unwrap_or(&m.shape);
        if !root.join(file).is_file() {
            anyhow::bail!("message `{}`: shape file `{file}` does not exist", m.name);
        }
        // the type is declared there (0.9.2: a typo'd type was accepted and
        // only validate said so)
        if let Some((_, ty)) = m.type_ref() {
            let src = std::fs::read_to_string(root.join(file)).unwrap_or_default();
            let declared = regex::Regex::new(&format!(
                r"\b(?:struct|enum|interface|type|class)\s+{}\b",
                regex::escape(ty)
            ))
            .map(|re| re.is_match(&src))
            .unwrap_or(true);
            if !declared {
                anyhow::bail!("message `{}`: `{file}` declares no type `{ty}`", m.name);
            }
        }
    }
    let line = c.line();
    match s.corridors.iter_mut().find(|x| x.name == c.name) {
        Some(slot) => *slot = c,
        None => s.corridors.push(c),
    }
    Ok(line)
}

pub fn remove_corridor(s: &mut Services, name: &str) -> anyhow::Result<String> {
    let before = s.corridors.len();
    s.corridors.retain(|x| x.name != name);
    if s.corridors.len() == before {
        anyhow::bail!("no corridor `{name}`");
    }
    Ok(format!("removed corridor `{name}`"))
}

/// The facets every service answers. A missing list is refused (0.8.4:
/// one bare call switched the release gate off); `""` clears on purpose.
pub fn set_require(s: &mut Services, facets: Option<Vec<String>>) -> anyhow::Result<String> {
    let Some(facets) = facets else {
        anyhow::bail!("name the facets (health,backups,…), or \"\" to require none");
    };
    s.require = clean(
        facets
            .iter()
            .flat_map(|f| f.split(','))
            .map(str::to_string)
            .collect(),
    );
    Ok(if s.require.is_empty() {
        "no readiness facets required — preflight no longer holds a release on them".into()
    } else {
        format!(
            "every service answers: {} ({} answer(s) missing)",
            s.require.join(", "),
            s.unready().len()
        )
    })
}

/// Answer a facet for a service; an empty answer removes it. `n/a` needs
/// its reason: `n/a: stateless`.
pub fn set_ready(
    s: &mut Services,
    service: &str,
    facet: &str,
    answer: &str,
) -> anyhow::Result<String> {
    let facet = facet.trim();
    let answer = answer.trim();
    if facet.is_empty() {
        anyhow::bail!("name the facet");
    }
    let lower = answer.to_ascii_lowercase();
    if !answer.is_empty() && crate::schema::store::services::is_placeholder(answer) {
        anyhow::bail!(
            "`{answer}` answers nothing — say where it lives, or \"n/a: <why>\" (an empty answer removes it)"
        );
    }
    if lower == "n/a"
        || (lower.starts_with("n/a") && lower[3..].trim_start_matches(':').trim().is_empty())
    {
        anyhow::bail!("`n/a` needs its reason: \"n/a: <why>\"");
    }
    let Some(svc) = s.services.iter_mut().find(|x| x.name == service) else {
        anyhow::bail!("no service `{service}`");
    };
    let mut out = if answer.is_empty() {
        svc.ready.remove(facet);
        format!("{service}/{facet}: answer removed")
    } else {
        svc.ready.insert(facet.to_string(), answer.to_string());
        format!("{service}/{facet}: {answer}")
    };
    if !s.require.is_empty() && !s.require.iter().any(|f| f == facet) {
        out.push_str(&format!(
            " (note: `{facet}` is not a required facet: {})",
            s.require.join(", ")
        ));
    }
    Ok(out)
}

/// A container renamed: the services running it follow (0.8.4: they kept
/// the old name and validate failed). Returns whether anything changed.
pub fn rename_container(s: &mut Services, old: &str, new: &str) -> bool {
    let mut changed = false;
    for svc in &mut s.services {
        for c in &mut svc.containers {
            if c == old {
                *c = new.to_string();
                changed = true;
            }
        }
    }
    changed
}

/// What follows a container rename, for the CLI and MCP alike: its
/// contract file and the services running it. Returns the lines to show.
pub fn follow_container_rename(
    store: &crate::schema::store::Store,
    old: &str,
    new: &str,
) -> anyhow::Result<String> {
    let mut out = String::new();
    if store.rename_contract(old, new)? {
        out.push_str(&format!(
            "contract `{old}` follows: .harna/contracts/{new}.toml\n"
        ));
    }
    let mut s = store.load_services()?;
    if rename_container(&mut s, old, new) {
        store.save_services(&s)?;
        out.push_str(&format!(
            "services.toml: the services running `{old}` now run `{new}`\n"
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::manifest::{Container, Layer};
    use crate::schema::store::services::Message;

    /// 0.8.4: what validate would reject is refused at write time, lists
    /// are cleaned, edits stay in place, and a rename follows.
    #[test]
    fn service_writes_refuse_what_validate_rejects() {
        let m = Manifest {
            layers: vec![Layer {
                name: "all".into(),
                containers: ["a", "b"]
                    .iter()
                    .map(|n| Container {
                        name: n.to_string(),
                        path: format!("crates/{n}/src"),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut s = Services::default();
        let e = |c: &[&str], o: &[&str]| ServiceEdit {
            containers: Some(c.iter().map(|x| x.to_string()).collect()),
            owns: Some(o.iter().map(|x| x.to_string()).collect()),
            ..Default::default()
        };
        put_service(&mut s, &m, "api", e(&[" a", "a", ""], &["db:ledger"]), true).unwrap();
        assert_eq!(s.services[0].containers, vec!["a"]);
        assert!(
            put_service(&mut s, &m, "w", e(&["a"], &[]), true).is_err(),
            "a runs in api"
        );
        assert!(put_service(&mut s, &m, "w", e(&["ghost"], &[]), true).is_err());
        assert!(put_service(&mut s, &m, "w", e(&["b"], &["DB:Ledger"]), true).is_err());
        assert!(
            put_service(&mut s, &m, "w", e(&["b"], &["ledger"]), true).is_err(),
            "kind:"
        );
        put_service(&mut s, &m, "w", e(&["b"], &["queue:sync"]), true).unwrap();
        put_service(
            &mut s,
            &m,
            "api",
            ServiceEdit {
                description: Some("d".into()),
                ..Default::default()
            },
            false,
        )
        .unwrap();
        assert_eq!(s.services[0].name, "api", "set edits in place");
        let root = std::env::temp_dir();
        let c = |from: &[&str], to: &[&str]| Corridor {
            name: "p".into(),
            from: from.iter().map(|x| x.to_string()).collect(),
            to: to.iter().map(|x| x.to_string()).collect(),
            ..Default::default()
        };
        assert!(
            put_corridor(&mut s, &root, c(&["api"], &["api"])).is_err(),
            "self"
        );
        assert!(put_corridor(&mut s, &root, c(&["api"], &[])).is_err());
        let mut twice = c(&["api"], &["w"]);
        twice.messages = vec![
            Message::parse("A=x.json").unwrap(),
            Message::parse("A=y.json").unwrap(),
        ];
        assert!(put_corridor(&mut s, &root, twice).is_err());
        let mut missing = c(&["api"], &["w"]);
        missing.messages = vec![Message::parse("no/such/file.rs::T").unwrap()];
        assert!(put_corridor(&mut s, &root, missing).is_err());
        put_corridor(&mut s, &root, c(&["api"], &["w"])).unwrap();
        assert!(remove_service(&mut s, "w").is_err(), "a corridor joins it");
        assert!(set_require(&mut s, None).is_err());
        set_require(
            &mut s,
            Some(vec!["health, backups".into(), "health".into()]),
        )
        .unwrap();
        assert_eq!(s.require, vec!["health", "backups"]);
        assert!(set_ready(&mut s, "api", "backups", "n/a").is_err());
        assert!(set_ready(&mut s, "api", "backups", "N/A:  ").is_err());
        set_ready(&mut s, "api", "backups", "n/a: stateless").unwrap();
        assert!(rename_container(&mut s, "a", "core"));
        assert_eq!(s.services[0].containers, vec!["core"]);
    }
}
