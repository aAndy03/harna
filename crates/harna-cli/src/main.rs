use std::path::PathBuf;
use std::process;

use clap::Parser;

use harna_core::search::digest;
use harna_core::composition::onboarding;
use harna_core::schema::rules::Rule;
use harna_core::schema::store::Store;

mod commands;
mod cli;

use cli::{Cli, Command};


fn main() {
    // clap's derived parser for the whole Command enum lives on the stack;
    // in debug builds it outgrew Windows' 1 MB main-thread stack (0.3.39).
    // The CLI runs on a thread with room to spare instead.
    let worker = std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .expect("spawn the harna CLI thread");
    match worker.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            eprintln!("error: {e:#}");
            process::exit(1);
        }
        Err(_) => process::exit(101), // the panic already printed itself
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let root = cli.root.unwrap_or_else(|| PathBuf::from("."));
    let store = Store::open(&root)?;

    match cli.command {
        Command::Init { from } => {
            store.init()?;
            let mut profile_note = String::new();
            if let Some(src) = from {
                let applied = store.apply_profile(&src)?;
                if applied.manifest {
                    profile_note.push_str(&format!(
                        "\nmanifest skeleton copied from {} — its paths point at the OTHER tree: prune and re-point them (edit the file or `harna scaffold`)",
                        src.display()
                    ));
                }
                if applied.rules {
                    profile_note.push_str(&format!(
                        "\nrules copied from {} — scopes that miss this tree simply apply nowhere",
                        src.display()
                    ));
                }
                if !applied.manifest && !applied.rules {
                    anyhow::bail!("no manifest.toml / rules.toml found under {}", src.display());
                }
            }
            let agents = store.ensure_agents_md()?;
            println!("initialized {}", store.manifest_path().parent().unwrap().display());
            if agents {
                println!("AGENTS.md written at repo root — agents now know the harna contract");
            } else {
                println!("AGENTS.md already covers harna — left untreated");
            }
            if !profile_note.is_empty() {
                println!("{profile_note}");
            }
            println!("next: `harna scan`, add rules, and wire the harna MCP server");
        }
        Command::Digest => {
            let onb = onboarding::build(&store)?;
            print!("{}", onb.text);
            eprintln!(
                "\n[onboarding: {} rules, {} modules, manifest_empty={}]",
                onb.rule_count, onb.module_count, onb.manifest_empty
            );
        }
        Command::Rules { scope, env, agent } => {
            let rules = store.load_rules()?;
            let rendered = rules.render_for_agent(scope.as_deref().filter(|_| agent), env.as_deref());
            print!("{}", rendered);
        }
        Command::AddRule { id, text, rationale, scopes, env, check, severity, pattern, limit } => {
            commands::rules::add(&store, Rule {
                id,
                text,
                rationale,
                scope: scopes,
                tags: vec![],
                origin: String::new(),
                examples: vec![],
                env,
                check,
                severity,
                pattern,
                limit,
                status: None,
            })?
        }
        Command::DeprecateRule { id, reason } => commands::rules::deprecate(&store, &id, reason.as_deref())?,
        Command::ActivateRule { id } => commands::rules::activate(&store, &id)?,
        Command::RemoveRule { id } => commands::rules::remove(&store, &id)?,
        Command::LogDecision { title, body, supersedes } => {
            let path = store.save_decision(&title, &body, supersedes.as_deref())?;
            println!("decision saved: {}", path.display());
        }
        Command::Decisions { chain } => {
            let decisions = harna_core::schema::decisions::list(&store.decisions_dir())?;
            match chain {
                Some(q) => {
                    println!("{}", harna_core::schema::decisions::chain(&decisions, q.trim())?);
                }
                None => {
                    println!("{}", harna_core::schema::decisions::render(&decisions));
                }
            }
        }
        Command::Manifest => {
            let m = store.load_manifest()?;
            if m.is_empty() {
                println!("manifest is empty — nothing recorded yet");
            } else {
                println!(
                    "{} layers, {} containers, {} components",
                    m.layers.len(),
                    m.container_count(),
                    m.component_count()
                );
            }
        }
        Command::Migrate { dry } => {
            let reports = harna_core::schema::migrate::run(&root, &store, dry)?;
            for r in &reports {
                println!("{}", r.render());
            }
        }
        Command::Restore { name } => {
            let restored = harna_core::schema::migrate::restore(&store, &name)?;
            println!("{restored}");
        }
        Command::Archive { keep } => {
            // The three ledgers file their own dead weight; this prints it.
            let closed = harna_core::enforcement::parity::archive_closed(&store)?;
            let superseded = harna_core::schema::decisions::archive_superseded(
                &store.decisions_dir(),
                &store.archive_dir(),
            )?;
            let old = harna_core::composition::score::archive_old_snapshots(&store, keep)?;
            println!(
                "archived: {} closed parity event(s), {} superseded decision(s), {} old score snapshot(s)",
                closed.len(),
                superseded.len(),
                old.len()
            );
            for id in &closed {
                println!("  parity/{id}.toml");
            }
            for f in &superseded {
                println!("  decisions/{f}");
            }
            for n in &old {
                println!("  scores/{n}.toml");
            }
            if closed.is_empty() && superseded.is_empty() && old.is_empty() {
                println!("nothing to archive — the live surface is already lean");
            }
        }
        Command::Templates { cmd } => commands::templates::run(&store, &root, cmd)?,
        Command::Notes { cmd } => commands::notes::run(&store, &root, cmd)?,
        Command::Deferred { cmd } => commands::deferred::run(&store, &root, cmd)?,
        Command::Terms { cmd } => commands::terms::run(&store, &root, cmd)?,
        Command::NonGoal { cmd } => commands::non_goal::run(&store, &root, cmd)?,
        Command::Lesson { cmd } => commands::lesson::run(&store, &root, cmd)?,
        Command::Roadmap { cmd } => commands::roadmap::run(&store, &root, cmd)?,
        Command::Adopt => {
            // A missing .harna/ is not an error, it is the fresh start
            // adopt exists for; a present manifest that is non-empty is
            // the refusal. A malformed manifest all the way to the CLI.
            if root.join(".harna/manifest.toml").is_file() {
                let existing = store.load_manifest()?;
                if !existing.is_empty() {
                    anyhow::bail!(
                        "this repository already has a manifest ({} layers) — `harna adopt` drafts fresh starts; edit the manifest or use `harna scaffold` instead",
                        existing.layers.len()
                    );
                }
            }
            let d = harna_core::generation::adopt::draft(&root, 10_000)?;
            // Summary to stderr: redirecting stdout must yield clean,
            // paste-ready manifest TOML — a draft is data, not a report.
            eprintln!(
                "draft: {} module(s) across {} layer(s)/{} container(s); {} cross-crate edge(s) proposed as depends_on",
                d.module_count, d.layer_count, d.container_count, d.cross_crate_edges
            );
            println!("{}", harna_core::generation::adopt::render(&d)?);
        }
        Command::Graph { format } => {
            let digest = store.load_digest()?;
            let manifest = store.load_manifest()?;
            let view = harna_core::generation::graph::build(&root, &manifest, &digest);
            let text = match format.as_str() {
                "dot" => harna_core::generation::graph::dot(&view),
                "mermaid" => harna_core::generation::graph::mermaid(&view),
                other => anyhow::bail!("unknown graph format `{other}` — use dot or mermaid"),
            };
            // The summary goes to stderr: redirecting stdout must yield a
            // clean, import-ready file — an export is data, not a report.
            eprintln!(
                "{} nodes · {} edges",
                view.nodes.len(),
                view.edges.len()
            );
            println!("{text}");
        }
        Command::Render { check, surface } => {
            let manifest = store.load_manifest()?;
            if manifest.is_empty() {
                anyhow::bail!("manifest has no layers — nothing to render yet");
            }
            if surface {
                // The surface view reads the committed map (inventory) too.
                let digest = store.load_digest()?;
                let stale = harna_core::generation::render::surface_is_stale(&root, &manifest, &digest)?;
                if stale {
                    if check {
                        anyhow::bail!(
                            "SURFACE.md is stale — run `harna render --surface` and commit the view"
                        );
                    }
                    let path = harna_core::generation::render::write_surface(&root, &manifest, &digest)?;
                    println!("rendered {}", path.display());
                } else {
                    if check {
                        println!("surface view is current");
                    } else {
                        println!("SURFACE.md already current (nothing to write)");
                    }
                }
                return Ok(());
            }
            let digest_ok = store.load_digest().ok();
            let arch_stale = harna_core::generation::render::is_stale(&root, &manifest)?;
            let surface_state = digest_ok.as_ref().map(|d| {
                harna_core::generation::render::surface_is_stale(&root, &manifest, d).unwrap_or(true)
            });
            let stale = arch_stale || surface_state.unwrap_or(false);
            if stale {
                if check {
                    anyhow::bail!(
                        "generated views are stale — run `harna render` and commit the views"
                    );
                }
                let path = harna_core::generation::render::write(&root, &manifest)?;
                println!("rendered {}", path.display());
                if let (Some(_d), Some(true)) = (digest_ok.as_ref(), surface_state) {
                    let path = harna_core::generation::render::write_surface(&root, &manifest, digest_ok.as_ref().unwrap())?;
                    println!("rendered {}", path.display());
                }
            } else if check {
                println!("views are current");
            } else {
                println!("generated views already current (nothing to write)");
                if surface_state.is_none() {
                    eprintln!(
                        "note: no committed code map — the surface view (SURFACE.md) needs one; run `harna scan`"
                    );
                }
            }
        }
        Command::Validate { changed, max_files, strict, json, env } => {
            let files = if changed {
                let changed_files = harna_core::enforcement::validate::changed_rust_files(&root)?;
                // No changed .rs files still runs the manifest-level gates
                // (parity debts, manifest paths, decisions, roadmap, data
                // health): a manifest-only edit must not pass unchecked.
                // Progress goes to stderr so `--json` stays parseable.
                if changed_files.is_empty() {
                    eprintln!("no changed .rs files — running manifest-level checks only");
                } else {
                    eprintln!("checking {} changed file(s)…", changed_files.len());
                }
                Some(changed_files)
            } else {
                None
            };
            let violations =
                harna_core::enforcement::validate::run_on(&root, &store, max_files, files, env.as_deref())?;
            // Advisory ledger (0.3.31): locked-away Infos ride as
            // receipts, never as blocks — and stay out of strict gating.
            let ledger = store.load_acknowledged().unwrap_or_default();
            let report = harna_core::enforcement::validate::acknowledge::gate(&ledger, violations, strict);
            let (errors, infos) = (report.errors, report.infos);

            if json {
                let out = serde_json::json!({
                    "violations": report.violations,
                    "acknowledged": report.acknowledged,
                    "clean": report.violations.is_empty(),
                    "counts": { "error": errors, "info": infos, "acknowledged": report.acknowledged.len() },
                    "blocks": report.blocks,
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                print!("{}", report.render());
            }

            if report.blocks {
                // Non-zero exit so pre-commit hooks and CI can gate on this.
                eprintln!("harna: conformance gate failed ({} error, {} info{})",
                    errors, infos,
                    if strict && infos > 0 && errors == 0 { " — strict mode" } else { "" });
                {
                    use std::io::Write as _;
                    let _ = std::io::stdout().flush();
                }
                process::exit(2);
            }
        }
        Command::Status { json } => {
            let s = harna_core::composition::status::build(&store)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&s)?);
            } else {
                print!("{}", s.render());
            }
        }
        Command::Place { idea } => {
            print!("{}", harna_core::composition::place::build(&store, &idea)?);
        }
        Command::Plan { idea } => {
            print!("{}", harna_core::composition::plan::build(&store, &idea)?);
        }
        Command::Handoff { summary, next, clear } => {
            if clear {
                if harna_core::schema::handoff::clear(&store)? {
                    println!("handoff cleared (burst resumed)");
                } else {
                    println!("no handoff on disk");
                }
                return Ok(());
            }
            match summary {
                Some(s) => {
                    let open_parity = harna_core::enforcement::parity::open_debt_count(&store)
                        .unwrap_or(0);
                    let path = harna_core::schema::handoff::save(&store, &s, &next, open_parity)?;
                    println!("handoff recorded: {}", path.display());
                }
                None => match harna_core::schema::handoff::load(&store) {
                    Some(h) => print!("{}", harna_core::schema::handoff::render(&h)),
                    None => println!("no handoff recorded — none needed if the burst is one plain commit"),
                },
            }
        }
        Command::Doctor { json } => {
            let report = harna_core::composition::doctor::build(&store, &root)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", report.render());
            }
            if report.fail_count() > 0 {
                eprintln!(
                    "harna: {} installation issue(s) block work — fix the FAIL items",
                    report.fail_count()
                );
                {
                    use std::io::Write as _;
                    let _ = std::io::stdout().flush();
                }
                process::exit(1);
            }
        }
        Command::Preflight { json, max_files } => {
            let r = harna_core::composition::preflight::build(&store, &root, max_files)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", r.render());
            }
            if !r.pass {
                // Gate precedent is `validate`'s exit 2: hooks and CI gate on this.
                eprintln!("harna: preflight FAIL — not releasable until every gate holds");
                {
                    use std::io::Write as _;
                    let _ = std::io::stdout().flush();
                }
                process::exit(2);
            }
        }
        Command::ValidateIntent { json } => {
            let r = harna_core::composition::intent::build(&store, &root)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", r.render());
            }
            if !r.pass {
                // Same gate precedent: drift is normal mid-burst, but a
                // claimed coherence with drift is not releasable.
                eprintln!("harna: validate-intent found DRIFT(s) — reconcile before claiming coherence");
                {
                    use std::io::Write as _;
                    let _ = std::io::stdout().flush();
                }
                process::exit(2);
            }
        }
        Command::Acknowledge { cmd } => commands::ack::run(&store, &root, cmd)?,
        Command::Hooks { cmd } => commands::hooks::run(&store, &root, cmd)?,
        Command::Hook { event } => commands::hooks::answer(&store, &root, &event)?,
        Command::Decompose { file } => {
            let src = std::fs::read_to_string(root.join(&file))
                .map_err(|_| anyhow::anyhow!("cannot read `{file}` — repo-relative path from the repo root"))?;
            let p = harna_core::composition::decompose::plan(&file, &src);
            print!("{}", p.render());
        }
        Command::Tree => {
            let d = store.load_digest().map_err(|_| {
                anyhow::anyhow!("no code map — run `harna scan` first")
            })?;
            print!("{}", d.render_tree());
        }
        Command::Dependents { module, json } => {
            let r = harna_core::search::dependents::build(&store, &module)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", r.render());
            }
        }
        Command::Whereis { symbol, json } => {
            let r = harna_core::search::whereis::build(&store, &symbol)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", r.render());
            }
        }
        Command::Tests { component, json } => {
            let manifest = store.load_manifest()?;
            let mut targets =
                harna_core::enforcement::validate::test_coverage::runnable(&root, &manifest);
            if let Some(name) = component {
                if !targets.iter().any(|t| t.component == name) {
                    anyhow::bail!("unknown component `{name}` — no registered tests under that name");
                }
                targets.retain(|t| t.component == name);
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&targets)?);
            } else {
                print!("{}", harna_core::enforcement::validate::test_coverage::render_targets(&targets));
            }
        }
        Command::Metrics { json, record, trend } => {
            if json {
                let r = harna_core::composition::score::full(&store)?;
                println!("{}", serde_json::to_string_pretty(&r)?);
                return Ok(());
            }
            let mut out = String::new();
            if record {
                let s = harna_core::composition::score::build(&store)?;
                let path = harna_core::composition::score::record(&store, &s)?;
                out.push_str(&format!("recorded {}\n", path.display()));
            }
            if trend || !record {
                if trend {
                    let snaps = harna_core::composition::score::list_snapshots(&store)?;
                    let rows = harna_core::composition::score::trend(&snaps);
                    out.push_str(&harna_core::composition::score::render_trend(&rows));
                } else {
                    // One scan, many consumers (0.3.17): metrics, score and
                    // hotspots share the single map read inside `full`.
                    let r = harna_core::composition::score::full(&store)?;
                    out.push_str(&r.metrics.render());
                    out.push_str(&r.score.render());
                    out.push_str(&r.hotspots.render());
                }
            }
            print!("{out}");
        }
        Command::Scaffold { layer, container, name, kind, path, description, role } => {
            let req = harna_core::generation::scaffold::ScaffoldRequest {
                layer,
                container,
                name,
                kind: kind.to_core(),
                path,
                description: description.unwrap_or_default(),
                role,
            };
            let res = harna_core::generation::scaffold::run(&root, &store, &req)?;
            println!("{}", res.message);
        }
        Command::Move { from, to } => {
            print!("{}", harna_core::generation::relocate::relocate(&root, &store, &from, &to)?.render());
        }
        Command::Transiter { from, to, name } => {
            let res = harna_core::generation::scaffold::scaffold_transiter(&root, &store, &from, &to, name)?;
            println!("{}", res.message);
        }
        Command::Scan { max_files, full } => {
            let started = std::time::Instant::now();
            let r = harna_core::composition::scan::run(&store, &root, max_files, full)?;
            println!("{} — {} ms", r.render(), started.elapsed().as_millis());
        }
        Command::Deny => {
            let manifest = store.load_manifest()?;
            let plan = harna_core::generation::deny::plan(&root, &manifest)?;
            let path = harna_core::generation::deny::generate(&root, &manifest)?;
            println!("deny.toml written: {}", path.display());
            for r in &plan.rules {
                let deps = if r.wrappers.is_empty() {
                    "nobody".to_string()
                } else {
                    r.wrappers.join(", ")
                };
                println!("  {} may be depended on by: {}", r.crate_name, deps);
            }
            println!("CI: `cargo deny check bans` enforces the layer graph at compile time.");
        }
        Command::Diff { from, max_files, json } => {
            // Hash-verified reuse (0.3.17): the working-tree map rebuilds
            // from the committed map where the body hash proves a file
            // unchanged — same diff, a fraction of the parsing. The
            // extractor gate inside build_incremental keeps old maps from
            // feeding stale edges.
            let previous = store.load_digest().ok();
            let (fresh, _, _) = digest::build_incremental(&root, max_files, previous.as_ref(), false)?;
            let baseline = harna_core::search::diff::digest_at_ref(&root, &from)?;
            let mut d =
                harna_core::search::diff::diff(&baseline, &fresh);
            d.from = from.clone();
            d.to = "working-tree".into();
            if json {
                println!("{}", serde_json::to_string_pretty(&d)?);
            } else {
                print!("{}", d.render());
            }
        }
        Command::Parity { cmd } => commands::parity::run(&store, &root, cmd)?,
    }
    Ok(())
}
