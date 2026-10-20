//! Checks group `services` (0.8.0): the services `.harna/services.toml`
//! declares, held to the manifest and to each other; their corridors
//! (0.8.1) held to the services and to the shapes they name.

use std::path::Path;

use super::Violation;
use crate::schema::model::manifest::Manifest;
use crate::schema::store::Store;

pub mod coherence;
pub mod corridors;
pub mod shapes;
pub mod talk;

/// Every services check, one call from validate. Nothing declared, nothing
/// checked.
pub fn check(
    root: &Path,
    store: &Store,
    manifest: &Manifest,
    digest: Option<&crate::search::digest::Digest>,
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
    coherence::check(root, &services, manifest, out);
    corridors::check(root, &services, digest, out);
    if let Some(d) = digest {
        talk::check(root, &services, manifest, d, out);
        shapes::check(root, &services, manifest, d, out);
    }
}
