//! Command group `terms`: the glossary — agent vocabulary maps onto
//! harna's fixed meanings.

use std::path::Path;

use clap::Subcommand;

use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum TermsCmd {
    /// Add or replace a term (same word overwrites).
    Add {
        /// harna's canonical word, e.g. engine.
        word: String,
        /// The meaning as harna defines it.
        #[arg(long)]
        means: String,
        /// Agent vocabulary mapping to this word (repeatable).
        #[arg(long = "alias")]
        aliases: Vec<String>,
    },
    /// Remove a term by word.
    Remove { word: String },
    /// List the glossary.
    List,
}

pub fn run(store: &Store, root: &Path, cmd: TermsCmd) -> anyhow::Result<()> {
    let _ = root;
    match cmd {
        TermsCmd::Add {
            word,
            means,
            aliases,
        } => {
            let mut t = store.load_terms()?;
            t.terms.retain(|e| !e.word.eq_ignore_ascii_case(&word));
            t.terms.push(harna_core::schema::registers::terms::Term {
                word: word.clone(),
                aliases: aliases.clone(),
                means: means.clone(),
            });
            let collisions = t.alias_collisions();
            store.save_terms(&t)?;
            println!("term `{word}` recorded ({} total)", t.terms.len());
            if !collisions.is_empty() {
                println!(
                    "  WATCH: {} alias collision(s) remain — {collisions:?} (doctor names them; one spelling, one meaning)",
                    collisions.len()
                );
            }
        }
        TermsCmd::Remove { word } => {
            let mut t = store.load_terms()?;
            let before = t.terms.len();
            t.terms.retain(|e| !e.word.eq_ignore_ascii_case(&word));
            if t.terms.len() == before {
                anyhow::bail!("no term `{word}` in the glossary");
            }
            store.save_terms(&t)?;
            println!("term `{word}` removed (git history keeps the record)");
        }
        TermsCmd::List => {
            let t = store.load_terms()?;
            if t.terms.is_empty() {
                println!("glossary empty — harna's vocabulary lives in the tool docs until terms are recorded (`harna terms add <word> --means \"..\" [--alias ..]`)");
            }
            for e in &t.terms {
                let aliases = if e.aliases.is_empty() {
                    String::new()
                } else {
                    format!(" (also: {})", e.aliases.join(", "))
                };
                println!("  {}{} = {}", e.word, aliases, e.means);
            }
        }
    }
    Ok(())
}
