//! Component `gateway` (0.3.10): boundary facilities are owned by the
//! container that declares them.
//!
//! A container may declare `gateway = "fs"`, `gateway = "fs:write"`,
//! `gateway = "net"`, `gateway = "fs:write, process"` in the manifest.
//! Declaring a facility is the adoption switch (data, never a hidden
//! default — same discipline as `provides` and `role = "engine"`): while a
//! facility is unowned, nothing is gated; once it is owned, modules
//! outside the registered gateway(s) may not touch it.
//!
//! - `fs` — local disk IO. The `:write` mode separates the halves: reads
//!   (`std::fs::read`, `fs::metadata`, `File::open`) stay open to
//!   everyone, writes (`fs::write`, `fs::create_dir`, `OpenOptions`,
//!   `tempfile`) are the gateway's monopoly. That is "storage write-only
//!   through its gateway".
//! - `net` — network clients (HTTP and DB-over-wire). No read/write split.
//! - `process` — subprocesses (`std::process`, `tokio::process`).
//! - `base` — an import into the manifest's rank-0 layers (bases) from a
//!   module that is neither in a base layer nor in a `base` gateway
//!   container: the repository pattern, enforced on the committed graph.
//!   A declared `depends_on` does not exempt — the gateway narrows the
//!   base reach from layer granularity to registered containers.
//!
//! The token vocabulary is the tool's own data: a wide-but-finite list of
//! std + common-crate markers, write-side sensitive where the distinction
//! is decidable from source text. A token hits when it does not continue
//! another path (`::fs::read`, `my_std::fs` name different things —
//! `std::fs::read_to_string` still scores its `std::fs`-anchored unions).
//! Violations carry the first token's line, the facility, and the
//! registered gateways. Test sections (`#[cfg(test)]` …) are exempt —
//! fixtures legitimately create files and temp dirs, exactly like the URL
//! check exempts them.
//!
//! Vocabulary sharpening (0.3.10, learned from this repo's own dogfood):
//! the generic `std::io::Write`/`std::io::Seek` tokens were dropped — they
//! also score terminal flushes (`stdout().flush()` needs the `Write` trait
//! in scope), which are not storage; `BufWriter` and the `fs`-anchored
//! words carry the write side without the false positives. 0.3.42 (the
//! font-manager test): `write!` and the bare `std::io` dropped too —
//! `write!(f, …)` in every `impl Display` and `std::io::Error` in every
//! error type are formatting and error plumbing, not storage. Test items
//! are masked per item (`search::digest::live`), so a `#[cfg(test)] use …;` at
//! the top no longer hides the production code below it.
//!
//! Known limitation (pinned by test, like the 0.3.3 brace collapse): the
//! brace form `use std::fs::{write, ...}` hides which symbols the brace
//! opens — the line counts as its READ side (`std::fs` head fires only
//! under a full `fs` gateway, not under `:write`).

use std::path::Path;

use super::{Severity, Violation};
use crate::schema::model::manifest::{effective_role, Manifest};
use crate::search::digest::live::live_code;
use crate::search::digest::{crate_name_dirs, resolve_dep, workspace_crate_names, Digest};

