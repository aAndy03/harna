//! Command group `quality` (0.4.0): long-term intent as data — what the
//! software must hold over its life (scale, latency, durability, privacy,
//! connectivity, …), in the project's own words and numbers.

use std::path::Path;

use clap::Subcommand;

use harna_core::schema::registers::qualities::Quality;
use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum QualityCmd {
    /// Add or replace a quality (same id refines it).
    Add {
        /// Stable id, e.g. scale-users.
        id: String,
        /// What must hold, in plain words.
        #[arg(long)]
        statement: String,
        /// How it is measured or bounded: "1M users", "p95 < 200ms".
        #[arg(long, default_value = "")]
        measure: String,
        /// A grouping word you choose: scale, latency, availability,
        /// durability, privacy, security, connectivity, …
        #[arg(long, default_value = "")]
        category: String,
        /// Where it applies (repeatable); none = the whole system.
        #[arg(long = "scope")]
        scope: Vec<String>,
        /// Why it matters.
        #[arg(long, default_value = "")]
        why: String,
        /// A rule that holds it (repeatable): the quality's enforcement.
        #[arg(long = "held-by")]
        held_by: Vec<String>,
    },
    /// Remove a quality by id.
    Remove { id: String },
    /// List the declared qualities.
    List,
}

pub fn run(store: &Store, root: &Path, cmd: QualityCmd) -> anyhow::Result<()> {
    let _ = root;
    match cmd {
        QualityCmd::Add {
            id,
            statement,
            measure,
            category,
            scope,
            why,
            held_by,
        } => {
            // rules are read only to check a link (qualities may come before init)
            let rules = if held_by.is_empty() {
                Default::default()
            } else {
                store.load_effective_rules()?
            };
            if let Some(r) =
                harna_core::schema::registers::qualities::unknown_holder(&held_by, &rules)
            {
                anyhow::bail!("--held-by `{r}` names no rule (harna rules lists them)");
            }
            let mut q = store.load_qualities()?;
            q.upsert(Quality {
                id: id.clone(),
                statement,
                measure,
                category,
                scope,
                why,
                recorded: chrono::Local::now().format("%Y-%m-%d").to_string(),
                held_by,
            })?;
            store.save_qualities(&q)?;
            println!(
                "quality `{id}` recorded ({} declared) — onboarding, plan and place show it to every session working in its scope",
                q.qualities.len()
            );
        }
        QualityCmd::Remove { id } => {
            let mut q = store.load_qualities()?;
            if !q.remove(&id) {
                anyhow::bail!("no quality `{id}` declared");
            }
            store.save_qualities(&q)?;
            println!("quality `{id}` removed — log the decision if the product changed its mind");
        }
        QualityCmd::List => {
            let q = store.load_qualities()?;
            if q.is_empty() {
                println!("no qualities declared — what the software must hold over its life (users, latency, offline, data-loss tolerance, privacy) belongs here as data: `harna quality add`");
            }
            for e in &q.qualities {
                println!("  - {}", e.line());
                if !e.why.is_empty() {
                    println!("    why: {}", e.why);
                }
            }
        }
    }
    Ok(())
}
