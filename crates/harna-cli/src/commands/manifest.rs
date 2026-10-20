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
        /// Who answers for the layer, comma-separated (`@team`, a mail).
        #[arg(long)]
        owners: Option<String>,
    },
    /// Take a layer and its containers out of the manifest (files stay).
    Remove { name: String },
}

/// Test kinds (0.4.7): where each kind of test lives, what it may reach.
#[derive(Subcommand)]
pub enum TestKindCmd {
    /// Add or replace a test kind (same name replaces).
    Add {
        /// The kind's name, e.g. integration.
        name: String,
        /// Path glob holding it (repeatable): crates/*/tests/**, **/*_tests.rs.
        #[arg(long = "path", required = true)]
        paths: Vec<String>,
        /// Layer it may reach beyond its home's own (repeatable); * = any.
        #[arg(long = "reach")]
        reach: Vec<String>,
        #[arg(long, default_value = "")]
        description: String,
        /// A marker its files contain (regex), e.g. '#\[gpui_kit::test' (a regex: escape the bracket):
        /// files in the globs without it are not of this kind.
        #[arg(long)]
        contains: Option<String>,
    },
    /// Remove a test kind.
    Remove { name: String },
    /// List the declared test kinds.
    List,
}

pub fn test_kind(store: &Store, cmd: TestKindCmd) -> anyhow::Result<()> {
    match cmd {
        TestKindCmd::Add {
            name,
            paths,
            reach,
            description,
            contains,
        } => print!(
            "{}",
            edit::apply(store, |m| edit::test_kinds::add(
                m,
                &name,
                &paths,
                &reach,
                &description,
                contains.as_deref()
            ))?
        ),
        TestKindCmd::Remove { name } => print!(
            "{}",
            edit::apply(store, |m| edit::test_kinds::remove(m, &name))?
        ),
        TestKindCmd::List => {
            let m = store.load_manifest()?;
            if m.test_kinds.is_empty() {
                println!("no test kinds declared — tests stay outside layering; declare one to hold a kind's reach: harna test-kind add integration --path 'crates/*/tests/**' --reach store");
            }
            for k in &m.test_kinds {
                let reach = if k.reach.is_empty() {
                    "its home layer only".to_string()
                } else {
                    k.reach.join(", ")
                };
                println!("  - {}: {} — reach: {reach}", k.name, k.paths.join(", "));
            }
        }
    }
    Ok(())
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
    /// Draft the container's declared surface (provides) from its code:
    /// the root module's re-exports, else its public items (0.4.5).
    Surface {
        /// Container name.
        name: String,
        /// Write the draft as the container's provides.
        #[arg(long)]
        apply: bool,
    },
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
    /// Declare a container over a folder, any language (no files written).
    Add {
        /// Container name.
        name: String,
        /// Layer it sits in (created when new).
        #[arg(long)]
        layer: String,
        /// The folder it governs, e.g. web/src/features/ledger.
        #[arg(long)]
        path: String,
        #[arg(long)]
        description: Option<String>,
    },
    /// Take a component out of the manifest (its files stay).
    Unregister {
        /// Component name.
        component: String,
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
        /// Who answers for the container, comma-separated (`@team`).
        #[arg(long)]
        owners: Option<String>,
    },
}

pub fn layer(store: &Store, _root: &Path, cmd: LayerCmd) -> anyhow::Result<()> {
    let (name, depends_on, role, description, isolated, owners) = match cmd {
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
            owners,
        } => (name, depends_on, role, description, isolated, owners),
    };
    let set = LayerSet {
        depends_on: depends_on.as_deref().map(edit::split_list),
        role,
        description,
        isolated,
        owners: owners.as_deref().map(edit::split_list),
    };
    print!(
        "{}",
        edit::apply(store, |m| edit::set_layer(m, &name, &set))?
    );
    Ok(())
}

pub fn container(store: &Store, root: &Path, cmd: ContainerCmd) -> anyhow::Result<()> {
    let out = match cmd {
        ContainerCmd::Add {
            name,
            layer,
            path,
            description,
        } => edit::apply(store, |m| {
            edit::add_container(m, &layer, &name, &path, description.as_deref())
        })?,
        ContainerCmd::Unregister { component } => {
            edit::apply(store, |m| edit::unregister_component(m, &component))?
        }
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
        ContainerCmd::Surface { name, apply } => {
            use harna_core::generation::structure::edit::surface;
            let m = store.load_manifest()?;
            let d = store.load_digest()?;
            let (source, names) = surface::draft(&m, &d, &name)?;
            if !apply {
                println!(
                    "draft surface of `{name}` ({source}, {} name(s)):",
                    names.len()
                );
                println!("  {}", names.join(", "));
                println!("accept it: harna container surface {name} --apply   (or edit: harna container set {name} --provides ...)");
                return Ok(());
            }
            edit::apply(store, |m| surface::apply(m, &name, &names))?
        }
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
            owners,
        } => {
            let set = ContainerSet {
                owners: owners.as_deref().map(edit::split_list),
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
