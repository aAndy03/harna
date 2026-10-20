//! The declared registers and ledgers the store persists: parity
//! templates, deferrals, glossary, non-goals, lessons, roadmap, the
//! advisory ledger, archive, score snapshots and decisions.

use super::*;

impl Store {
    /// Write a register, creating `.harna/` when this is the first thing
    /// recorded (0.4.9 field test: `harna quality add` before `init` died
    /// on "os error 3" — qualities first is a natural order).
    fn put(&self, path: PathBuf, raw: impl AsRef<[u8]>) -> anyhow::Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("creating {}", self.root.display()))?;
        atomic::write(path, raw)
    }

    /// The parity templates file (0.3.22): declared fan-out shapes for
    /// recurring change types. Absence is the natural empty state —
    /// `load_templates` returns the default instead of an error.
    pub fn templates_path(&self) -> PathBuf {
        self.root.join(TEMPLATES)
    }

    /// Persist the parity templates document.
    pub fn save_templates(
        &self,
        t: &crate::schema::registers::parity_templates::ParityTemplates,
    ) -> anyhow::Result<()> {
        let raw = toml::to_string_pretty(t)?;
        self.put(self.templates_path(), raw)
            .with_context(|| format!("writing {}", TEMPLATES))?;
        Ok(())
    }

    /// Load the parity templates document; a missing file is an empty set
    /// (templates are adopted as data — until the first `template add`
    /// there is nothing declared and nothing to notice).
    pub fn load_templates(
        &self,
    ) -> anyhow::Result<crate::schema::registers::parity_templates::ParityTemplates> {
        if !self.templates_path().exists() {
            return Ok(Default::default());
        }
        let t: crate::schema::registers::parity_templates::ParityTemplates =
            read_toml(&self.templates_path())?;
        check_schema_version(t.schema_version)?;
        Ok(t)
    }

    /// The debt register file (0.3.23): deliberate deferrals with a
    /// review trigger. Absence is the natural empty state.
    pub fn deferred_path(&self) -> PathBuf {
        self.root.join(DEFERRED)
    }

    /// Persist the debt register document.
    pub fn save_deferred(
        &self,
        r: &crate::schema::registers::ledgers::deferred::DeferredRegister,
    ) -> anyhow::Result<()> {
        let raw = toml::to_string_pretty(r)?;
        self.put(self.deferred_path(), raw)
            .with_context(|| format!("writing {}", DEFERRED))?;
        Ok(())
    }

    /// Load the debt register document; a missing file is an empty set —
    /// a repo declares its deferrals as they happen, and no register
    /// means nothing was deferred. (An unparseable register errors here;
    /// callers choose to surface or skip.)
    pub fn load_deferred(
        &self,
    ) -> anyhow::Result<crate::schema::registers::ledgers::deferred::DeferredRegister> {
        if !self.deferred_path().exists() {
            return Ok(Default::default());
        }
        let r: crate::schema::registers::ledgers::deferred::DeferredRegister =
            read_toml(&self.deferred_path())?;
        check_schema_version(r.schema_version)?;
        Ok(r)
    }

    /// The glossary file (0.3.26): agent vocabulary -> harna vocabulary.
    /// Absence is the natural empty state.
    pub fn terms_path(&self) -> PathBuf {
        self.root.join(TERMS)
    }

    /// Persist the glossary document.
    pub fn save_terms(&self, t: &crate::schema::registers::terms::Terms) -> anyhow::Result<()> {
        let raw = toml::to_string_pretty(t)?;
        self.put(self.terms_path(), raw)
            .with_context(|| format!("writing {}", TERMS))?;
        Ok(())
    }

    /// Load the glossary document; a missing file is an empty set —
    /// a repo grows its vocabulary as sessions reveal the mis-mapped
    /// words. (An unparseable glossary errors here; callers choose to
    /// surface or skip.)
    pub fn load_terms(&self) -> anyhow::Result<crate::schema::registers::terms::Terms> {
        if !self.terms_path().exists() {
            return Ok(Default::default());
        }
        let t: crate::schema::registers::terms::Terms = read_toml(&self.terms_path())?;
        check_schema_version(t.schema_version)?;
        Ok(t)
    }

    /// The decline register file (0.3.27): deliberate product refusals.
    /// Absence is the natural empty state.
    pub fn non_goals_path(&self) -> PathBuf {
        self.root.join(NON_GOALS)
    }

    /// Persist the decline register document.
    pub fn save_non_goals(
        &self,
        g: &crate::schema::registers::non_goals::NonGoals,
    ) -> anyhow::Result<()> {
        let raw = toml::to_string_pretty(g)?;
        self.put(self.non_goals_path(), raw)
            .with_context(|| format!("writing {}", NON_GOALS))?;
        Ok(())
    }

    /// Load the decline register; a missing file is an empty set —
    /// refusals are declared when they are agreed. (Unparseable errors
    /// here; callers choose to surface or skip.)
    pub fn load_non_goals(&self) -> anyhow::Result<crate::schema::registers::non_goals::NonGoals> {
        if !self.non_goals_path().exists() {
            return Ok(Default::default());
        }
        let g: crate::schema::registers::non_goals::NonGoals = read_toml(&self.non_goals_path())?;
        check_schema_version(g.schema_version)?;
        Ok(g)
    }

    /// The dead-ends register file (0.3.27): tried-and-failed records
    /// with verdicts. Absence is the natural empty state.
    pub fn lessons_path(&self) -> PathBuf {
        self.root.join(LESSONS)
    }

    /// Persist the dead-ends register document.
    pub fn save_lessons(
        &self,
        l: &crate::schema::registers::lessons::Lessons,
    ) -> anyhow::Result<()> {
        let raw = toml::to_string_pretty(l)?;
        self.put(self.lessons_path(), raw)
            .with_context(|| format!("writing {}", LESSONS))?;
        Ok(())
    }

    /// Load the dead-ends register; a missing file is an empty set —
    /// lessons are recorded when failures are owned. (Unparseable
    /// errors here; callers choose to surface or skip.)
    pub fn load_lessons(&self) -> anyhow::Result<crate::schema::registers::lessons::Lessons> {
        if !self.lessons_path().exists() {
            return Ok(Default::default());
        }
        let l: crate::schema::registers::lessons::Lessons = read_toml(&self.lessons_path())?;
        check_schema_version(l.schema_version)?;
        Ok(l)
    }

    /// The qualities register (0.4.0): long-term intent. Absence is the
    /// natural empty state.
    pub fn qualities_path(&self) -> PathBuf {
        self.root.join(QUALITIES)
    }

    pub fn save_qualities(
        &self,
        q: &crate::schema::registers::qualities::Qualities,
    ) -> anyhow::Result<()> {
        let raw = toml::to_string_pretty(q)?;
        self.put(self.qualities_path(), raw)
            .with_context(|| format!("writing {}", QUALITIES))?;
        Ok(())
    }

    /// Load the qualities; a missing file is an empty set.
    pub fn load_qualities(&self) -> anyhow::Result<crate::schema::registers::qualities::Qualities> {
        if !self.qualities_path().exists() {
            return Ok(Default::default());
        }
        let q: crate::schema::registers::qualities::Qualities = read_toml(&self.qualities_path())?;
        check_schema_version(q.schema_version)?;
        Ok(q)
    }

    /// The roadmap file (0.3.28): declared milestones, never
    /// generated. Absence is the natural empty state.
    pub fn roadmap_path(&self) -> PathBuf {
        self.root.join(ROADMAP)
    }

    /// Persist the roadmap document.
    pub fn save_roadmap(
        &self,
        r: &crate::schema::registers::roadmap::Roadmap,
    ) -> anyhow::Result<()> {
        let raw = toml::to_string_pretty(r)?;
        self.put(self.roadmap_path(), raw)
            .with_context(|| format!("writing {}", ROADMAP))?;
        Ok(())
    }

    /// Load the roadmap; a missing file is an empty set — a project
    /// declares a roadmap when it wants one. (Unparseable errors here;
    /// callers choose to surface or skip.)
    pub fn load_roadmap(&self) -> anyhow::Result<crate::schema::registers::roadmap::Roadmap> {
        if !self.roadmap_path().exists() {
            return Ok(Default::default());
        }
        let r: crate::schema::registers::roadmap::Roadmap = read_toml(&self.roadmap_path())?;
        check_schema_version(r.schema_version)?;
        Ok(r)
    }

    /// The advisory ledger file (0.3.31): accepted Info findings with
    /// reasons. Absence is the natural empty state.
    pub fn acknowledged_path(&self) -> PathBuf {
        self.root.join(ACKNOWLEDGED)
    }

    /// Persist the advisory ledger document.
    pub fn save_acknowledged(
        &self,
        a: &crate::schema::registers::ledgers::acknowledged::Acknowledged,
    ) -> anyhow::Result<()> {
        let raw = toml::to_string_pretty(a)?;
        self.put(self.acknowledged_path(), raw)
            .with_context(|| format!("writing {}", ACKNOWLEDGED))?;
        Ok(())
    }

    /// Load the advisory ledger; a missing file is an empty set —
    /// nothing is acknowledged until someone accepts it in writing.
    /// (Unparseable errors here; callers choose to surface or skip.)
    pub fn load_acknowledged(
        &self,
    ) -> anyhow::Result<crate::schema::registers::ledgers::acknowledged::Acknowledged> {
        if !self.acknowledged_path().exists() {
            return Ok(Default::default());
        }
        let a: crate::schema::registers::ledgers::acknowledged::Acknowledged =
            read_toml(&self.acknowledged_path())?;
        check_schema_version(a.schema_version)?;
        Ok(a)
    }

    /// The command registry (0.3.46). Absence is the natural empty state.
    pub fn commands_path(&self) -> PathBuf {
        self.root.join(COMMANDS)
    }

    pub fn save_commands(
        &self,
        c: &crate::schema::registers::commands::Commands,
    ) -> anyhow::Result<()> {
        let raw = toml::to_string_pretty(c)?;
        self.put(self.commands_path(), raw)
            .with_context(|| format!("writing {}", COMMANDS))?;
        Ok(())
    }

    pub fn load_commands(&self) -> anyhow::Result<crate::schema::registers::commands::Commands> {
        if !self.commands_path().exists() {
            return Ok(Default::default());
        }
        let c: crate::schema::registers::commands::Commands = read_toml(&self.commands_path())?;
        check_schema_version(c.schema_version)?;
        Ok(c)
    }

    /// The ratchet ledger (0.3.44). Absence is the natural empty state.
    pub fn baselines_path(&self) -> PathBuf {
        self.root.join(BASELINES)
    }

    pub fn save_baselines(
        &self,
        b: &crate::schema::registers::ledgers::baselines::Baselines,
    ) -> anyhow::Result<()> {
        let raw = toml::to_string_pretty(b)?;
        self.put(self.baselines_path(), raw)
            .with_context(|| format!("writing {}", BASELINES))?;
        Ok(())
    }

    pub fn load_baselines(
        &self,
    ) -> anyhow::Result<crate::schema::registers::ledgers::baselines::Baselines> {
        if !self.baselines_path().exists() {
            return Ok(Default::default());
        }
        let b: crate::schema::registers::ledgers::baselines::Baselines =
            read_toml(&self.baselines_path())?;
        check_schema_version(b.schema_version)?;
        Ok(b)
    }

    /// Move one live file into the archive's `kind` subdirectory and
    /// return the archive path. A same-volume rename — the bytes never
    /// change, only the surface does; that is the whole point (the file
    /// stays honest data, just not in the way).
    pub fn archive_file(&self, src: &Path, kind: &str) -> anyhow::Result<PathBuf> {
        let name = src
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("archiving {}: no file name", src.display()))?;
        let dir = self.archive_dir().join(kind);
        if !src.exists() {
            // Idempotent: already archived (or never live) is a no-op.
            return Ok(dir.join(name));
        }
        fs::create_dir_all(&dir)?;
        // An earlier archive with the same name keeps its bytes: the new
        // one takes the next free `_N` (rename would clobber it).
        let name = Path::new(name);
        let stem = name
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = name
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let dest = atomic::free_name(&dir, &stem, &ext);
        fs::rename(src, &dest)
            .with_context(|| format!("archiving {} -> {}", src.display(), dest.display()))?;
        Ok(dest)
    }

    /// Persist a score snapshot (0.3.13): `stem[_N].toml` under scores/.
    /// Recordings append, never replace — a stem collision takes the next
    /// `_N` suffix. The underscore keeps chronological filename order (`-`
    /// would sort before `.toml`'s dot and put the newer recording first).
    /// Persistence lives here, in the storage gateway, so composition
    /// briefings never touch the disk directly (gateway-ownership holds
    /// the `fs:write` monopoly to schema/generation/conformance).
    pub fn save_score_snapshot(&self, stem: &str, content: &str) -> anyhow::Result<PathBuf> {
        let dir = self.scores_dir();
        fs::create_dir_all(&dir)?;
        let mut n = 0u32;
        loop {
            let candidate = dir.join(format!(
                "{stem}{}.toml",
                if n == 0 {
                    String::new()
                } else {
                    format!("_{n}")
                }
            ));
            if !candidate.exists() {
                atomic::write(&candidate, content)?;
                return Ok(candidate);
            }
            n += 1;
        }
    }

    /// Sorted `.toml` stems under scores/; a missing dir is no recordings,
    /// not an error.
    pub fn score_snapshot_names(&self) -> Vec<String> {
        let dir = self.scores_dir();
        let Ok(entries) = fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
            .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .collect();
        names.sort();
        names
    }

    /// Read one snapshot file back (parse errors fail loudly at the call
    /// site — silent rot would poison every trend drawn from it).
    pub fn read_score_snapshot(&self, name: &str) -> anyhow::Result<String> {
        let path = self.scores_dir().join(format!("{name}.toml"));
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
    }

    /// Write a decision record and return its relative path. The optional
    /// `supersedes` points at the decision file this one replaces — the
    /// decision log renders the chain.
    pub fn save_decision(
        &self,
        title: &str,
        body: &str,
        supersedes: Option<&str>,
    ) -> anyhow::Result<PathBuf> {
        let slug = slug(title, "decision");
        let content = crate::schema::journal::decisions::markdown(title, body, supersedes);
        let dir = self.decisions_dir();
        fs::create_dir_all(&dir)?;
        // Two same-day decisions with one title take `_N` — a record is
        // appended, never silently replaced.
        let stem = format!("{}-{slug}", chrono::Utc::now().format("%Y-%m-%d"));
        let file = atomic::free_name(&dir, &stem, ".md");
        atomic::write_new(&file, content)?;
        Ok(file)
    }
}