/// fs facility vocabulary: (token, is this write-side?).
const FS_TOKENS: &[(&str, bool)] = &[
    // write side — the storage monopoly lives exactly here
    ("std::fs::write", true),
    ("std::fs::create_dir", true),
    ("std::fs::create_dir_all", true),
    ("std::fs::remove_file", true),
    ("std::fs::remove_dir", true),
    ("std::fs::remove_dir_all", true),
    ("std::fs::rename", true),
    ("std::fs::copy", true),
    ("std::fs::hard_link", true),
    ("std::fs::File::create", true),
    ("std::fs::OpenOptions", true),
    ("std::fs::symlink", true),
    ("std::io::BufWriter", true),
    ("tokio::fs::write", true),
    ("tokio::fs::OpenOptions", true),
    ("tokio::fs::create_dir", true),
    ("tokio::fs::remove", true),
    ("tokio::fs::rename", true),
    ("tokio::fs::copy", true),
    ("tempfile", true),
    // bare-module alias forms (`use std::fs;` then `fs::write(...)`)
    ("fs::write", true),
    ("fs::create_dir", true),
    ("fs::remove_file", true),
    ("fs::remove_dir", true),
    ("fs::rename", true),
    ("fs::copy", true),
    ("fs::hard_link", true),
    ("fs::File::create", true),
    ("fs::OpenOptions", true),
    ("fs::symlink", true),
    ("File::create", true),
    // read side
    ("std::fs", false),
    ("std::io::BufReader", false),
    ("tokio::fs", false),
    ("walkdir", false),
    ("fs::read", false),
    ("fs::read_to_string", false),
    ("fs::read_exact", false),
    ("fs::read_link", false),
    ("fs::metadata", false),
    ("fs::canonicalize", false),
    ("fs::exists", false),
    ("fs::read_dir", false),
    ("File::open", false),
];

const NET_TOKENS: &[&str] = &[
    "std::net",
    "tokio::net",
    "reqwest",
    "ureq",
    "hyper",
    "sqlx",
    "tokio_postgres",
    "postgres::",
    "redis::",
    "mongodb",
    "mysql::",
    "diesel",
    "native_tls",
    "rustls",
];

const PROCESS_TOKENS: &[&str] = &[
    "std::process",
    "tokio::process",
    "std::os::windows::process",
    "std::os::unix::process",
];

/// One registered gateway declaration: facility id + optional `:write`.
#[derive(Debug, PartialEq)]
struct Facility {
    id: &'static str,
    write_only: bool,
}

fn known_facility(id: &str) -> Option<&'static str> {
    match id {
        "fs" => Some("fs"),
        "net" => Some("net"),
        "process" => Some("process"),
        "base" => Some("base"),
        _ => None,
    }
}

/// Parse `gateway = "fs:write, net"`. Malformed parts (unknown facility
/// words, non-`write` modes, empties) come back separately and surface as
/// Info violations — hand-edited data stays visible, never silently
/// half-honored.
fn parse_gateway(raw: &str) -> (Vec<Facility>, Vec<String>) {
    let mut ok = Vec::new();
    let mut bad = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if crate::schema::model::manifest::external_crate(part).is_some() {
            continue; // an external-crate gateway (0.3.48): crates.rs
        }
        if part
            .strip_prefix("npm:")
            .is_some_and(|p| !p.trim().is_empty())
        {
            continue; // a package gateway (0.6.0): boundary/ts.rs
        }
        let (word, mode) = match part.split_once(':') {
            Some((w, m)) => (w.trim(), m.trim()),
            None => (part, ""),
        };
        match known_facility(word) {
            Some(id) => match mode {
                "" => ok.push(Facility {
                    id,
                    write_only: false,
                }),
                m if m.eq_ignore_ascii_case("write") => ok.push(Facility {
                    id,
                    write_only: true,
                }),
                _ => bad.push(format!("{part:?}")),
            },
            None => bad.push(format!("{part:?}")),
        }
    }
    (ok, bad)
}

/// All validly declared gateways across the manifest, in manifest order:
/// (container path, facilities). Wholly malformed declarations keep the
/// container out of enforcement (nothing engages) and stay advisory.
fn declared_gateways(manifest: &Manifest) -> Vec<(&str, Vec<Facility>)> {
    manifest
        .layers
        .iter()
        .flat_map(|l| {
            l.containers
                .iter()
                .map(|c| (c.path.as_str(), c.gateway.as_deref()))
        })
        .filter_map(|(path, raw)| raw.map(|raw| (path, raw)))
        .filter_map(|(path, raw)| {
            let (facilities, _) = parse_gateway(raw);
            if facilities.is_empty() {
                None
            } else {
                Some((path, facilities))
            }
        })
        .collect()
}

