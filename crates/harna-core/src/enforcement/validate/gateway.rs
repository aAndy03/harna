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
//! words plus `write!` carry the write side without the false positives.
//!
//! Known limitation (pinned by test, like the 0.3.3 brace collapse): the
//! brace form `use std::fs::{write, ...}` hides which symbols the brace
//! opens — the line counts as its READ side (`std::fs` head fires only
//! under a full `fs` gateway, not under `:write`).

use std::path::Path;

use super::{Severity, Violation};
use crate::enforcement::checks;
use crate::schema::manifest::Manifest;
use crate::search::digest::{
    crate_name_dirs, crate_of, hosted_module, resolve_dep, workspace_crate_names, Digest,
};

/// fs facility vocabulary: (token, is this write-side?).
const FS_TOKENS: &[(&str, bool)] = &[
    // write side — the storage monopoly lives exactly here
    ("std::fs::write", true), ("std::fs::create_dir", true), ("std::fs::create_dir_all", true),
    ("std::fs::remove_file", true), ("std::fs::remove_dir", true), ("std::fs::remove_dir_all", true),
    ("std::fs::rename", true), ("std::fs::copy", true), ("std::fs::hard_link", true),
    ("std::fs::File::create", true), ("std::fs::OpenOptions", true), ("std::fs::symlink", true),
    ("std::io::BufWriter", true), ("write!", true),
    ("tokio::fs::write", true), ("tokio::fs::OpenOptions", true), ("tokio::fs::create_dir", true),
    ("tokio::fs::remove", true), ("tokio::fs::rename", true), ("tokio::fs::copy", true),
    ("tempfile", true),
    // bare-module alias forms (`use std::fs;` then `fs::write(...)`)
    ("fs::write", true), ("fs::create_dir", true), ("fs::remove_file", true), ("fs::remove_dir", true),
    ("fs::rename", true), ("fs::copy", true), ("fs::hard_link", true),
    ("fs::File::create", true), ("fs::OpenOptions", true), ("fs::symlink", true),
    ("File::create", true),
    // read side
    ("std::fs", false), ("std::io", false), ("tokio::fs", false), ("walkdir", false),
    ("fs::read", false), ("fs::read_to_string", false), ("fs::read_exact", false),
    ("fs::read_link", false), ("fs::metadata", false), ("fs::canonicalize", false),
    ("fs::exists", false), ("fs::read_dir", false), ("File::open", false),
];

const NET_TOKENS: &[&str] = &[
    "std::net", "tokio::net", "reqwest", "ureq", "hyper", "sqlx",
    "tokio_postgres", "postgres::", "redis::", "mongodb", "mysql::", "diesel",
    "native_tls", "rustls",
];

const PROCESS_TOKENS: &[&str] = &[
    "std::process", "tokio::process", "std::os::windows::process", "std::os::unix::process",
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
        let (word, mode) = match part.split_once(':') {
            Some((w, m)) => (w.trim(), m.trim()),
            None => (part, ""),
        };
        match known_facility(word) {
            Some(id) => match mode {
                "" => ok.push(Facility { id, write_only: false }),
                m if m.eq_ignore_ascii_case("write") => ok.push(Facility { id, write_only: true }),
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
        .flat_map(|l| l.containers.iter().map(|c| (c.path.as_str(), c.gateway.as_deref())))
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
            let Some(raw) = c.gateway.as_deref() else { continue };
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
                    hint: "facilities: fs | net | process | base; optional mode after `:` is `write`"
                        .into(),
                });
            }
        }
    }
    let mut claims: Vec<(&'static str, Vec<String>)> = Vec::new();
    for (container, facilities) in declared_gateways(manifest) {
        for f in facilities {
            if let Some(claim) = claims.iter_mut().find(|(id, _)| *id == f.id) {
                claim.1.push(container.into());
            } else {
                claims.push((f.id, vec![container.into()]));
            }
        }
    }
    for (id, containers) in &claims {
        if containers.len() > 1 {
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
                hint: "one container should own a facility (split by facility — fs vs net — not by code area)".into(),
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
    for (facility, tokens, owned) in [("net", NET_TOKENS, net), ("process", PROCESS_TOKENS, process)] {
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
    container_paths.iter().any(|c| path.starts_with(&format!("{c}/")))
}

/// The gateway's ownership spans its whole subtree.
fn is_inside(path: &str, gateway_paths: &[&str]) -> bool {
    gateway_paths.iter().any(|p| path == *p || path.starts_with(&format!("{p}/")))
}

/// POS where `token` occurs as a real path-word, not as the tail of a
/// longer one (no `::` before, no identifier chars after — `my_std::fs`
/// and `crate::fs::read` name different things).
fn token_at(line: &str, token: &str) -> Option<usize> {
    let mut from = 0usize;
    while from < line.len() {
        let Some(at) = line[from..].find(token) else { break };
        let at = from + at;
        let prev_ok = at == 0
            || line[..at]
                .chars()
                .next_back()
                .map_or(true, |c| !(c.is_alphanumeric() || c == '_' || c == ':'));
        let next_ok = token.contains("::")
            || line[at + token.len()..]
                .chars()
                .next()
                .map_or(true, |c| !(c.is_alphanumeric() || c == '_'));
        if prev_ok && next_ok {
            return Some(at);
        }
        from = at + token.len();
    }
    None
}

/// First facility marker in the source: (1-based line, token, write-side).
fn first_hit<'t>(src: &str, tokens: &[(&'t str, bool)]) -> Option<(usize, &'t str, bool)> {
    for (i, line) in src.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[cfg(test)]") || trimmed.starts_with("#[test]") || trimmed.starts_with("mod tests") {
            break; // test fixtures may touch every facility
        }
        if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
            continue;
        }
        for (token, write) in tokens {
            if token_at(trimmed, token).is_some() {
                return Some((i + 1, token, *write));
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
        for (i, line) in src.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("#[cfg(test)]") || trimmed.starts_with("#[test]") || trimmed.starts_with("mod tests") {
                break;
            }
            if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
                continue;
            }
            if let Some(t) = tokens.iter().find(|t| token_at(trimmed, t).is_some()) {
                push(out, file, i + 1, t, facility, &registered);
                break;
            }
        }
    }
}

