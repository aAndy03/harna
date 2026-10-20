//! Command groups `layer` and `container` (0.3.42): typed manifest edits —
//! rename a container, set layer/container fields — so no agent hand-edits
//! `manifest.toml` for a `depends_on` or a `gateway`.

use std::path::Path;

use clap::Subcommand;

use harna_core::generation::structure::edit::{self, ContainerSet, LayerSet};
use harna_core::schema::store::Store;

#[derive(Subcommand)]
pub enum LayerCmd {
    /// Set a layer's fields; an empty value clears (`--depends-on ""`).
    Set {
        /// Layer name.
        name: String,
        /// Layers this layer may import, comma-separated (replaces the list).
        #[arg(long)]
        depends_on: Option<String>,
        /// Role from the role catalog (base, engine, gateway, …).
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        description: Option<String>,
        /// Sibling isolation: its containers are peers that may not import
        /// each other (true | false).
        #[arg(long)]
        isolated: Option<bool>,
    },
    /// Take a layer and its containers out of the manifest (files stay).
    Remove { name: String },
}

/// Planned components (0.4.1): the shape of the future, before its code.
#[derive(Subcommand)]
pub enum SlotCmd {
    /// Declare a slot: a component whose code does not exist yet.
    Add {
        /// Container the slot belongs to.
        container: String,
        /// Component name, e.g. warm-cache.
        name: String,
        /// Where its code will live, e.g. crates/app/src/cache/warm.rs.
        #[arg(long)]
        path: String,
        /// What it will do, and behind which boundary.
        #[arg(long, default_value = "")]
        summary: String,
    },
    /// Mark a slot built once its code exists.
    Fill { name: String },
    /// List the open slots.
    List,
}

pub fn slot(store: &Store, root: &Path, cmd: SlotCmd) -> anyhow::Result<()> {
    use harna_core::generation::structure::edit::slots;
    match cmd {
        SlotCmd::Add {
            container,
            name,
            path,
            summary,
        } => print!(
            "{}",
            edit::apply(store, |m| slots::add_slot(
                m, &container, &name, &path, &summary
            ))?
        ),
        SlotCmd::Fill { name } => print!(
            "{}",
            edit::apply(store, |m| slots::fill_slot(root, m, &name))?
        ),
        SlotCmd::List => {
            let m = store.load_manifest()?;
            let open = slots::slots(&m);
            if open.is_empty() {
                println!("no planned slots — declare the shape of the future before its code: `harna slot add <container> <name> --path ...`");
            }
            for (layer, container, s) in open {
                println!("  - {} [{layer}/{container}] at {}", s.name, s.path);
                if !s.summary.is_empty() {
                    println!("    {}", s.summary);
                }
            }
        }
    }
    Ok(())
}

#[derive(Subcommand)]
pub enum ContainerCmd {
    /// Rename a container and relink its layer's sequence (paths move with `harna move`).
    Rename { old: String, new: String },
    /// Take a container out of the manifest (its files stay on disk).
    Remove { name: String },
    /// Register an existing file as a component (what `harna adopt`'s refresh proposes).
    Register {
        /// Container name.
        container: String,
        /// Repo-relative file.
        path: String,
        /// Component name (default: the file stem, kebab case).
        #[arg(long)]
        name: Option<String>,
        /// One line: what the component is for.
        #[arg(long)]
        summary: Option<String>,
    },
    /// Add or remove the tests registered for a container's components —
    /// every component unless --component narrows it (0.3.49).
    Tests {
        /// Container name.
        container: String,
        /// Only these components (repeatable); default: all of them.
        #[arg(long = "component")]
        components: Vec<String>,
        /// Test file to register (repeatable), e.g. crates/x/tests/api.rs.
        #[arg(long = "add")]
        add: Vec<String>,
        /// Test file to unregister (repeatable).
        #[arg(long = "remove")]
        remove: Vec<String>,
    },
    /// Set a container's fields; an empty value clears (`--gateway ""`).
    Set {
        /// Container name.
        name: String,
        /// Source directory the container owns, e.g. src/backend/index.
        #[arg(long)]
        path: Option<String>,
        /// Owned facilities: fs | fs:write | net | process | base, or
        /// crate:NAME for an external crate (crate:rusqlite); comma-separated.
        #[arg(long)]
        gateway: Option<String>,
        /// Role from the role catalog; overrides the layer's role.
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        description: Option<String>,
        /// What only this container constructs/declares: capability types
        /// (CheckedPath) or markers (#[op2), comma-separated.
        #[arg(long)]
        owns: Option<String>,
        /// Counted escape hatches (CheckedPath::unsafe_new), comma-separated.
        #[arg(long)]
        escapes: Option<String>,
        /// Declared surface: the names other containers may import,
        /// comma-separated; everything else stays private (declared-surface).
        #[arg(long)]
        provides: Option<String>,
    },
}

pub fn layer(store: &Store, _root: &Path, cmd: LayerCmd) -> anyhow::Result<()> {
    let (name, depends_on, role, description, isolated) = match cmd {
        LayerCmd::Remove { name } => {
            print!("{}", edit::apply(store, |m| edit::remove_layer(m, &name))?);
            return Ok(());
        }
        LayerCmd::Set {
            name,
            depends_on,
            role,
            description,
            isolated,
        } => (name, depends_on, role, description, isolated),
    };
    let set = LayerSet {
        depends_on: depends_on.as_deref().map(edit::split_list),
        role,
        description,
        isolated,
    };
    print!(
        "{}",
        edit::apply(store, |m| edit::set_layer(m, &name, &set))?
    );
    Ok(())
}

pub fn container(store: &Store, root: &Path, cmd: ContainerCmd) -> anyhow::Result<()> {
    let out = match cmd {
        ContainerCmd::Register {
            container,
            path,
            name,
            summary,
        } => edit::apply(store, |m| {
            edit::register_component(
                root,
                m,
                &container,
                &path,
                name.as_deref(),
                summary.as_deref(),
            )
        })?,
        ContainerCmd::Tests {
            container,
            components,
            add,
            remove,
        } => edit::apply(store, |m| {
            edit::set_tests(root, m, &container, &components, &add, &remove)
        })?,
        ContainerCmd::Remove { name } => edit::apply(store, |m| edit::remove_container(m, &name))?,
        ContainerCmd::Rename { old, new } => {
            edit::apply(store, |m| edit::rename_container(m, &old, &new))?
        }
        ContainerCmd::Set {
            name,
            path,
            gateway,
            role,
            description,
            owns,
            escapes,
            provides,
        } => {
            let set = ContainerSet {
                path,
                gateway,
                role,
                description,
                owns: owns.as_deref().map(edit::split_list),
                escapes: escapes.as_deref().map(edit::split_list),
                provides: provides.as_deref().map(edit::split_list),
            };
            edit::apply(store, |m| edit::set_container(m, &name, &set))?
        }
    };
    print!("{out}");
    Ok(())
}