/// Declaration coherence (Info): unrecognized facility words and modes are
/// narrated per container, and two containers claiming the same facility
/// are named once (single-owner spirit — allowed, said aloud). Runs
/// without a digest.
pub fn check_declarations(manifest: &Manifest, out: &mut Vec<Violation>) {
    for layer in &manifest.layers {
        for c in &layer.containers {
            let Some(raw) = c.gateway.as_deref() else {
                continue;
            };
            let (_, bad) = parse_gateway(raw);
            for b in &bad {
                out.push(Violation {
                    check: "gateway-ownership".into(),
                    file: ".harna/manifest.toml".into(),
                    line: 0,
                    severity: Severity::Info,
                    message: format!(
                        "container `{}` carries {b} — an unrecognized part of gateway = \"{raw}\"",
                        c.name
                    ),
                    hint: "facilities: fs | net | process | base (optional mode after `:` is `write`), or crate:NAME for an external crate"
                        .into(),
                });
            }
        }
    }
    // (label, declared role) per owner
    type Owner = (String, Option<String>);
    let mut claims: Vec<(&'static str, Vec<Owner>)> = Vec::new();
    // The ownership narration is role-aware (0.3.25): each registered
    // gateway names its container, layer and effective role — multi-owner
    // facilities read as "whose, in what role", not just a count.
    for layer in &manifest.layers {
        for c in &layer.containers {
            let Some(raw) = c.gateway.as_deref() else {
                continue;
            };
            let (facilities, _) = parse_gateway(raw);
            if facilities.is_empty() {
                continue;
            }
            let place = if c.path.is_empty() {
                c.name.as_str()
            } else {
                c.path.as_str()
            };
            let role = effective_role(layer, c).map(|r| r.to_string());
            let label = match &role {
                Some(role) => format!("{place} ({}, role {role})", layer.name),
                None => format!("{place} ({}, no role declared)", layer.name),
            };
            for f in facilities {
                let entry = (label.clone(), role.clone());
                if let Some(claim) = claims.iter_mut().find(|(id, _)| *id == f.id) {
                    claim.1.push(entry);
                } else {
                    claims.push((f.id, vec![entry]));
                }
            }
        }
    }
    for (id, owners) in &claims {
        // owners in different declared roles are the deliberate split the
        // hint describes (1.1.0: it was reported anyway)
        let mut roles: Vec<&str> = owners.iter().filter_map(|(_, r)| r.as_deref()).collect();
        let all_roles = roles.len() == owners.len();
        roles.sort();
        roles.dedup();
        let deliberate = all_roles && roles.len() == owners.len();
        let containers: Vec<&str> = owners.iter().map(|(l, _)| l.as_str()).collect();
        if containers.len() > 1 && !deliberate {
            out.push(Violation {
                check: "gateway-ownership".into(),
                file: ".harna/manifest.toml".into(),
                line: 0,
                severity: Severity::Info,
                message: format!(
                    "facility `{}` has {} registered gateways ({}) — keep one owner unless the split is deliberate",
                    id,
                    containers.len(),
                    containers.join(", ")
                ),
                hint: "one container should own a facility; owners in different declared roles are a deliberate split and pass — give each its `role`".into(),
            });
        }
    }
}

/// The live-boundary check: modules outside the registered gateways may
/// not touch a declared facility (`fs`/`net`/`process` via the sources of
/// the run's file set; `base` via the committed import graph). Without a
/// digest only the base facility degrades; the rest still runs.
pub fn check(
    root: &Path,
    manifest: &Manifest,
    digest: Option<&Digest>,
    srcs: &[(String, String)],
    out: &mut Vec<Violation>,
) {
    crates::check(root, manifest, srcs, out);
    let gateways = declared_gateways(manifest);
    if gateways.is_empty() {
        return; // nothing is owned yet: nothing is gated
    }
    let homes: Vec<String> = manifest
        .layers
        .iter()
        .flat_map(|l| l.containers.iter().map(|c| c.path.clone()))
        .filter(|c| !c.is_empty())
        .collect();

    let fs: Vec<(&str, bool)> = gateways
        .iter()
        .flat_map(|(p, fs)| {
            fs.iter()
                .filter(|f| f.id == "fs")
                .map(move |f| (*p, f.write_only))
        })
        .collect();
    let pick = |id: &'static str| {
        gateways
            .iter()
            .filter(|(_, fs)| fs.iter().any(|f| f.id == id))
            .map(|(p, _)| *p)
            .collect::<Vec<_>>()
    };
    let net = pick("net");
    let process = pick("process");
    let base = pick("base");

    if !fs.is_empty() {
        check_fs(srcs, &fs, &homes, out);
    }
    for (facility, tokens, owned) in [
        ("net", NET_TOKENS, net),
        ("process", PROCESS_TOKENS, process),
    ] {
        if !owned.is_empty() {
            check_other(facility, tokens, srcs, &owned, &homes, out);
        }
    }
    if !base.is_empty() {
        if let Some(d) = digest {
            check_base(root, manifest, d, &base, out);
        }
    }
}

