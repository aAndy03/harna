//! `harna assume` (1.1.2): record what was assumed, hand the owner the
//! next questions, settle them.

use clap::Subcommand;

use harna_core::schema::registers::assumptions::{self, Assumption};
use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum AssumeCmd {
    /// Record an assumption the code now follows (same id replaces).
    Add {
        /// Stable id, e.g. accounts-are-personal.
        id: String,
        /// What was assumed, as a statement.
        #[arg(long)]
        statement: String,
        /// The question that settles it, put to the owner.
        #[arg(long)]
        question: String,
        /// What the code does meanwhile, if not the statement itself.
        #[arg(long, default_value = "")]
        default: String,
        /// What the answer would change.
        #[arg(long, default_value = "")]
        impact: String,
        /// The roadmap milestone it must be settled by.
        #[arg(long, default_value = "")]
        by: String,
        /// Hold the release until it is answered.
        #[arg(long)]
        blocking: bool,
    },
    /// The owner confirmed it: the code keeps following it.
    Confirm {
        id: String,
        /// The owner's words, if any.
        #[arg(long, default_value = "")]
        answer: String,
    },
    /// The owner corrected it: say what is true instead.
    Correct {
        id: String,
        #[arg(long)]
        answer: String,
    },
    /// Every assumption, open first.
    List,
    /// The next questions for the owner: blocking first, then by milestone.
    Next {
        /// How many to ask at once.
        #[arg(long, default_value_t = 3)]
        n: usize,
    },
}

pub fn run(store: &Store, cmd: AssumeCmd) -> anyhow::Result<()> {
    let mut reg = store.load_assumptions()?;
    match cmd {
        AssumeCmd::Add {
            id,
            statement,
            question,
            default,
            impact,
            by,
            blocking,
        } => {
            if !by.is_empty() && store.load_roadmap()?.find(&by).is_none() {
                anyhow::bail!("no milestone `{by}` on the roadmap (harna roadmap list)");
            }
            reg.upsert(Assumption {
                id: id.clone(),
                statement,
                question,
                default,
                impact,
                by,
                blocking,
                state: "open".into(),
                answer: String::new(),
                recorded: chrono::Local::now().format("%Y-%m-%d").to_string(),
            });
            store.save_assumptions(&reg)?;
            println!(
                "assumption `{id}` recorded ({} open) — `harna assume next` hands the owner the questions",
                reg.open().len()
            );
        }
        AssumeCmd::Confirm { id, answer } => {
            let line = reg.settle(&id, "confirmed", &answer)?.line();
            store.save_assumptions(&reg)?;
            println!("{line}");
        }
        AssumeCmd::Correct { id, answer } => {
            let line = reg.settle(&id, "corrected", &answer)?.line();
            store.save_assumptions(&reg)?;
            println!("{line}\nthe code follows the answer now: change what it affects");
        }
        AssumeCmd::List => {
            if reg.is_empty() {
                println!("no assumptions recorded");
            }
            let mut all: Vec<&Assumption> = reg.assumptions.iter().collect();
            all.sort_by_key(|a| (!a.is_open(), !a.blocking, a.id.clone()));
            for a in all {
                println!("{}", a.line());
            }
        }
        AssumeCmd::Next { n } => {
            let order: Vec<String> = store
                .load_roadmap()?
                .milestones
                .iter()
                .map(|m| m.id.clone())
                .collect();
            print!("{}", assumptions::ask(&reg.next(n, &order)));
        }
    }
    Ok(())
}
