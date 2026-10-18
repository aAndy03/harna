//! The change-flow tools: diff, parity, place, handoff, dependents,
//! whereis, deny, doctor, scaffold — merged into the router at startup.

use super::*;

#[tool_router(router = flow_router, vis = "pub(super)")]
impl HarnaTools {
    /// Compare the working tree's code map against a committed digest.
    #[tool(description = "Structural diff of the code map vs a git ref: modules, pub signatures, internal deps added/removed.")]
    async fn harna_diff(
        &self,
        Parameters(DiffRequest { from, max_files }): Parameters<DiffRequest>,
    ) -> Result<String, String> {
        self.call(move |_store| {
            // Hash-verified reuse (0.3.17): same discipline as the CLI diff —
            // the committed map stands in for unchanged bodies.
            let previous = _store.load_digest().ok();
            let (fresh, _, _) = digest::build_incremental(
                &self.root,
                max_files.unwrap_or(10_000) as usize,
                previous.as_ref(),
                false,
            )?;
            let baseline =
                harna_core::search::diff::digest_at_ref(&self.root, from.as_deref().unwrap_or("HEAD"))?;
            let mut d = harna_core::search::diff::diff(&baseline, &fresh);
            d.from = from.unwrap_or_else(|| "HEAD".into());
            d.to = "working-tree".into();
            Ok(d.render())
        })
    }