/// Governed iff the file sits inside some manifest container (a nested
/// container inside a gateway counts too — the subtree exemption covers it).
/// Governed iff the file sits inside some manifest container (a nested
/// container inside a gateway counts too — the subtree exemption covers it).
fn any_home(path: &str, container_paths: &[String]) -> bool {
    container_paths
        .iter()
        .any(|c| path.starts_with(&format!("{c}/")))
}

/// The gateway's ownership spans its whole subtree.
fn is_inside(path: &str, gateway_paths: &[&str]) -> bool {
    gateway_paths
        .iter()
        .any(|p| path == *p || path.starts_with(&format!("{p}/")))
}

/// POS where `token` occurs as a real path-word, not as the tail of a
/// longer one (no `::` before, no identifier chars after — `my_std::fs`
/// and `crate::fs::read` name different things).
pub(crate) fn token_at(line: &str, token: &str) -> Option<usize> {
    let mut from = 0usize;
    while from < line.len() {
        let Some(at) = line[from..].find(token) else {
            break;
        };
        let at = from + at;
        let prev_ok = at == 0
            || line[..at]
                .chars()
                .next_back()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == ':'));
        let next_ok = token.contains("::")
            || line[at + token.len()..]
                .chars()
                .next()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        if prev_ok && next_ok {
            return Some(at);
        }
        from = at + token.len();
    }
    None
}

/// First facility marker in the source: (1-based line, token, write-side).
fn first_hit<'t>(src: &str, tokens: &[(&'t str, bool)]) -> Option<(usize, &'t str, bool)> {
    // test items are masked: fixtures may touch every facility
    for (n, line) in live_code(src) {
        for (token, write) in tokens {
            if token_at(line, token).is_some() {
                return Some((n, token, *write));
            }
        }
    }
    None
}

