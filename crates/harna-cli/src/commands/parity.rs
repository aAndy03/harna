//! Command group `parity`: the change-propagation ledger — open, show,
//! resolve, auto-verify, bulk.

use std::path::Path;

use clap::Subcommand;

use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum ParityCmd {
    /// Open (or re-open = revise) a parity event from structural changes.
    Open {
        /// Event id, e.g. change-store-api.
        id: String,
        /// Event kind (rule-change, manual, digest-diff...).
        #[arg(long)]
        kind: Option<String>,
        /// Git ref whose committed digest is the baseline.
        #[arg(long, default_value = "HEAD")]
        from: String,
        #[arg(long, default_value_t = harna_core::search::digest::MAX_FILES)]
        max_files: usize,
    },
    /// Show parity events with open-debt counts.
    Show {
        #[arg(long)]
        id: Option<String>,
        /// Only events with unresolved targets (debt view).
        #[arg(long)]
        open: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Resolve one target of one event.
    Resolve {
        id: String,
        /// Module path of the target (as listed by `parity show`).
        path: String,
        /// ack (work done) | noop (nothing needed) | auto (verified)
        #[arg(long)]
        status: String,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        by: Option<String>,
    },
    /// Re-verify open targets against the event's baseline (no new generation).
    Auto {
        /// Event id; omit to sweep every event.
        id: Option<String>,
        #[arg(long, default_value_t = harna_core::search::digest::MAX_FILES)]
        max_files: usize,
    },
    /// Resolve several targets of one event in one go.
    Bulk {
        /// Event id.
        id: String,
        /// Status applied to every listed target: ack | noop | auto.
        #[arg(long)]
        status: String,
        /// Note applied to every listed target.
        #[arg(long)]
        note: Option<String>,
        /// File with module paths, one per line; omit to read paths from stdin.
        #[arg(long)]
        paths: Option<std::path::PathBuf>,
        /// Resolve every currently-open target of the event (ignores --paths).
        #[arg(long)]
        all_open: bool,
    },
}

pub fn run(store: &Store, root: &Path, cmd: ParityCmd) -> anyhow::Result<()> {
    match cmd {
        ParityCmd::Open {
            id,
            kind,
            from,
            max_files,
        } => {
            if let Some(n) = harna_core::composition::scan::refresh_if_stale(store, root) {
                eprintln!("{n}");
            }
            let ev = harna_core::enforcement::parity::open(
                root,
                store,
                &id,
                kind.as_deref().unwrap_or("digest-diff"),
                "",
                &from,
                max_files,
            )?;
            println!("{}", ev.summary());
            if let Some(note) = harna_core::search::diff::baseline_note(root, &from) {
                println!("  note: {note}");
            }
            if !ev.open_targets().is_empty() {
                println!("  list: harna parity show --open; resolve: harna parity resolve");
            }
        }
        ParityCmd::Show { id, open, json } => {
            if json {
                let mut events = harna_core::enforcement::parity::list_events(store)?;
                if let Some(only) = id.as_deref() {
                    events.retain(|e| e.id == only);
                    if events.is_empty() {
                        anyhow::bail!("parity event `{only}` not found");
                    }
                }
                if open {
                    events.retain(|e| !e.open_targets().is_empty());
                }
                println!("{}", serde_json::to_string_pretty(&events)?);
            } else {
                print!(
                    "{}",
                    harna_core::enforcement::parity::show(store, id.as_deref(), open)?
                );
            }
        }
        ParityCmd::Resolve {
            id,
            path,
            status,
            note,
            by,
        } => {
            let before = harna_core::enforcement::parity::event(store, &id)
                .ok()
                .and_then(|e| harna_core::enforcement::parity::previous_resolution(&e, &path));
            let ev = harna_core::enforcement::parity::resolve(
                store,
                &id,
                &path,
                &status,
                note.as_deref().unwrap_or(""),
                by.as_deref().unwrap_or("user via harna CLI"),
            )?;
            if let Some(warning) = before {
                eprintln!("warning: {warning}");
            };
            let target = ev.targets.iter().find(|t| t.path == path).unwrap();
            println!(
                "resolved `{path}` -> {} in `{}`; {} open target(s) left",
                target.status,
                ev.id,
                ev.open_targets().len()
            );
        }
        ParityCmd::Auto { id, max_files } => match id.as_deref() {
            Some(id) => {
                let (ev, verified, open) =
                    harna_core::enforcement::parity::auto(root, store, id, max_files)?;
                println!(
                    "parity `{}`: {} auto-verified, {} still open (generation unchanged)",
                    ev.id, verified, open
                );
            }
            None => {
                for line in harna_core::enforcement::parity::auto_all(root, store, max_files)? {
                    println!("{line}");
                }
            }
        },
        ParityCmd::Bulk {
            id,
            status,
            note,
            paths,
            all_open,
        } => {
            // Where the paths come from: --all_open, a file, or stdin.
            let mut entries: Vec<(String, String, String)> = Vec::new();
            if all_open {
                let events = harna_core::enforcement::parity::list_events(store)?;
                let ev = events
                    .iter()
                    .find(|e| e.id == id)
                    .ok_or_else(|| anyhow::anyhow!("parity event `{id}` not found"))?;
                for t in ev.open_targets() {
                    entries.push((
                        t.path.clone(),
                        status.clone(),
                        note.clone().unwrap_or_default(),
                    ));
                }
                if entries.is_empty() {
                    anyhow::bail!("event `{id}` has no open targets");
                }
            } else {
                let text = match paths {
                    Some(f) => std::fs::read_to_string(f)?,
                    None => {
                        use std::io::Read;
                        let mut s = String::new();
                        std::io::stdin().read_to_string(&mut s)?;
                        s
                    }
                };
                for line in text.lines() {
                    let path = line.trim();
                    if path.is_empty() {
                        continue;
                    }
                    entries.push((
                        path.into(),
                        status.clone(),
                        note.clone().unwrap_or_default(),
                    ));
                }
            }
            let (ev, missing) = harna_core::enforcement::parity::resolve_bulk(
                store,
                &id,
                &entries,
                "user via harna CLI",
            )?;
            println!(
                "bulk {status} on `{}`: {} resolved, {} open left",
                ev.id,
                entries.len() - missing.len(),
                ev.open_targets().len()
            );
            for m in missing {
                eprintln!("warning: no target `{m}` in event `{id}`");
            }
        }
    }
    Ok(())
}
