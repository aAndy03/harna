//! Checks group `services` (0.8.0): the services `.harna/services.toml`
//! declares, held to the manifest and to each other; their corridors
//! (0.8.1) held to the services and to the shapes they name.

use std::path::Path;

use super::Violation;
use crate::schema::model::manifest::Manifest;
use crate::schema::store::Store;

pub mod coherence;
pub mod corridors;
pub mod fields;
pub mod shapes;
pub mod talk;

/// Every services check, one call from validate. Nothing declared, nothing
/// checked.
pub fn check(
    root: &Path,
    store: &Store,
    manifest: &Manifest,
    digest: Option<&crate::search::digest::Digest>,
    scoped: bool,
    cargo: bool,
    out: &mut Vec<Violation>,
) {
    let services = match store.load_services() {
        Ok(s) => s,
        Err(e) => {
            out.push(Violation {
                check: "service-coherence".into(),
                file: ".harna/services.toml".into(),
                line: 0,
                severity: crate::enforcement::checks::severity_of("service-coherence"),
                message: format!("{e:#}"),
                hint: "fix the file (harna service add/set/remove write it)".into(),
            });
            return;
        }
    };
    if services.is_empty() {
        return;
    }
    // a scoped --changed run (0.9.2): the edges that start in the change
    // only — the declaration and the shapes read the whole map, so they
    // run when services.toml or a shape changed (TS field test: a partial
    // map made a sound corridor read as "the map does not hold" its shape)
    if scoped {
        if let Some(d) = digest {
            talk::check(root, &services, manifest, d, cargo, out);
        }
        return;
    }
    coherence::check(root, &services, manifest, out);
    corridors::check(root, &services, digest, out);
    if let Some(d) = digest {
        talk::check(root, &services, manifest, d, true, out);
        shapes::check(root, &services, manifest, d, out);
    }
}
