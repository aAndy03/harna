//! Component `scan` (0.3.38): one map rebuild for every surface (CLI and
//! MCP used to carry twin copies). Rebuilds the code map incrementally,
//! shards it by the manifest, and links tests: every empty `tests` field
//! whose component has real tests is registered (generation::linkage).

use std::path::Path;

use crate::schema::store::Store;
use crate::search::digest;

pub struct ScanReport {
    pub modules: usize,
    pub signatures: usize,
    pub reused: usize,
    pub rescanned: usize,
    /// Components whose tests this scan registered in the manifest.
    pub tests_registered: Vec<String>,
}

impl ScanReport {
    pub fn render(&self) -> String {
        let mut out = format!(
            "digest written: {} modules, {} signatures (reused {}, scanned {})",
            self.modules, self.signatures, self.reused, self.rescanned
        );
        if !self.tests_registered.is_empty() {
            out.push_str(&format!(
                "\ntests registered for {} component(s): {}",
                self.tests_registered.len(),
                self.tests_registered.join(", ")
            ));
        }
        out
    }
}

pub fn run(store: &Store, root: &Path, max_files: usize, full: bool) -> anyhow::Result<ScanReport> {
    let previous = store.load_digest().ok();
    let (d, reused, rescanned) = digest::build_incremental(root, max_files, previous.as_ref(), full)?;
    // Only a manifest that exists AND loads is ever rewritten; a broken
    // one is validate's to report, never scan's to overwrite.
    let (manifest, tests_registered) = match store.manifest_path().exists().then(|| store.load_manifest()) {
        Some(Ok(mut m)) => {
            let registered = crate::generation::linkage::register_tests(root, &mut m);
            if !registered.is_empty() {
                store.save_manifest(&m)?;
            }
            (m, registered)
        }
        _ => (Default::default(), Vec::new()),
    };
    store.save_digest(&manifest, &d)?;
    Ok(ScanReport {
        modules: d.module_count(),
        signatures: d.signature_count(),
        reused,
        rescanned,
        tests_registered,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_registers_existing_tests_once_and_never_touches_a_broken_manifest() {
        let root = std::env::temp_dir().join(format!("harna-scan-{:?}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "pub fn f() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n").unwrap();
        let store = Store::open(&root).unwrap();
        store.init().unwrap();
        std::fs::write(
            store.manifest_path(),
            "schema_version = 1\n[[layers]]\nname = \"l\"\n[[layers.containers]]\nname = \"c\"\npath = \"src\"\n\
             [[layers.containers.components]]\nname = \"a\"\nkind = \"primitive\"\npath = \"src/a.rs\"\nsummary = \"s\"\n",
        )
        .unwrap();
        let r = run(&store, &root, 1000, false).unwrap();
        assert_eq!(r.tests_registered, vec!["a".to_string()]);
        assert!(r.render().contains("tests registered for 1 component(s): a"));
        let m = store.load_manifest().unwrap();
        assert_eq!(m.layers[0].containers[0].components[0].tests, vec!["src/a.rs".to_string()]);
        assert!(run(&store, &root, 1000, false).unwrap().tests_registered.is_empty(), "idempotent");

        let broken = "schema_version = 1\n[[layers]\n";
        std::fs::write(store.manifest_path(), broken).unwrap();
        assert!(run(&store, &root, 1000, false).is_ok(), "the map still rebuilds");
        assert_eq!(std::fs::read_to_string(store.manifest_path()).unwrap(), broken);
        let _ = std::fs::remove_dir_all(&root);
    }
}
