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
    },
}

#[derive(Subcommand)]
pub enum ContainerCmd {
    /// Rename a container and relink its layer's sequence (paths move with `harna move`).
    Rename { old: String, new: String },
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
    /// Set a container's fields; an empty value clears (`--gateway ""`).
    Set {
        /// Container name.
        name: String,
        /// Source directory the container owns, e.g. src/backend/index.
        #[arg(long)]
        path: Option<String>,
        /// Owned facilities: fs | fs:write | net | process | base, comma-separated.
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
    },
}

pub fn layer(store: &Store, _root: &Path, cmd: LayerCmd) -> anyhow::Result<()> {
    let LayerCmd::Set {
        name,
        depends_on,
        role,
        description,
    } = cmd;
    let set = LayerSet {
        depends_on: depends_on.as_deref().map(edit::split_list),
        role,
        description,
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
        } => {
            let set = ContainerSet {
                path,
                gateway,
                role,
                description,
                owns: owns.as_deref().map(edit::split_list),
                escapes: escapes.as_deref().map(edit::split_list),
            };
            edit::apply(store, |m| edit::set_container(m, &name, &set))?
        }
    };
    print!("{out}");
    Ok(())
}
