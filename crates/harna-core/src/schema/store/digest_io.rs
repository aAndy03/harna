//! The sharded code map on disk (0.3.15): index + one shard per
//! container, streamed reads, atomic writes, orphan cleanup.

use super::*;

impl Store {
    /// Sharded digest directory (0.3.15): `index.json` plus one shard
    /// per container under `shards/`.
    pub fn digest_dir(&self) -> PathBuf {
        self.root.join("digest")
    }

    /// Load the code map: sharded layout first, legacy `digest.json`
    /// second (repos scanned before 0.3.15 keep working until their next
    /// scan migrates them forward).
    pub fn load_digest(&self) -> anyhow::Result<Digest> {
        let index_path = self.digest_dir().join("index.json");
        if index_path.is_file() {
            return self.load_sharded(&index_path);
        }
        let raw = fs::read_to_string(self.digest_path())
            .with_context(|| format!("reading {}", self.digest_path().display()))?;
        serde_json::from_str(&raw).context("parsing digest.json")
    }

    /// The digest index alone (0.3.16): stamps plus compact per-module
    /// records — the whole path list at a fraction of the map's bytes.
    /// Sharded repos only; a missing index is the caller's signal to fall
    /// back (bounded reads do it via `for_each_shard`, the monolith's
    /// stamps live in the monolith itself).
    pub fn load_index(&self) -> anyhow::Result<DigestIndex> {
        let index_path = self.digest_dir().join("index.json");
        let raw = fs::read_to_string(&index_path)
            .with_context(|| format!("reading {}", index_path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", index_path.display()))
    }

    /// One shard file (0.3.16), `file` relative to `.harna/digest/` as the
    /// index records it: the parse unit is a container, never the map.
    pub fn load_shard(&self, file: &str) -> anyhow::Result<DigestShard> {
        let path = self.digest_dir().join(file);
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
    }

    /// Bounded module stream (0.3.16): visits every digest module in
    /// digest order, one shard in memory at a time — the visitor holds a
    /// `&Module` and decides what to keep, so a query never materializes
    /// the whole map. The legacy monolith joins the stream as a single
    /// pass (bilingual, like `load_digest`); its stamps ride along the
    /// same way the sharded index's do. The visitor returns `false` to
    /// stop early. Returns `false` when no map exists at all (the
    /// caller owns the "run `harna scan` first" refusal).
    pub fn for_each_shard(
        &self,
        mut visit: impl FnMut(&DigestStamp, &DigestShard) -> bool,
    ) -> anyhow::Result<bool> {
        let index_path = self.digest_dir().join("index.json");
        if index_path.is_file() {
            let index = self.load_index()?;
            let stamp = DigestStamp {
                generated_at: index.generated_at,
                extractor_version: index.extractor_version,
            };
            for shard in &index.shards {
                let parsed = self.load_shard(&shard.file)?;
                if !visit(&stamp, &parsed) {
                    return Ok(true);
                }
            }
            return Ok(true);
        }
        let path = self.digest_path();
        if !path.is_file() {
            return Ok(false);
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let d: Digest =
            serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
        let stamp = DigestStamp {
            generated_at: d.generated_at,
            extractor_version: d.extractor_version,
        };
        let shard = DigestShard { modules: d.modules };
        visit(&stamp, &shard);
        Ok(true)
    }

    fn load_sharded(&self, index_path: &Path) -> anyhow::Result<Digest> {
        let raw = fs::read_to_string(index_path)
            .with_context(|| format!("reading {}", index_path.display()))?;
        let index: DigestIndex = serde_json::from_str(&raw)
            .with_context(|| format!("parsing {}", index_path.display()))?;
        let mut modules = Vec::new();
        for shard in &index.shards {
            let path = self.digest_dir().join(&shard.file);
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let parsed: DigestShard = serde_json::from_str(&raw)
                .with_context(|| format!("parsing {}", path.display()))?;
            modules.extend(parsed.modules);
        }
        Ok(Digest {
            generated_at: index.generated_at,
            extractor_version: index.extractor_version,
            modules,
        })
    }

    /// Save the code map sharded per container (0.3.15): `index.json`
    /// (stamps + compact per-module records) plus one shard file per
    /// container holding its modules in full, digest order preserved
    /// inside each shard and across the index — a sharded round-trip is
    /// module-for-module identical to the monolith. Modules outside every
    /// container ride the `_unhomed` shard. The legacy `digest.json` is
    /// removed on save: one truth, never two.
    pub fn save_digest(&self, manifest: &Manifest, d: &Digest) -> anyhow::Result<()> {
        use crate::schema::map::{IndexModule, IndexShard};

        let homes = module_homes(manifest, d);
        // Container path -> (layer, name) for shard naming.
        let mut names: std::collections::BTreeMap<&str, (&str, &str)> =
            std::collections::BTreeMap::new();
        for layer in &manifest.layers {
            for c in &layer.containers {
                names.entry(c.path.as_str()).or_insert((layer.name.as_str(), c.name.as_str()));
            }
        }
        // First-seen container order; digest order inside each shard.
        let mut order: Vec<String> = Vec::new();
        let mut grouped: std::collections::BTreeMap<String, Vec<&Module>> =
            std::collections::BTreeMap::new();
        for m in &d.modules {
            let key = homes
                .get(m.path.as_str())
                .map(|h| h.container_path.to_string())
                .unwrap_or_else(|| "_unhomed".to_string());
            if !grouped.contains_key(&key) {
                order.push(key.clone());
            }
            grouped.entry(key).or_default().push(m);
        }
        let dir = self.digest_dir();
        let shards_dir = dir.join("shards");
        fs::create_dir_all(&shards_dir)?;
        let mut taken: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut index = DigestIndex {
            generated_at: d.generated_at.clone(),
            extractor_version: d.extractor_version,
            shards: Vec::new(),
        };
        for key in &order {
            let (layer, container) = if key == "_unhomed" {
                ("", "_unhomed")
            } else {
                names.get(key.as_str()).copied().unwrap_or(("", "_unhomed"))
            };
            let stem = shard_slug(&format!("{layer}-{container}"));
            let mut n = 0u32;
            let file = loop {
                let candidate = format!(
                    "shards/{stem}{}.json",
                    if n == 0 { String::new() } else { format!("_{n}") }
                );
                if !taken.contains(&candidate) {
                    taken.insert(candidate.clone());
                    break candidate;
                }
                n += 1;
            };
            let modules = &grouped[key.as_str()];
            let shard = DigestShard {
                modules: modules.iter().map(|m| (*m).clone()).collect(),
            };
            atomic::write(dir.join(&file), serde_json::to_string_pretty(&shard)?)?;
            index.shards.push(IndexShard {
                layer: layer.into(),
                container: container.into(),
                file,
                modules: modules
                    .iter()
                    .map(|m| IndexModule { path: m.path.clone(), body_hash: m.body_hash.clone() })
                    .collect(),
            });
        }
        atomic::write(dir.join("index.json"), serde_json::to_string_pretty(&index)?)?;
        // Only after the new index is live: shards it no longer lists (a
        // container renamed or removed) are generated leftovers — drop
        // them so they do not linger in git forever.
        for entry in fs::read_dir(&shards_dir)?.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".json") && !taken.contains(&format!("shards/{name}")) {
                fs::remove_file(entry.path())
                    .with_context(|| format!("removing orphaned shard {name}"))?;
            }
        }
        let legacy = self.digest_path();
        if legacy.is_file() {
            fs::remove_file(&legacy)?;
        }
        Ok(())
    }
}
