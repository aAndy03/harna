//! `harna ui` (1.1.5): the UI catalog — each component's states and
//! variants, and whether its gallery shows them.

use clap::Subcommand;

use harna_core::enforcement::validate::ui::words;
use harna_core::generation::structure::edit;
use harna_core::schema::model::manifest::Component;
use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum UiCmd {
    /// Every UI component: its states, variants and gallery coverage.
    List,
    /// Declare a component's states and/or variants (comma lists; an
    /// empty value clears).
    Set {
        /// Component name, e.g. calendar.
        component: String,
        /// e.g. default,hover,disabled,loading,error,empty
        #[arg(long)]
        states: Option<String>,
        /// e.g. full,mini
        #[arg(long)]
        variants: Option<String>,
    },
}

pub fn run(store: &Store, root: &std::path::Path, cmd: Option<UiCmd>) -> anyhow::Result<()> {
    match cmd.unwrap_or(UiCmd::List) {
        UiCmd::Set {
            component,
            states,
            variants,
        } => {
            let out = edit::apply(store, |m| {
                edit::ui::set(
                    m,
                    &component,
                    states.as_deref().map(edit::split_list),
                    variants.as_deref().map(edit::split_list),
                )
            })?;
            print!("{out}");
        }
        UiCmd::List => {
            let m = store.load_manifest()?;
            let mut ui: Vec<&Component> = Vec::new();
            fn walk<'c>(cs: &'c [Component], out: &mut Vec<&'c Component>) {
                for c in cs {
                    if c.is_ui() {
                        out.push(c);
                    }
                    walk(&c.children, out);
                }
            }
            for c in m.layers.iter().flat_map(|l| &l.containers) {
                walk(&c.components, &mut ui);
            }
            if ui.is_empty() {
                println!("no UI component declares states or variants yet — `harna ui set <component> --states default,error --variants full,mini`");
                return Ok(());
            }
            for c in ui {
                let mut shown = std::collections::BTreeSet::new();
                for t in &c.tests {
                    if let Ok(text) = std::fs::read_to_string(root.join(t)) {
                        shown.extend(words(&text));
                    }
                }
                let mark = |w: &String| {
                    let ok = words(w).iter().all(|x| shown.contains(x));
                    if ok {
                        w.clone()
                    } else {
                        format!("{w}(!)")
                    }
                };
                println!(
                    "{} — {}",
                    c.name,
                    if c.path.is_empty() { "-" } else { &c.path }
                );
                if !c.states.is_empty() {
                    println!(
                        "  states:   {}",
                        c.states.iter().map(mark).collect::<Vec<_>>().join(", ")
                    );
                }
                if !c.variants.is_empty() {
                    println!(
                        "  variants: {}",
                        c.variants.iter().map(mark).collect::<Vec<_>>().join(", ")
                    );
                }
            }
            println!(
                "(!) = no registered test or gallery file names it; `harna validate` says which"
            );
        }
    }
    Ok(())
}