    /// Show the parity ledger (change-propagation debts).
    #[tool(description = "Show parity events and their targets; open_only=true for the open debts that gate validate.")]
    async fn harna_parity(
        &self,
        Parameters(ParityShowRequest { id, open_only }): Parameters<ParityShowRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            harna_core::enforcement::parity::show(&store, id.as_deref(), open_only.unwrap_or(false))
        })
    }

    /// Open a parity event: compute the fan-out of the current structural diff.
    #[tool(description = "Before committing a structural change: open (or revise) a parity event — the modules that must follow the diff vs a ref.")]
    async fn harna_parity_open(
        &self,
        Parameters(ParityOpenRequest { id, kind, from }): Parameters<ParityOpenRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let ev = harna_core::enforcement::parity::open(
                &self.root,
                &store,
                &id,
                kind.as_deref().unwrap_or("digest-diff"),
                "",
                from.as_deref().unwrap_or("HEAD"),
                10_000,
            )?;
            let open = ev.open_targets().len();
            Ok(format!(
                "parity `{}` gen{}: {} open / {} targets — list them with harna_parity",
                ev.id, ev.generation, open, ev.targets.len()
            ))
        })
    }

    /// Resolve one parity target.
    #[tool(description = "Resolve one parity target: 'ack' (the change landed there) or 'noop' (nothing needed; the note says why).")]
    async fn harna_parity_resolve(
        &self,
        Parameters(ParityResolveRequest { id, path, status, note }): Parameters<ParityResolveRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let before = harna_core::enforcement::parity::event(&store, &id)
                .ok()
                .and_then(|e| harna_core::enforcement::parity::previous_resolution(&e, &path));
            let ev = harna_core::enforcement::parity::resolve(
                &store,
                &id,
                &path,
                &status,
                note.as_deref().unwrap_or(""),
                "agent via harna MCP",
            )?;
            Ok(format!(
                "resolved `{path}` -> {} in `{}`; {} open target(s) left{}",
                status,
                ev.id,
                ev.open_targets().len(),
                before.map(|w| format!("\nwarning: {w}")).unwrap_or_default()
            ))
        })
    }

    /// Auto-verify open parity targets against the event's baseline.
    #[tool(description = "Auto-verify open parity targets whose surface is unchanged vs the baseline. id = one event; omit = all.")]
    async fn harna_parity_auto(
        &self,
        Parameters(ParityAutoRequest { id }): Parameters<ParityAutoRequest>,
    ) -> Result<String, String> {
        self.call(move |store| match id.as_deref() {
            Some(id) => {
                let (ev, verified, open) =
                    harna_core::enforcement::parity::auto(&self.root, &store, id, 10_000)?;
                Ok(format!(
                    "parity `{}`: {} auto-verified, {} still open (generation unchanged)",
                    ev.id, verified, open
                ))
            }
            None => Ok(harna_core::enforcement::parity::auto_all(&self.root, &store, 10_000)?
                .join("\n")),
        })
    }

    /// PLAN step: full pre-code briefing for an idea, before any code exists.
    #[tool(description = "PLAN step, before code: the full briefing for an idea — where it lives, decisions, constraining rules, expected fan-out, test obligations, refusals and dead ends (CLI: harna plan).")]
    async fn harna_place(
        &self,
        Parameters(PlaceRequest { idea }): Parameters<PlaceRequest>,
    ) -> Result<String, String> {
        self.call(move |store| harna_core::composition::plan::build(&store, &idea))
    }

    /// Burst journal: record state before compaction; resume from it later.
    #[tool(description = "Before compaction: record summary + next_steps; onboarding carries it to the next session. clear=true when resumed; no args = read it.")]
    async fn harna_handoff(
        &self,
        Parameters(HandoffRequest { summary, next_steps, clear }): Parameters<HandoffRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            if clear.unwrap_or(false) {
                return Ok(if harna_core::schema::handoff::clear(&store)? {
                    "handoff cleared (burst resumed)".into()
                } else {
                    "no handoff on disk".into()
                });
            }
            match summary {
                Some(s) => {
                    let open_parity =
                        harna_core::enforcement::parity::open_debt_count(&store).unwrap_or(0);
                    let path = harna_core::schema::handoff::save(
                        &store,
                        &s,
                        &next_steps.unwrap_or_default(),
                        open_parity,
                    )?;
                    Ok(format!("handoff recorded: {}", path.display()))
                }
                None => Ok(match harna_core::schema::handoff::load(&store) {
                    Some(h) => harna_core::schema::handoff::render(&h),
                    None => {
                        "no handoff recorded — none needed if the burst is one plain commit".into()
                    }
                }),
            }
        })
    }

    /// Reverse-edge query: who imports this module (ring 1) and who follows.
    #[tool(description = "Who imports a module (ring 1 with the edge, ring 2 knock-on) and their containers. Ask before changing it.")]
    async fn harna_dependents(
        &self,
        Parameters(DependentsRequest { module }): Parameters<DependentsRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            Ok(harna_core::search::dependents::build(&store, &module)?.render())
        })
    }

    /// Symbol-level locator: where does a declared symbol live?
    #[tool(description = "Where a symbol (name or module::Symbol) is defined, and which modules reach it.")]
    async fn harna_whereis(
        &self,
        Parameters(WhereisRequest { symbol }): Parameters<WhereisRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            Ok(harna_core::search::whereis::build(&store, &symbol)?.render())
        })
    }

    /// Generate deny.toml (compile-time layering) from the manifest.
    #[tool(description = "Write the layer topology into deny.toml so `cargo deny check bans` fails cross-layer crate deps.")]
    async fn harna_deny(
        &self,
        Parameters(EmptyRequest { .. }): Parameters<EmptyRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let manifest = store.load_manifest()?;
            let plan = harna_core::generation::deny::plan(&self.root, &manifest)?;
            let path = harna_core::generation::deny::generate(&self.root, &manifest)?;
            let mut out = format!("deny.toml written: {}\n", path.display());
            for r in &plan.rules {
                let deps = if r.wrappers.is_empty() {
                    "nobody".to_string()
                } else {
                    r.wrappers.join(", ")
                };
                out.push_str(&format!("  {} may be depended on by: {}\n", r.crate_name, deps));
            }
            Ok(out)
        })
    }

    /// Installation self-audit.
    #[tool(description = "Installation health: schema, data files, map freshness, git, commit gate, agent wiring. Act on FAIL items first.")]
    async fn harna_doctor(
        &self,
        Parameters(DoctorRequest {}): Parameters<DoctorRequest>,
    ) -> Result<String, String> {
        self.call(|store| {
            let report = harna_core::composition::doctor::build(&store, &self.root)?;
            Ok(report.render())
        })
    }

    /// Scaffold a new component from a template and register it in the manifest.
    #[tool(description = "Create a component (primitive | compound | data) from a template and register it in the manifest. Prefer this over hand-made structure. role optionally stamps the container.")]
    async fn harna_scaffold(
        &self,
        Parameters(ScaffoldRequest { layer, container, name, kind, path, description, role }): Parameters<ScaffoldRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let kind = match kind.as_str() {
                "primitive" => harna_core::generation::scaffold::ScaffoldKind::Primitive,
                "compound" => harna_core::generation::scaffold::ScaffoldKind::Compound,
                "data" => harna_core::generation::scaffold::ScaffoldKind::Data,
                other => anyhow::bail!("unknown kind '{other}': use 'primitive', 'compound' or 'data'"),
            };
            let req = harna_core::generation::scaffold::ScaffoldRequest {
                layer, container, name, kind, path, role,
                description: description.unwrap_or_default(),
            };
            let res = harna_core::generation::scaffold::run(&self.root, &store, &req)?;
            Ok(res.message)
        })
    }

    /// Scaffold a transiter: a primitive dedicated to connecting two engines.
    #[tool(description = "Create the transiter that connects two layers (engines never call each other directly).")]
    async fn harna_scaffold_transiter(
        &self,
        Parameters(TransiterRequest { from, to, name }): Parameters<TransiterRequest>,
    ) -> Result<String, String> {
        self.call(move |store| {
            let res = harna_core::generation::scaffold::scaffold_transiter(&self.root, &store, &from, &to, name)?;
            Ok(res.message)
        })
    }
}