fn push(out: &mut Vec<Violation>, file: &str, line: usize, token: &str, facility: &str, registered: &str) {
    let marker = if token.is_empty() { "a boundary marker" } else { token };
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
    checks::spec("gateway-ownership").map(|s| s.default_severity).unwrap_or(Severity::Error)
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
        .filter_map(|m| crate_of(&m.path).0)
        .filter(|dir| dir != ".")
        .collect();
    let name_dirs = crate_name_dirs(root, &dirs.into_iter().collect::<Vec<_>>());
    for m in &d.modules {
        let Some(home) = homes.get(m.path.as_str()) else { continue };
        if bases.contains(home.layer)
            || gateways.iter().any(|p| m.path == *p || m.path.starts_with(&format!("{p}/")))
        {
            continue;
        }
        let (importer_dir, stem) = crate_of(&m.path);
        for dep in &m.deps {
            let Some(target) = resolve_dep(dep, stem.as_str(), &crate_names) else { continue };
            let Some(hosted) = hosted_module(d, &name_dirs, importer_dir.as_deref(), &target)
            else {
                continue;
            };
            let Some(t_home) = homes.get(hosted.path.as_str()) else { continue };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::manifest::{Container, Layer};
    use crate::search::digest::Module;

    fn container(name: &str, path: &str, gateway: Option<&str>, connects: &[&str]) -> Container {
        Container {
            name: name.into(),
            path: path.into(),
            description: String::new(),
            sequence: vec![],
            provides: vec![],
            role: None,
            gateway: gateway.map(|g| g.to_string()),
            connects: connects.iter().map(|s| s.to_string()).collect(),
            components: vec![],
        }
    }

    fn manifest(layers: Vec<(&str, Vec<Container>, Vec<&str>)>) -> Manifest {
        Manifest {
            schema_version: 1,
            layers: layers
                .into_iter()
                .map(|(name, containers, depends_on)| Layer {
                    name: name.into(),
                    description: String::new(),
                    sequence: vec![],
                    depends_on: depends_on.into_iter().map(|s| s.to_string()).collect(),
                    containers,
                })
                .collect(),
        }
    }

    fn module(path: &str, deps: &[&str]) -> Module {
        Module {
            path: path.into(),
            lang: "rust".into(),
            signatures: vec![],
            deps: deps.iter().map(|s| s.to_string()).collect(),
            body_hash: "x".into(),
            inventory: vec![],
            mods: vec![],
        }
    }

    fn digest_of(modules: Vec<Module>) -> Digest {
        Digest {
            generated_at: "now".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules,
        }
    }

    fn cargo_fixtures() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "harna-gateway-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("crates/t/src")).unwrap();
        std::fs::write(
            root.join("crates/t/Cargo.toml"),
            "[package]\nname = \"t\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = [\"crates/t\"]\n").unwrap();
        root
    }

    fn run(manifest: &Manifest, root: &Path, files: &[(&str, &str)]) -> Vec<Violation> {
        for (rel, src) in files {
            let abs = root.join(rel);
            std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
            std::fs::write(&abs, src).unwrap();
        }
        let srcs: Vec<(String, String)> = files
            .iter()
            .map(|(rel, src)| (rel.to_string(), src.to_string()))
            .collect();
        let v = Digest {
            generated_at: "now".into(),
            extractor_version: crate::search::digest::EXTRACTOR_VERSION,
            modules: vec![module("crates/t/src/storage/gw.rs", &[])],
        };
        let mut out = Vec::new();
        check(root, manifest, Some(&v), &srcs, &mut out);
        check_declarations(manifest, &mut out);
        out
    }

    #[test]
    fn write_gateway_gates_writers_not_readers() {
        let root = cargo_fixtures();
        let m = manifest(vec![(
            "data",
            vec![
                container("storage", "crates/t/src/storage", Some("fs:write"), &[]),
                container("app", "crates/t/src/app", None, &[]),
            ],
            vec![],
        )]);
        let out = run(
            &m,
            &root,
            &[
                ("crates/t/src/app/writer.rs", "fn go() { let s = fs::write(&path, b\"x\"); }"),
                ("crates/t/src/app/reader.rs", "fn go() { fs::read_to_string(&path); }"),
                ("crates/t/src/app/meta.rs", "fn go() { let m = std::fs::metadata(&path); }"),
                ("crates/t/src/app/blanket.rs", "use std::fs;\nlet m = fs::read_dir(dir);"),
                ("crates/t/src/storage/gw.rs", "use std::fs;\nstd::fs::create_dir_all(d);"),
            ],
        );
        let hits: Vec<_> = out.iter().filter(|v| v.check == "gateway-ownership").collect();
        assert_eq!(hits.len(), 1, "only the writer is gated, was: {out:?}");
        assert_eq!(hits[0].file, "crates/t/src/app/writer.rs");
        assert_eq!(hits[0].line, 1);
        assert!(hits[0].message.contains("fs (write side)"));
        assert!(hits[0].message.contains("fs::write"));
        assert!(hits[0].message.contains("crates/t/src/storage"));
    }

    #[test]
    fn full_fs_gateway_gates_reads_too_and_subtrees_are_exempt() {
        let root = cargo_fixtures();
        let m = manifest(vec![(
            "data",
            vec![
                container("storage", "crates/t/src/storage", Some("fs"), &[]),
                container("app", "crates/t/src/app", None, &[]),
            ],
            vec![],
        )]);
        let out = run(
            &m,
            &root,
            &[
                ("crates/t/src/app/reader.rs", "fn go() { fs::read_to_string(&path); }"),
                ("crates/t/src/app/writer.rs", "fn go() { std::fs::write(&path, b\"x\"); }"),
                ("crates/t/src/storage/local.rs", "fs::metadata(&path);"),
            ],
        );
        let hits: Vec<_> = out.iter().filter(|v| v.check == "gateway-ownership").collect();
        assert_eq!(hits.len(), 2, "both touches outside the gateway, was: {out:?}");
        assert!(hits.iter().all(|v| v.message.contains("outside its registered gateway (crates/t/src/storage (fs))")));
        assert!(hits.iter().all(|v| !v.file.contains("storage/local.rs")), "subtree exempt");
    }

    #[test]
    fn ungoverned_files_and_test_fixtures_stay_open() {
        let root = cargo_fixtures();
        let m = manifest(vec![(
            "data",
            vec![container("storage", "crates/t/src/storage", Some("fs:write"), &[])],
            vec![],
        )]);
        let out = run(
            &m,
            &root,
            &[
                // No container covers src/original: ungoverned carve-out.
                ("crates/t/src/original/rogue.rs", "fn go() { fs::write(&p, b\"x\"); }"),
                // Declared but test-gated: fixtures write temp dirs freely.
                (
                    "crates/t/src/app/t.rs",
                    "#[cfg(test)]\nmod tests {\n  fn f() { std::fs::write(&t, b\"y\"); }\n}",
                ),
            ],
        );
        assert!(out.iter().all(|v| v.check != "gateway-ownership"), "was: {out:?}");
    }

    #[test]
    fn net_and_process_facilities_gate_by_declaration() {
        let root = cargo_fixtures();
        let m = manifest(vec![(
            "edges",
            vec![
                container(
                    "client",
                    "crates/t/src/client",
                    Some("net, process"),
                    &[],
                ),
                container("app", "crates/t/src/app", None, &[]),
            ],
            vec![],
        )]);
        let out = run(
            &m,
            &root,
            &[
                ("crates/t/src/app/net.rs", "use reqwest::Client;\nfn f(c: reqwest::Client) {}"),
                ("crates/t/src/app/db.rs", "use sqlx::postgres;",),
                ("crates/t/src/app/run.rs", "fn f() { std::process::Command::new(\"cargo\"); }"),
                ("crates/t/src/client/gw.rs", "use reqwest::Client;"),
                ("crates/t/src/app/io.rs", "use std::io::Write;"),
            ],
        );
        let hits: Vec<_> = out.iter().filter(|v| v.check == "gateway-ownership").collect();
        assert_eq!(hits.len(), 3, "net client, db client and process spawn gate; io does not, was: {out:?}");
        assert!(hits.iter().any(|v| v.file.ends_with("net.rs")));
        assert!(hits.iter().any(|v| v.file.ends_with("db.rs")));
        assert!(hits.iter().any(|v| v.file.ends_with("run.rs")));
    }

    #[test]
    fn base_facility_narrows_the_reach_to_registered_gateways() {
        let root = cargo_fixtures();
        let m = manifest(vec![
            ("base", vec![container("storage", "crates/t/src/storage", None, &[])], vec![]),
            (
                "app",
                vec![
                    container("gw", "crates/t/src/repo", Some("base"), &[]),
                    container("business", "crates/t/src/business", None, &[]),
                ],
                vec!["base"],
            ),
        ]);
        let d = digest_of(vec![
            module("crates/t/src/storage/files.rs", &[]),
            module("crates/t/src/repo/gwmain.rs", &["crate::storage::files"]),
            module("crates/t/src/business/math.rs", &["crate::storage::files"]),
        ]);
        let srcs: Vec<(String, String)> = vec![];
        let mut out = Vec::new();
        check(&root, &m, Some(&d), &srcs, &mut out);
        let hits: Vec<_> = out.iter().filter(|v| v.check == "gateway-ownership").collect();
        assert_eq!(hits.len(), 1, "the registered gateway mediates; business may not, was: {out:?}");
        assert!(hits[0].file.ends_with("business/math.rs"));
        assert!(hits[0].message.contains("base layer `base`"));
    }

    #[test]
    fn depends_on_does_not_exempt_base_reaches() {
        let root = cargo_fixtures();
        let mut m = manifest(vec![
            ("base", vec![container("storage", "crates/t/src/storage", None, &[])], vec![]),
            (
                "app",
                vec![
                    container("gw", "crates/t/src/repo", Some("base"), &[]),
                    container("business", "crates/t/src/business", None, &[]),
                ],
                vec![],
            ),
        ]);
        // The layer declares its dependency: legal for layer-boundary, but
        // the gateway narrows the base reach to registered containers.
        m.layers[1].depends_on = vec!["base".into()];
        let d = digest_of(vec![
            module("crates/t/src/storage/files.rs", &[]),
            module("crates/t/src/business/math.rs", &["crate::storage::files"]),
        ]);
        let srcs: Vec<(String, String)> = vec![];
        let mut out = Vec::new();
        check(&root, &m, Some(&d), &srcs, &mut out);
        assert_eq!(
            out.iter().filter(|v| v.check == "gateway-ownership").count(),
            1,
            "declared reach is not a registered gateway, was: {out:?}"
        );
    }

    #[test]
    fn malformed_gateway_declarations_advise_and_stay_out_of_enforcement() {
        let root = cargo_fixtures();
        let m = manifest(vec![(
            "data",
            vec![container("storage", "crates/t/src/storage", Some("fs:writing, banana"), &[])],
            vec![],
        )]);
        let out = run(
            &m,
            &root,
            &[("crates/t/src/app/writer.rs", "fn go() { fs::write(&p, b\"x\"); }")],
        );
        assert!(
            out.iter().all(|v| v.file == ".harna/manifest.toml"),
            "nothing valid -> nothing engages (advisories only), was: {out:?}"
        );
        let infos: Vec<_> = out.iter().filter(|v| v.check == "gateway-ownership").collect();
        assert_eq!(infos.len(), 2, "both malformed parts named, was: {out:?}");
        assert!(infos.iter().all(|v| v.severity == Severity::Info));

        // A partially valid declaration engages the valid half only.
        let m2 = manifest(vec![(
            "data",
            vec![
                container("storage", "crates/t/src/storage", Some("fs, banana"), &[]),
                container("app", "crates/t/src/app", None, &[]),
            ],
            vec![],
        )]);
        let out2 = run(
            &m2,
            &root,
            &[
                ("crates/t/src/app/writer.rs", "fn go() { fs::write(&p, b\"x\"); }"),
                ("crates/t/src/app/net.rs", "use reqwest::Client;"),
            ],
        );
        let hits: Vec<_> = out2
            .iter()
            .filter(|v| v.check == "gateway-ownership" && v.file != ".harna/manifest.toml")
            .collect();
        assert_eq!(hits.len(), 1, "fs fire, net stays ungoverned, was: {out2:?}");
        assert!(hits[0].file.ends_with("writer.rs"));
    }

    #[test]
    fn terminal_flushes_are_not_storage_writes() {
        let root = cargo_fixtures();
        let m = manifest(vec![(
            "data",
            vec![
                container("storage", "crates/t/src/storage", Some("fs:write"), &[]),
                container("app", "crates/t/src/app", None, &[]),
            ],
            vec![],
        )]);
        let out = run(
            &m,
            &root,
            &[
                ("crates/t/src/app/term.rs", "use std::io::Write as _;\nlet _ = std::io::stdout().flush();"),
                ("crates/t/src/app/w.rs", "fn f(p: &str) { write!(g, \"x\"); }"),
            ],
        );
        let hits: Vec<_> = out
            .iter()
            .filter(|v| v.check == "gateway-ownership" && v.file != ".harna/manifest.toml")
            .collect();
        assert_eq!(hits.len(), 1, "the flush stays open; `write!` scores, was: {out:?}");
        assert!(hits[0].file.ends_with("w.rs"));
        assert!(hits[0].message.contains("write!"));
    }

    #[test]
    fn duplicate_facility_gateways_are_named_once_and_both_exempt() {
        let root = cargo_fixtures();
        let m = manifest(vec![(
            "data",
            vec![
                container("a", "crates/t/src/storage", Some("fs:write"), &[]),
                container("b", "crates/t/src/render", Some("fs:write"), &[]),
            ],
            vec![],
        )]);
        let out = run(
            &m,
            &root,
            &[
                ("crates/t/src/storage/writer.rs", "fn go() { fs::write(&p, b\"x\"); }"),
                ("crates/t/src/render/view.rs", "std::fs::create_dir_all(d);"),
            ],
        );
        assert!(
            out.iter().all(|v| v.check != "gateway-ownership" || v.file == ".harna/manifest.toml"),
            "both registered containers are exempt on the file side, was: {out:?}"
        );
        let claimed = out
            .iter()
            .find(|v| v.message.contains("2 registered gateways"))
            .expect("the doubled ownership is named");
        assert_eq!(claimed.severity, Severity::Info);
        assert!(claimed.message.contains("fs` has 2"));
    }

    #[test]
    fn tokens_do_not_score_longer_paths_or_cross_crate_names() {
        let root = cargo_fixtures();
        let m = manifest(vec![(
            "data",
            vec![
                container("storage", "crates/t/src/storage", Some("fs"), &[]),
                container("app", "crates/t/src/app", None, &[]),
            ],
            vec![],
        )]);
        let out = run(
            &m,
            &root,
            &[
                ("crates/t/src/app/z.rs", "let t = my_fs::read_to_string(p);\nlet u = self::fs::read_dir(d);"),
                ("crates/t/src/app/o.rs", "fn f() { fs::read_to_string(p); }"),
            ],
        );
        let hits: Vec<_> = out.iter().filter(|v| v.check == "gateway-ownership").collect();
        assert_eq!(hits.len(), 1, "only the std fs touch, was: {out:?}");
        assert!(hits[0].file.ends_with("app/o.rs"));
    }

    #[test]
    fn brace_form_is_a_read_side_limitation_pinned() {
        let root = cargo_fixtures();
        let writeo = manifest(vec![(
            "data",
            vec![
                container("storage", "crates/t/src/storage", Some("fs:write"), &[]),
                container("app", "crates/t/src/app", None, &[]),
            ],
            vec![],
        )]);
        let full = manifest(vec![(
            "data",
            vec![
                container("storage", "crates/t/src/storage", Some("fs"), &[]),
                container("app", "crates/t/src/app", None, &[]),
            ],
            vec![],
        )]);
        let files = &[("crates/t/src/app/brace.rs", "use std::fs::{write, File};\nfn f() {}")];
        assert!(
            run(&writeo, &root, files).iter().all(|v| v.check != "gateway-ownership"),
            "documented limitation: braces hide the write side under :write"
        );
        assert_eq!(
            run(&full, &root, files)
                .iter()
                .filter(|v| v.check == "gateway-ownership")
                .count(),
            1,
            "a full fs gateway still catches the blanket head"
        );
    }
}
