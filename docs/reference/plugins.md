# Plugins

Generated from the packs shipped with harna (`crates/harna-core/plugins/`) — do not edit.

A plugin is a framework's conventions as a versioned data pack (`harna plugin add <name>`), vendored into `.harna/plugins/` and applied as `<plugin>/<id>`. It binds harna's generic checks; it ships no code. 3 shipped.

## `gpui-kit` v0.8.0-8

gpui-kit 0.8.0 conventions (Coding Guides, Testing guide) and vocabulary for Rust desktop apps

- **Mirrors:** gpui-kit 0.8.0 (the doctor warns when `Cargo.lock` outruns it)
- **Capabilities** (one container owns each, `--owns`): `gpui-kit:menus`, `gpui-kit:raw-input`

| Rule | Check | What it says |
|---|---|---|
| `gpui-kit/one-gpui-dependency` | `crate-conformity` | Depend on gpui-kit only; GPUI is `use gpui_kit::*;` and the layers are gpui_kit::component, ::base, ::assets, ::platform |
| `gpui-kit/never-invent-an-api` | `api-parity` | Search the current gpui-kit source or docs.rs for the real signature before calling it; never translate a React, CSS or older-GPUI example by analogy |
| `gpui-kit/stable-identity` | `code-pattern` | Repeated elements take ElementIds derived from domain ids, never list indexes |
| `gpui-kit/state-outside-render` | `code-pattern` | Retained state lives in Entity<T> owned by a view; render reads it and never creates it |
| `gpui-kit/state-in-entities` | `code-pattern` | Views share state through Entity<T> handles, never RefCell, Mutex or RwLock reached from render |
| `gpui-kit/no-pub-fields-across-the-seam` | — | Public data types use builders and reader methods, not pub fields |
| `gpui-kit/entities-before-dialogs` | `code-pattern` | Create a dialog's entities (form inputs, state) before window.open_dialog; its builder closure re-runs and must only read them |
| `gpui-kit/subscriptions-are-owned` | `code-pattern` | Keep every Subscription returned by cx.subscribe/observe in the owning entity; dropping it ends the subscription |
| `gpui-kit/ui-integration-tests` | — | Behavior of a view is tested with #[gpui_kit::test] and TestAppContext (render in a headless window, simulate input, assert state); headless crates keep plain unit tests |
| `gpui-kit/menus-from-contributions` | — | Menus are assembled by one engine from entries features contribute as data (label, icon, action, when); a feature never builds a PopupMenu or context menu itself |
| `gpui-kit/actions-are-handled` | `registry` | Every action declared with actions! is handled somewhere (on_action with a handler taking &Action) or dispatched; an unused action is dead UI surface |
| `gpui-kit/key-bindings-unique` | `registry` | One keystroke means one action per key context: never bind the same keys twice in the same context |

## `practices` v1.1.9

Opt-in practices for agent-built code: no swallowed errors or silent fallbacks, no placeholders or debug leftovers, no secret shapes, design tokens instead of raw colors, no copied files

| Rule | Check | What it says |
|---|---|---|
| `practices/errors-are-not-swallowed` | `code-pattern` | An error is handled or passed on, never caught and dropped: no empty catch block, no Err(_) arm that does nothing |
| `practices/no-silent-fallbacks` | `code-pattern` | A failure is not turned into a plausible default: no catch that returns an empty value, no Err(_) arm that returns one |
| `practices/no-placeholders` | `code-pattern` | No placeholder left in shipped code: TODO, FIXME, XXX, HACK, todo!() or unimplemented!() |
| `practices/no-debug-leftovers` | `code-pattern` | No debugging output left in shipped code: dbg!(), console.log(), console.debug() |
| `practices/no-secret-shapes` | `code-pattern` | No secret in source: no AWS key id, private key block, or a password, token or API key written as a literal |
| `practices/design-tokens-not-raw-colors` | `code-pattern` | Interface code uses the design tokens, not raw colors: no #rrggbb, #rgb or rgb()/rgba() literal in a string of interface code (JS, TS, Rust) outside the token file; stylesheets (.css) are not read — keep their colors in custom properties |
| `practices/no-raw-pixel-sizes` | `code-pattern` | Interface code uses the spacing and size scale, not raw pixel literals like '13px' in a string of interface code (JS, TS, Rust); stylesheets (.css) are not read |
| `practices/no-copied-files` | `pattern` | A variant lives in the original component, not in a copy: no Calendar2, auth_new, view_old, x_copy or x_backup files |

## `tauri` v2.0-1

Tauri 2 desktop apps: commands as cross-language edges, IPC behind one module

- **Mirrors:** tauri 2.0.0 (the doctor warns when `Cargo.lock` outruns it)
- **Bridges** (Rust↔TS edges in the map): `tauri-command`

| Rule | Check | What it says |
|---|---|---|
| `tauri/commands-are-registered` | `registry` | Every #[tauri::command] is registered in tauri::generate_handler! (or referenced elsewhere); an unregistered command fails only at runtime |
| `tauri/ipc-behind-one-module` | — | The webview calls Rust through one typed module (invoke wrappers per command); components never call invoke() directly |
| `tauri/commands-return-results` | `code-pattern` | A command that can fail returns Result<T, E> with a serializable E; it never panics across the bridge |