fn check_fs(
    srcs: &[(String, String)],
    gateways: &[(&str, bool)],
    homes: &[String],
    out: &mut Vec<Violation>,
) {
    // Full-fs ownership dominates: with any full `fs` gateway on record
    // even reads are guarded; `:write`-only gateways own the write half.
    let has_full = gateways.iter().any(|(_, wo)| !wo);
    let registered = gateways
        .iter()
        .map(|(p, wo)| {
            if *wo {
                format!("{p} (fs:write)")
            } else {
                format!("{p} (fs)")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let gated_paths: Vec<&str> = gateways.iter().map(|(p, _)| *p).collect();
    for (file, src) in srcs {
        if is_inside(file, &gated_paths) {
            continue; // the gateway's own subtree
        }
        if !any_home(file, homes) {
            continue; // ungoverned module: the map does not reach it (yet)
        }
        if let Some((line, token, write)) = first_hit(src, FS_TOKENS) {
            if !has_full && !write {
                continue; // `:write` ownership: reads stay open
            }
            let facility = if has_full { "fs" } else { "fs (write side)" };
            push(out, file, line, token, facility, &registered);
        }
    }
}

fn check_other(
    facility: &str,
    tokens: &[&str],
    srcs: &[(String, String)],
    gateways: &[&str],
    homes: &[String],
    out: &mut Vec<Violation>,
) {
    let registered = gateways.join(", ");
    for (file, src) in srcs {
        if is_inside(file, gateways) {
            continue;
        }
        if !any_home(file, homes) {
            continue;
        }
        for (n, line) in live_code(src) {
            if let Some(t) = tokens.iter().find(|t| token_at(line, t).is_some()) {
                push(out, file, n, t, facility, &registered);
                break;
            }
        }
    }
}

fn push(
    out: &mut Vec<Violation>,
    file: &str,
    line: usize,
    token: &str,
    facility: &str,
    registered: &str,
) {
    let marker = if token.is_empty() {
        "a boundary marker"
    } else {
        token
    };
    out.push(Violation {
        check: "gateway-ownership".into(),
        file: file.into(),
        line,
        severity: severity_for(),
        message: format!(
            "module touches `{facility}` ({marker}) outside its registered gateway ({registered})"
        ),
        hint: format!(
            "either own the touch where it lives (`gateway = \"{}\"` on this module's container) or route it through the gateway's facade",
            facility.split(" (").next().unwrap_or(facility)
        ),
    });
}

fn severity_for() -> Severity {
    crate::enforcement::checks::severity_of("gateway-ownership")
}

/// The `base` facility: imports into rank-0 layers from outside the
/// registered `base` gateways and the base layers themselves. A declared
/// `depends_on` does not exempt — the gateway narrows the base reach from
/// layer granularity to registered containers.
fn check_base(
    root: &Path,
    manifest: &Manifest,
    d: &Digest,
    gateways: &[&str],
    out: &mut Vec<Violation>,
) {
    let mut cycle = Vec::new();
    let ranks = super::layering::rank_layers(manifest, &mut cycle);
    out.extend(cycle); // an unrankable DAG stays visible (layering reports it too; dedup merges)
    let bases: std::collections::BTreeSet<&str> = manifest
        .layers
        .iter()
        .filter(|l| ranks.get(l.name.as_str()).copied() == Some(0))
        .map(|l| l.name.as_str())
        .collect();
    if bases.is_empty() {
        return;
    }
    let homes = super::module_homes(manifest, d);
    let crate_names = workspace_crate_names(root);
    let dirs: std::collections::BTreeSet<String> = d
        .modules
        .iter()
        .filter_map(|m| m.crate_of().0)
        .filter(|dir| dir != ".")
        .collect();
    let name_dirs = crate_name_dirs(root, &dirs.into_iter().collect::<Vec<_>>());
    let hosts = crate::search::digest::HostIndex::new(d);
    for m in &d.modules {
        let Some(home) = homes.get(m.path.as_str()) else {
            continue;
        };
        if bases.contains(home.layer)
            || gateways
                .iter()
                .any(|p| m.path == *p || m.path.starts_with(&format!("{p}/")))
        {
            continue;
        }
        let (importer_dir, stem) = m.crate_of();
        for dep in &m.deps {
            let Some(target) = resolve_dep(dep, stem.as_str(), &crate_names) else {
                continue;
            };
            let Some(hosted) = hosts.hosted(&name_dirs, importer_dir.as_deref(), &target) else {
                continue;
            };
            let Some(t_home) = homes.get(hosted.path.as_str()) else {
                continue;
            };
            if !bases.contains(t_home.layer) || t_home.layer == home.layer {
                continue;
            }
            out.push(Violation {
                check: "gateway-ownership".into(),
                file: m.path.clone(),
                line: 0,
                severity: severity_for(),
                message: format!(
                    "module reaches the base layer `{}` ({dep}) without a `base` gateway in between",
                    t_home.layer
                ),
                hint: "only containers that declare `gateway = \"base\"` reach the bases directly; route through a registered gateway's API".into(),
            });
        }
    }
}

pub mod crates;

#[cfg(test)]
mod tests;
