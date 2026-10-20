# Profiles

Generated from the profiles shipped with harna (`crates/harna-core/profiles/`) — do not edit.

A profile is a starting shape for a kind of project: layers, rules, qualities and vocabulary to prune (`harna init --profile <name>`). Your own go in `~/.harna/profiles/`. 2 shipped.

## `rust-gpui-desktop` v7

Rust desktop app with gpui-kit: app shell, isolated feature crates, app components, headless shared capabilities, platform seams

- **Mirrors:** gpui-kit 0.8.0
- **Installs plugins:** gpui-kit

| Layer | May import | Containers |
|---|---|---|
| `shell` | features, ui, services, shared, platform | `app` |
| `features` | ui, shared, platform |  |
| `ui` | shared | `components` |
| `shared` | — | `core`, `commands` |
| `services` | shared, platform | `use-cases` |
| `platform` | shared | `os` |

**Rules:**

- `headless-below-the-ui` — shared and platform crates never depend on gpui-kit: domain types, ports and OS seams stay headless
- `features-by-capability` — Organize by capability: one feature crate keeps its model, views, commands and dialogs together; never global models/, views/, modals/ folders
- `commands-are-the-corridor` — A view reaches a use case only by dispatching a named command on the bus (crates/shared/src/commands); the shell wires each command to its service handler
- `thin-command-bus` — The command bus carries names and payloads only: no logic, few items
- `views-have-ui-tests` — Every view (a file implementing Render) is exercised by a ui test: rendered in a headless window, driven by simulated input, its state asserted
- `platform-behind-seams` — Platform-specific code lives behind a narrow capability seam with a defined fallback; the semantic contract holds on every target

**After init:**

1. create the workspace crates the manifest names (crates/app, crates/ui, crates/shared, crates/services, crates/platform; `harna scaffold ... --new-crate lib` makes each); the UI crates depend on gpui-kit only (`use gpui_kit::*;`); then `harna scan` and a first commit, so place/plan and parity have a map and a baseline
2. say what your product must hold over its life (users, offline or online, data-loss tolerance, privacy): harna quality add
3. add each capability as its own feature crate, model + views + commands together: harna scaffold --layer features --container <feature> --name <feature>-view --kind primitive --path crates/<feature>/src/<feature>_view
4. read the gpui-kit Coding and Design Guides before UI work; `harna tools` lists what harna can hold for you

## `tauri-react` v2

Tauri 2 desktop app: Rust core and commands, React + TypeScript webview with isolated features, one typed IPC module

- **Mirrors:** tauri 2.0.0
- **Installs plugins:** tauri

| Layer | May import | Containers |
|---|---|---|
| `app` | features, shared, ipc | `web-app` |
| `features` | shared, ipc |  |
| `shared` | — | `web-shared` |
| `ipc` | shared, commands | `ipc` |
| `commands` | core | `tauri-commands` |
| `core` | — | `domain` |

**Rules:**

- `core-is-headless` — crates/core never depends on tauri: the domain runs and is tested without a window
- `commands-are-adapters` — A command parses its arguments, calls the core, maps the error; domain logic lives in crates/core
- `features-by-capability` — A feature folder keeps its components, hooks and state together; never global components/, hooks/, stores/ folders for feature code

**After init:**

1. create the crates and the web app the manifest names (src-tauri as the Tauri crate, crates/core for the headless domain, web/src for the React app); `harna scaffold ... --new-crate lib` makes a Rust crate, `harna container add` declares any folder; then `harna scan` and a first commit
2. say what the product must hold over its life (users, offline or synced, data-loss tolerance): harna quality add
3. expose each use case as a #[tauri::command] in src-tauri/src/commands and call it only from web/src/ipc (typed wrappers); `harna dependents src-tauri/src/commands/<x>.rs` then names its TS callers
4. one folder per feature under web/src/features (components + hooks + state together); siblings meet through web/src/shared
