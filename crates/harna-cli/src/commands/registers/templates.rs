//! Command group `templates`: declared fan-out shapes for recurring
//! change types.

use std::path::Path;

use clap::Subcommand;

use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum TemplatesCmd {
    /// Add or replace a template (same id overwrites).
    Add {
        /// Stable id, e.g. schema-shard-change.
        id: String,
        /// Module-path prefix this change type touches (empty = everywhere).
        #[arg(long, default_value = "")]
        scope: String,
        /// The fan-out ceiling an event over this scope shouldn't walk past.
        #[arg(long)]
        max_targets: usize,
        /// Why this shape is declared.
        #[arg(long)]
        rationale: Option<String>,
    },
    /// Remove a template by id.
    Remove { id: String },
    /// List the declared templates.
    List,
}

pub fn run(store: &Store, root: &Path, cmd: TemplatesCmd) -> anyhow::Result<()> {
    let _ = root;
    match cmd {
        TemplatesCmd::Add {
            id,
            scope,
            max_targets,
            rationale,
        } => {
            let mut t = store.load_templates()?;
            t.templates.retain(|x| x.id != id);
            t.templates.push(
                harna_core::schema::registers::parity_templates::ParityTemplate {
                    id: id.clone(),
                    scope: scope.clone(),
                    max_targets,
                    rationale: rationale.unwrap_or_default(),
                },
            );
            t.templates.sort_by(|a, b| a.id.cmp(&b.id));
            store.save_templates(&t)?;
            let shown_scope = if scope.is_empty() {
                "*".to_string()
            } else {
                scope
            };
            println!("template `{id}` declared — matching events over `{shown_scope}` notice at {max_targets}");
        }
        TemplatesCmd::Remove { id } => {
            let mut t = store.load_templates()?;
            let before = t.templates.len();
            t.templates.retain(|x| x.id != id);
            store.save_templates(&t)?;
            if t.templates.len() == before {
                anyhow::bail!("no template `{id}` declared");
            }
            println!("template `{id}` removed");
        }
        TemplatesCmd::List => {
            let t = store.load_templates()?;
            if t.templates.is_empty() {
                println!("no templates declared — `harna templates add <id> --scope <path> --max-targets <n>` declares a recurring change type's fan-out shape");
            }
            for x in &t.templates {
                println!(
                    "- [{}] scope `{}` ceilings {} target(s){}",
                    x.id,
                    if x.scope.is_empty() {
                        "*".to_string()
                    } else {
                        x.scope.clone()
                    },
                    x.max_targets,
                    if x.rationale.is_empty() {
                        String::new()
                    } else {
                        format!(" — why: {}", x.rationale)
                    }
                );
            }
        }
    }
    Ok(())
}
