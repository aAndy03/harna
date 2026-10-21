use std::path::PathBuf;
use std::process;

use harna_core::schema::model::rules::Rule;
use harna_core::schema::store::Store;
use harna_core::search::digest;

mod cli;
mod commands;
mod help;

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
    let cli: Cli = help::parse();
    let root = cli.root.unwrap_or_else(|| PathBuf::from("."));
    let store = Store::open(&root)?;

    match cli.command {
        Command::Profile { cmd } => commands::packs::profile::run(cmd)?,
        Command::Contract { cmd } => commands::architecture::contract::run(&store, cmd)?,
        Command::Service { cmd } => commands::architecture::service::run(&store, cmd)?,
        Command::Services => commands::architecture::service::run(
            &store,
            commands::architecture::service::ServiceCmd::Show { name: None },
        )?,
        Command::Affected { base, json } => {
            use harna_core::composition::affected;
            let manifest = store.load_manifest()?;
            let digest = store.load_digest()?;
            let changed = affected::changed_files(&root, base.as_deref())?;
            if let Some(n) = affected::map_note(&root, &digest) {
                eprintln!("warning: {n}");
            }
            let a = affected::with_services(
                affected::compute(&root, &manifest, &digest, &changed),
                &store.load_services()?,
                &root,
                &manifest,
            );
            if json {
                println!("{}", serde_json::to_string_pretty(&a)?);
            } else {
                print!("{}", a.render());
            }
        }
        Command::Deprecations { window } => {
            use harna_core::enforcement::validate::artifacts::deprecation as dep;
            let rules = store.load_effective_rules()?;
            let window = window
                .or_else(|| {
                    rules
                        .rules
                        .iter()
                        .find(|r| r.check.as_deref() == Some("deprecation-window"))
                        .and_then(|r| r.limit.map(u64::from))
                })
                .unwrap_or(2);
            let now = dep::project_version(&root);
            let found = dep::scan(&root);
            if found.is_empty() {
                println!("no deprecated items (#[deprecated(since = ..)] or @deprecated since x)");
            }
            for d in found {
                let left = now.as_deref().map(|n| dep::left(&d.since, n, window));
                println!(
                    "{}:{} `{}` since {} — {}",
                    d.file,
                    d.line,
                    d.name,
                    d.since,
                    match left {
                        Some(l) if l <= 0 => "window closed: remove it".to_string(),
                        Some(l) => format!("removable in {l} version(s)"),
                        None => "no project version found (Cargo.toml / package.json)".into(),
                    }
                );
            }
        }
        Command::Owners {
            paths,
            changed,
            base,
        } => {
            let manifest = store.load_manifest()?;
            let routing = if changed || base.is_some() {
                harna_core::composition::owners::route_changed(
                    &root,
                    &harna_core::schema::store::services::owners_view(&root, &manifest),
                    base.as_deref(),
                )?
            } else {
                harna_core::composition::owners::route(
                    &harna_core::schema::store::services::owners_view(&root, &manifest),
                    &paths,
                )
            };
            print!("{}", routing.render());
        }
        Command::Plugin { cmd } => commands::packs::plugin::run(&store, cmd)?,
        Command::Init {
            from,
            agents,
            profile,
            force,
        } => {
            if (profile.is_some() || from.is_some()) && !force {
                let own = store.own_data();
                if !own.is_empty() {
                    anyhow::bail!(
                        ".harna/{} already hold(s) this project's data — a profile would replace it. Add the pieces you want with the edit commands, or pass --force to start over",
                        own.join(", .harna/")
                    );
                }
            }
            store.init()?;
            let mut profile_note = String::new();
            if let Some(spec) = profile {
                use harna_core::generation::structure::profile as profiles;
                let p = profiles::resolve(&spec, profiles::home_dir().as_deref())?;
                for line in profiles::apply(&store, &root, &p)? {
                    profile_note.push_str(&format!("\n{line}"));
                }
            }
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
                    anyhow::bail!(
                        "no manifest.toml / rules.toml found under {}",
                        src.display()
                    );
                }
            }
            println!(
                "initialized {}",
                store.manifest_path().parent().unwrap().display()
            );
            if store.pin_map_eol()? {
                println!(
                    ".gitattributes: the code map stays LF on checkout (no phantom changes under core.autocrlf)"
                );
            }
            // AGENTS.md is the humans' file (0.3.47, H11): init offers,
            // `--agents` opts in, and only the managed block is added.
            use harna_core::generation::views::agents::{self, Opted};
            if agents {
                let found = agents::opt_in(&root, harna_core::schema::store::AGENTS_TEMPLATE)?;
                agents::sync(
                    &root,
                    &store.load_manifest()?,
                    &store.load_effective_rules()?,
                    &store.load_commands()?,
                    true,
                )?;
                println!(
                    "{}",
                    match found {
                        Opted::Missing =>
                            "AGENTS.md written: the harna contract plus its managed block",
                        Opted::Hand =>
                            "AGENTS.md: managed harna block appended — your prose untouched",
                        Opted::Managed => "AGENTS.md already carries the harna block — left as is",
                    }
                );
            } else {
                match agents::opted(&root) {
                    Opted::Managed => {}
                    Opted::Hand => println!(
                        "AGENTS.md left untouched — `harna init --agents` appends harna's managed block (rules, commands, layers) without rewriting your prose"
                    ),
                    Opted::Missing => println!(
                        "no AGENTS.md — `harna init --agents` writes the harna contract for agents"
                    ),
                }
            }
            if !profile_note.is_empty() {
                println!("{profile_note}");
            }
            println!("next: `harna scan`, add rules, and wire the harna MCP server");
        }
        Command::Rules { scope, env, agent } => {
            let rules = store.load_effective_rules()?;
            let rendered =
                rules.render_for_agent(scope.as_deref().filter(|_| agent), env.as_deref());
            print!("{}", rendered);
        }
        Command::AddRule {
            id,
            text,
            rationale,
            scopes,
            env,
            check,
            severity,
            pattern,
            limit,
            values,
            command,
            scope_signature,
            replace,
        } => commands::rules::add(
            &store,
            Rule {
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
                values,
                command,
                scope_signature,
                status: None,
            },
            replace,
        )?,
        Command::DeprecateRule { id, reason } => {
            commands::rules::deprecate(&store, &id, reason.as_deref())?
        }
        Command::ActivateRule { id } => commands::rules::activate(&store, &id)?,
        Command::RemoveRule { id } => commands::rules::remove(&store, &id)?,
        Command::TryRule { id, text, file, at } => {
            let (path, src) = match (text, file) {
                (Some(t), None) => (at.unwrap_or_else(|| "src/lib.rs".into()), t),
                (None, Some(f)) => {
                    let src = std::fs::read_to_string(root.join(&f))
                        .map_err(|e| anyhow::anyhow!("cannot read `{f}`: {e}"))?;
                    (at.unwrap_or(f), src)
                }
                _ => anyhow::bail!("give --text \"<code>\" or --file <path> (one of them)"),
            };
            let found = harna_core::enforcement::validate::content::pattern::try_rule(
                &store.load_effective_rules()?,
                &id,
                &path,
                &src,
            )?;
            if found.is_empty() {
                println!("rule `{id}` finds nothing in {path} — if it should, check its pattern and scope");
            }
            for v in &found {
                println!("{}", v.render());
            }
        }
        Command::LogDecision {
            title_arg,
            body_arg,
            title,
            body,
            supersedes,
        } => {
            // positional or flagged (1.1.0, proof: `harna log-decision ".."`
            // was refused for want of --title)
            let title = title.or(title_arg).ok_or_else(|| {
                anyhow::anyhow!(
                    "a decision needs a title: harna log-decision \"<title>\" \"<why>\""
                )
            })?;
            let body = body.or(body_arg).ok_or_else(|| {
                anyhow::anyhow!(
                    "a decision needs its why: harna log-decision \"{title}\" \"<why>\""
                )
            })?;
            let path = store.save_decision(&title, &body, supersedes.as_deref())?;
            println!("decision saved: {}", path.display());
        }
        Command::Decisions { chain, archived } => {
            let mut decisions =
                harna_core::schema::journal::decisions::list(&store.decisions_dir())?;
            // the archived tail, on request (0.7.6; deferral archived-decisions-viewer)
            if archived {
                let dir = store
                    .decisions_dir()
                    .parent()
                    .map(|h| h.join("archive").join("decisions"))
                    .unwrap_or_default();
                if dir.is_dir() {
                    decisions.extend(harna_core::schema::journal::decisions::list(&dir)?);
                }
            }
            match chain {
                Some(q) => {
                    println!(
                        "{}",
                        harna_core::schema::journal::decisions::chain(&decisions, q.trim())?
                    );
                }
                None => {
                    println!(
                        "{}",
                        harna_core::schema::journal::decisions::render(&decisions)
                    );
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
        Command::Migrate { dry, check } => {
            let reports = harna_core::schema::store::migrate::run(&root, &store, dry || check)?;
            for r in &reports {
                println!("{}", r.render());
            }
            // every file read back with this harna (0.9.0)
            let problems = harna_core::schema::store::migrate::verify(&store);
            for p in &problems {
                println!("unreadable: {p}");
            }
            if check {
                let pending = reports
                    .iter()
                    .filter(|r| r.action != "already-current")
                    .count();
                if pending > 0 || !problems.is_empty() {
                    anyhow::bail!(
                        "{pending} file(s) need `harna migrate`, {} do not read with this harna",
                        problems.len()
                    );
                }
                println!("every .harna/ file is current and reads with this harna");
            }
        }
        Command::Restore { name } => {
            let restored = harna_core::schema::store::migrate::restore(&store, &name)?;
            println!("{restored}");
        }
        Command::Archive { keep } => {
            // The three ledgers file their own dead weight; this prints it.
            let closed = harna_core::enforcement::parity::archive_closed(&store)?;
            let superseded = harna_core::schema::journal::decisions::archive_superseded(
                &store.decisions_dir(),
                &store.archive_dir(),
            )?;
            let old = harna_core::composition::gates::score::archive_old_snapshots(&store, keep)?;
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
        Command::Templates { cmd } => commands::registers::templates::run(&store, &root, cmd)?,
        Command::Notes { cmd } => commands::notes::run(&store, &root, cmd)?,
        Command::Deferred { cmd } => {
            commands::registers::ledgers::deferred::run(&store, &root, cmd)?
        }
        Command::Terms { cmd } => commands::registers::terms::run(&store, &root, cmd)?,
        Command::NonGoal { cmd } => commands::registers::non_goal::run(&store, &root, cmd)?,
        Command::Lesson { cmd } => commands::registers::lesson::run(&store, &root, cmd)?,
        Command::Quality { cmd } => commands::registers::quality::run(&store, &root, cmd)?,
        Command::Layer { cmd } => commands::manifest::layer(&store, &root, cmd)?,
        Command::Container { cmd } => commands::manifest::container(&store, &root, cmd)?,
        Command::Slot { cmd } => commands::manifest::slot(&store, &root, cmd)?,
        Command::TestKind { cmd } => commands::manifest::test_kind(&store, cmd)?,
        Command::Baseline { cmd } => {
            commands::registers::ledgers::baseline::run(&store, &root, cmd)?
        }
        Command::Commands { cmd } => commands::registers::command::run(&store, &root, cmd)?,
        Command::Roadmap { cmd } => commands::registers::roadmap::run(&store, &root, cmd)?,
        Command::Adopt { by_dir, exclude } => {
            // No manifest: the fresh draft; an adopted repo: the refresh
            // (0.3.43). A malformed manifest fails all the way to the CLI.
            let (summary, body) = harna_core::generation::structure::adopt::run(
                &root,
                &store,
                harna_core::search::digest::MAX_FILES,
                by_dir,
                &exclude,
            )?;
            // Summary to stderr: redirecting stdout must yield clean,
            // paste-ready manifest TOML — a draft is data, not a report.
            eprintln!("{summary}");
            println!("{body}");
        }
        Command::Graph { format } => {
            let digest = store.load_digest()?;
            let manifest = store.load_manifest()?;
            let view = harna_core::generation::views::graph::build(&root, &manifest, &digest);
            let text = match format.as_str() {
                "dot" => harna_core::generation::views::graph::dot(&view),
                "mermaid" => harna_core::generation::views::graph::mermaid(&view),
                other => anyhow::bail!("unknown graph format `{other}` — use dot or mermaid"),
            };
            // The summary goes to stderr: redirecting stdout must yield a
            // clean, import-ready file — an export is data, not a report.
            eprintln!("{} nodes · {} edges", view.nodes.len(), view.edges.len());
            println!("{text}");
        }
        Command::Render {
            check,
            surface,
            scopes,
        } => {
            let manifest = store.load_manifest()?;
            // per-scope instruction files (0.7.6): created on request, then kept
            for s in &scopes {
                let rel = harna_core::generation::views::agents::scoped::create(
                    &root,
                    &manifest,
                    &store.load_effective_rules()?,
                    s,
                )?;
                println!("rendered {rel}");
            }
            if surface {
                // The surface view alone; it reads the committed map.
                let digest = store.load_digest()?;
                let stale = harna_core::generation::views::render::surface_is_stale(
                    &root, &manifest, &digest,
                )?;
                match (stale, check) {
                    (true, true) => anyhow::bail!(
                        "SURFACE.md is stale — run `harna render` and commit the view"
                    ),
                    (true, false) => {
                        let path = harna_core::generation::views::render::write_surface(
                            &root, &manifest, &digest,
                        )?;
                        println!("rendered {}", path.display());
                    }
                    (false, _) => println!("SURFACE.md already current (nothing to write)"),
                }
                return Ok(());
            }
            // every view in one pass, the same one MCP and `finish` run (1.1.0)
            let views = harna_core::generation::views::sync::all(&root, &store, !check)?;
            if check && !views.changed.is_empty() {
                anyhow::bail!(
                    "stale: {} — run `harna render` and commit",
                    views.changed.join(", ")
                );
            }
            print!("{}", views.render(!check));
        }
        Command::Validate {
            changed,
            max_files,
            strict,
            json,
            env,
            check,
        } => commands::gates::validate(
            &store,
            &root,
            changed,
            max_files,
            strict,
            json,
            env.as_deref(),
            check.as_deref(),
        )?,
        Command::Onboarding { section } => print!(
            "{}",
            harna_core::composition::briefing::onboarding::section(
                &store,
                section.as_deref().unwrap_or("")
            )?
        ),
        Command::Status { json } => {
            let s = harna_core::composition::briefing::status::build(&store)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&s)?);
            } else {
                print!("{}", s.render());
            }
        }
        Command::Place { idea } => {
            print!(
                "{}",
                harna_core::composition::briefing::place::build(&store, &idea)?
            );
        }
        Command::Plan { idea } => {
            print!(
                "{}",
                harna_core::composition::briefing::plan::build(&store, &idea)?
            );
        }
        Command::Handoff {
            summary,
            next,
            clear,
        } => {
            if clear {
                if harna_core::schema::journal::handoff::clear(&store)? {
                    println!("handoff cleared (burst resumed)");
                } else {
                    println!("no handoff on disk");
                }
                return Ok(());
            }
            match summary {
                Some(s) => {
                    let open_parity =
                        harna_core::enforcement::parity::open_debt_count(&store).unwrap_or(0);
                    let path =
                        harna_core::schema::journal::handoff::save(&store, &s, &next, open_parity)?;
                    println!("handoff recorded: {}", path.display());
                }
                None => match harna_core::schema::journal::handoff::load(&store) {
                    Some(h) => print!("{}", harna_core::schema::journal::handoff::render(&h)),
                    None => println!(
                        "no handoff recorded — none needed if the burst is one plain commit"
                    ),
                },
            }
        }
        Command::Doctor { json } => commands::gates::doctor(&store, &root, json)?,
        Command::Finish => {
            let f = harna_core::composition::gates::finish::run(&store, &root)?;
            print!("{}", f.render());
            if !f.ready() {
                std::process::exit(2);
            }
        }
        Command::Preflight { json, max_files } => {
            commands::gates::preflight(&store, &root, json, max_files)?
        }
        Command::ValidateIntent { json } => commands::gates::intent(&store, &root, json)?,
        Command::Acknowledge { cmd } => commands::registers::ledgers::ack::run(&store, &root, cmd)?,
        Command::Hooks { cmd } => commands::hooks::run(&store, &root, cmd)?,
        Command::Hook { event } => commands::hooks::answer(&store, &root, &event)?,
        Command::Decompose { file } => {
            let src = std::fs::read_to_string(root.join(&file)).map_err(|_| {
                anyhow::anyhow!("cannot read `{file}` — repo-relative path from the repo root")
            })?;
            let p = harna_core::composition::decompose::plan(&file, &src);
            print!("{}", p.render());
        }
        Command::Tree => {
            let d = store
                .load_digest()
                .map_err(|_| anyhow::anyhow!("no code map — run `harna scan` first"))?;
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
            let mut targets = harna_core::enforcement::validate::coherence::test_coverage::runnable(
                &root, &manifest,
            );
            if let Some(name) = component.clone() {
                if !targets.iter().any(|t| t.component == name)
                    && !harna_core::enforcement::validate::coherence::test_coverage::by_kind::covered(&root, &manifest).contains(&name)
                {
                    anyhow::bail!(
                        "unknown component `{name}` — no registered tests under that name"
                    );
                }
                targets.retain(|t| t.component == name);
            }
            use harna_core::enforcement::validate::coherence::test_coverage::by_kind;
            let mut kinds = by_kind::files(&root, &manifest);
            if let Some(name) = &component {
                kinds.retain(|k| k.covers.contains(name));
            }
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "registered": targets,
                        "by_kind": kinds,
                    }))?
                );
            } else {
                print!(
                    "{}",
                    harna_core::enforcement::validate::coherence::test_coverage::render_targets(
                        &targets
                    )
                );
                print!("{}", by_kind::render(&kinds));
            }
        }
        Command::Metrics {
            json,
            record,
            trend,
        } => {
            if json {
                let r = harna_core::composition::gates::score::full(&store)?;
                println!("{}", serde_json::to_string_pretty(&r)?);
                return Ok(());
            }
            let mut out = String::new();
            if record {
                let s = harna_core::composition::gates::score::build(&store)?;
                let path = harna_core::composition::gates::score::record(&store, &s)?;
                out.push_str(&format!("recorded {}\n", path.display()));
            }
            if trend || !record {
                if trend {
                    let snaps = harna_core::composition::gates::score::list_snapshots(&store)?;
                    let rows = harna_core::composition::gates::score::trend(&snaps);
                    out.push_str(&harna_core::composition::gates::score::render_trend(&rows));
                } else {
                    // One scan, many consumers (0.3.17): metrics, score and
                    // hotspots share the single map read inside `full`.
                    let r = harna_core::composition::gates::score::full(&store)?;
                    out.push_str(&r.metrics.render());
                    out.push_str(&r.score.render());
                    out.push_str(&r.hotspots.render());
                }
            }
            print!("{out}");
        }
        Command::Scaffold {
            layer,
            container,
            names,
            kind,
            path,
            description,
            role,
            empty,
            new_crate,
        } => {
            // several names with --path: the folder they go in (0.4.10)
            let many = names.len() > 1;
            let mut new_crate = new_crate;
            // Several components in one call (0.3.49: the ledger field test
            // spent ~50 scaffold calls); each is born as if called alone.
            for name in names {
                let req = harna_core::generation::structure::scaffold::ScaffoldRequest {
                    empty,
                    new_crate: new_crate.take(),
                    layer: layer.clone(),
                    container: container.clone(),
                    path: match &path {
                        Some(p) if many => Some(format!(
                            "{}/{}",
                            p.trim_end_matches('/'),
                            harna_core::generation::structure::linkage::module_name(&name)
                        )),
                        p => p.clone(),
                    },
                    name,
                    kind: kind.to_core(),
                    description: description.clone().unwrap_or_default(),
                    role: role.clone(),
                };
                let res = harna_core::generation::structure::scaffold::run(&root, &store, &req)?;
                println!("{}", res.message);
            }
        }
        Command::Move { from, to } => {
            print!(
                "{}",
                harna_core::generation::structure::relocate::relocate(&root, &store, &from, &to)?
                    .render()
            );
            // The map follows the move (0.3.50, household field test: validate
            // kept reporting pre-move findings until a manual scan).
            let r = harna_core::composition::scan::run(
                &store,
                &root,
                harna_core::search::digest::MAX_FILES,
                false,
            )?;
            println!("map: {}", r.render());
        }
        Command::Transiter { from, to, name } => {
            let res = harna_core::generation::structure::scaffold::scaffold_transiter(
                &root, &store, &from, &to, name,
            )?;
            println!("{}", res.message);
        }
        Command::Scan { max_files, full } => {
            let started = std::time::Instant::now();
            let r = harna_core::composition::scan::run(&store, &root, max_files, full)?;
            println!("{} — {} ms", r.render(), started.elapsed().as_millis());
        }
        Command::Checks { id } => commands::rules::checks(id.as_deref())?,
        Command::Tools { id } => print!(
            "{}",
            match id {
                Some(id) => harna_core::composition::briefing::toolbox::render_tool(&id)?,
                None => harna_core::composition::briefing::toolbox::render_index(),
            }
        ),
        Command::Deny => {
            let manifest = store.load_manifest()?;
            let plan = harna_core::generation::views::deny::plan(&root, &manifest)?;
            let path = harna_core::generation::views::deny::generate(&root, &manifest)?;
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
            // banned-api rules land in each covered crate's clippy.toml (0.3.45).
            let rules = store.load_effective_rules()?;
            for path in harna_core::generation::views::clippy::generate(&root, &rules)? {
                println!("clippy.toml bans written: {path}");
            }
        }
        Command::Diff {
            from,
            max_files,
            json,
        } => {
            // Hash-verified reuse (0.3.17): the working-tree map rebuilds
            // from the committed map where the body hash proves a file
            // unchanged — same diff, a fraction of the parsing. The
            // extractor gate inside build_incremental keeps old maps from
            // feeding stale edges.
            let previous = store.load_digest().ok();
            let (fresh, _, _) =
                digest::build_incremental(&root, max_files, previous.as_ref(), false)?;
            let baseline = harna_core::search::diff::digest_at_ref(&root, &from)?;
            let mut d = harna_core::search::diff::diff(&baseline, &fresh);
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
