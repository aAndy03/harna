# Field test 059 - harna 0.5.8, profile rust-gpui-desktop, gpui-kit 0.8.0

Project: C:\Users\andym\Documente\Proiecte\Developer\fieldtest-radial-059 (deleted after this log was written).

## What was built
Workspace of 5 crates, `cargo check`/`cargo test --workspace`/`cargo clippy -D warnings` all green.
- `shared`: `Money` (i64 cents, parse/format/checked_add), `Category`, `Transaction`/`TransactionId` (private fields + readers), port `TransactionRead`, and `commands/` (`Command` enum, `Outcome`, thin `CommandBus`).
- `services`: `Ledger` (in-memory, seeded) installs itself as the bus handler; handles Edit(begin)/Duplicate/Categorize/Delete.
- `ui` (container `components`, owns `gpui-kit:menus`): `menu::{entry (MenuEntry/ActionId data), host (MenuHost entity), popup (engine v1), radial (engine v2)}`. Engine choice = one line `use radial as engine;` in `menu/mod.rs`.
- `transactions` (feature): `contributions.rs` (row_menu() = four MenuEntry data items + ActionId -> Command mapping), `delegate.rs` (gpui-kit `DataTable`/`TableDelegate`, rows keyed `("transaction-row", domain_id)`), `transactions_view.rs` (subscribes to `TableEvent::RightClickedRow`, hands entries to `MenuHost`, dispatches the Command on the bus, refreshes through the read port). Does NOT implement `TableDelegate::context_menu` (that hook hands the feature a `PopupMenu`, which would make the feature build menus and bind it to one engine).
- `app`: shell wiring ledger -> bus -> view, opens window.
- Tests: 5 domain/service unit tests, 2 contribution tests, 2 radial geometry tests, 2 `#[gpui_kit::test]` UI tests (`ui/tests/menu_host.rs`, `transactions/tests/row_menu.rs`).
Not done: Edit is only a status-line "Editing transaction N" (no edit dialog); I did not launch the window (ran headless tests only).

## Proof: swap the menu without touching the feature - HELD
Commits: `985a706` popup version, `1070df1` UI tests (against popup), `e840b39` widen engine contract, `59b47e7` swap.
`git diff --stat e840b39 HEAD~2` (swap commit 59b47e7 only):
```
 .harna/acknowledged.toml                |  13 +++
 .harna/digest/*                         | (regenerated)
 .harna/manifest.toml                    |   7 ++
 .harna/parity/swap-radial-engine.toml   |  23 +++++
 crates/ui/src/menu/mod.rs               |   3 +-      (use popup as engine -> use radial as engine, + pub mod radial)
 crates/ui/src/menu/radial.rs            | 160 +++
```
Zero lines under crates/transactions, shared, services, app. The two UI tests (written against the popup engine, finding entries by accessibility label) passed unchanged on the radial engine.
Caveat, honestly: before the swap I had to widen the engine contract (`e840b39`: `render(&Presented)` -> `render(&Presented, &App)` in host.rs/popup.rs, 2 lines) because the radial engine needs the theme. That is inside `ui`, but it shows the v1 contract was under-specified; the feature was still untouched.
Real bug found by the test during the swap: radial ring clipped off-window when right-clicking near the left edge ("Delete" at 9 o'clock had negative x, the click missed). Fixed with `clamp_centre` (+ unit test). harna did not and could not catch this; the headless UI test did.

## harna commands used and verdict
| command | helped? |
|---|---|
| `harna --help`, `harna tools` | yes, tools list is a good map; long |
| `harna init --profile rust-gpui-desktop` | yes. Wrote manifest/rules/qualities/terms + installed plugin gpui-kit 0.8.0-7 (12 rules). Next-step hints useful. |
| `harna plugin show gpui-kit` | yes |
| `harna scaffold ... --new-crate lib/bin` | mixed: creates crates, wires `pub mod`, registers in manifest. But does NOT create the workspace root Cargo.toml (I wrote it by hand). See bugs 1-3. |
| `harna container register / set / tests` | yes, the way to register hand-written files |
| `harna scan` | yes, needed before every validate or results are stale |
| `harna validate` / `--changed` | yes, main loop |
| `harna deny` | generated deny.toml; not exercised (cargo-deny not run) |
| `harna place` | partly: "similar:" ranking put feature files first, the "homes by purpose" section (correct: container `components`) was the useful part |
| `harna parity open swap-radial-engine` | ran; "2 targets, 0 open - nothing owed". Correct but zero information; neither blocked nor helped |
| `harna acknowledge add` | yes, used for 2 advisories on popup.rs (intentional parallel engines) |
| `harna log-decision`, `harna add-rule` | worked; add-rule rewrote rules.toml (see bug 11) |
| `harna init --agents`, `harna render` | worked but produced false positives (bug 10) |
| `harna tests`, `harna status`, `harna metrics`, `harna doctor`, `harna preflight` | worked |
Ignored: `harna hooks install` (no need in a throwaway), `harna quality add`, `roadmap`, `handoff`, `baseline`, `slot`, `transiter` (the profile already declares the `commands` transit container; I used the manifest as given), MCP server (not wired; CLI only), `decompose`, `move`, `templates`, `non-goal`/`lesson`.

## Bugs / false positives / confusing messages (exact)
1. `harna scaffold --new-crate` does not create the workspace root Cargo.toml, though the doc says "workspace member". Had to write `[workspace] members=["crates/*"]` manually; `cargo` fails otherwise.
2. Misleading error: `harna scaffold --layer features --container transactions --name transactions-view --kind primitive --new-crate lib --path ...` ->
   `error: --new-crate needs container `transactions` declared with a path (harna container set transactions --path crates/<crate>/src)`; but then `harna container set transactions --path crates/transactions/src` -> `error: no container named `transactions` in the manifest`. The suggested fix cannot work. What worked: scaffold WITHOUT --new-crate (creates the container; message then says "not wired into the module tree: no crate root"), then a second scaffold with --new-crate on the now-existing container, and wire the first file's `mod` by hand (the first file stayed unwired).
3. `harna scaffold --layer shell --container app --name main --kind bin ... --new-crate bin --path crates/app/src/main` -> `error: refusing to overwrite existing file: crates/app/src/main.rs` (the new crate already wrote main.rs). Harmless but the sequence --new-crate + component named main is a trap.
4. Deleting a scaffolded file leaves a dangling manifest entry: `[!manifest-path] ... component `transactions-model` (primitive): path ... does not exist` (error, blocks). No `harna container unregister`/component remove command; I edited manifest.toml by hand (also for the compound `menu` + child `menu-core` stub). Missing tool: `harna component remove`.
5. `[?tests-declared] crates/transactions/tests/row_menu.rs — component `transactions-view` declares ... as a test, but the file contains no test code` -> FALSE POSITIVE: the file has `#[gpui_kit::test]` (the very attribute the profile's ui test-kind and `views-have-ui-tests` are built around). Same for `crates/ui/tests/menu_host.rs`. The check only knows `#[test]`-like attributes.
6. `views-have-ui-tests` (check kind-coverage) behaviour: correctly fired `[?kind-coverage] crates/ui/src/menu/host.rs — component `host` is named by no `ui` test` and for `transactions_view.rs`. It matches by module name/file stem, so my test that used `transactions::TransactionsView` (type, via crate re-export) did NOT count; changing to `use transactions::transactions_view::TransactionsView;` satisfied it. Name-matching is a weak proof of "exercised" (a bare `use` satisfies it), and a re-exported type does not count: false negative/positive in both directions. Severity is info, so it never gates; to gate it needs strict mode.
7. `[?code-pattern] crates/transactions/src/transactions_view.rs:37 — Keep every Subscription ... (rule gpui-kit/subscriptions-are-owned)` -> FALSE POSITIVE: the line is `let subscription = cx.subscribe_in(...)` and it is stored in `_subscriptions: vec![subscription]` on the next statement. Also every code-pattern message prints `[bound rule(s): stable-identity, state-outside-render, state-in-entities, entities-before-dialogs, subscriptions-are-owned]` for each finding, so you cannot tell which rule produced the finding without reading the "(rule ...)" text.
8. `[?single-owner] ... type `Presented` is declared in 2 home(s) in the same crate: popup.rs, radial.rs` and `[?no-copy-paste] popup.rs and radial.rs share 3 normalized block(s)`: both are by-design for interchangeable engines modules behind one contract (the pattern the architecture asks for). Had to `acknowledge add` both. Not a bug, but a pattern the plugin could expect.
9. `harna parity open swap-radial-engine` -> "2 target(s): 0 open - nothing owed". For a swap of the menu engine nothing was proposed (e.g. "UI tests that cover host", "decision to log"). Correct, but no value.
10. `harna init --agents` (re-run on an existing .harna) generated AGENTS.md listing the plugin rule ids; then validate emitted 12x `[?doc-claims] AGENTS.md:90 — `gpui-kit/one-gpui-dependency` does not exist in the repository` (also 91..101). The generated file trips harna's own doc-claims check because plugin rules are namespaced `gpui-kit/<id>`. Preflight stays PASS (info), but validate went from 4 to 16 infos right after `init --agents`. `harna doctor` itself recommends `init --agents` ("no AGENTS.md — agents start every session blind").
11. `harna add-rule` rewrote the whole rules.toml: schema_version 1 -> 2, dropped the profile's header comment, reformatted arrays, added `origin = ""` to unowned rules. Noisy diff in the commit; the file is data so acceptable, but the comment explaining the plugin was lost.
12. `harna container register` prints only "manifest saved — run `harna validate` to confirm"; `harna container tests --add` printed `tests = [...]`. Fine.
13. `[?manifest-path] crates/platform/src — container `os` declares path ... that does not exist yet` stays forever (info) because the profile ships a platform layer and this product needs none. Needed `container remove` to silence; I left it.
14. `harna` creates git commits as author "User <user@example.com>" in the fresh repo? (git identity was not configured in the new repo; not harna's fault.)

## gpui-kit plugin rules: what I probed (deliberate violations in a temporary probe.rs, reachable via `mod`)
- `api-parity` (invented APIs): catches invented ITEMS via `use` and inline paths: `use gpui_kit::component::menu::RadialMenu` -> `[?api-parity] ... `RadialMenu` is not in `component::menu` in gpui_kit 0.8.0`; `DataTabel`, `VirtualTable` in `component::table`; inline `gpui_kit::component::menu::PopupMenuRing::new()` -> "module `component::menu::PopupMenuRing` does not exist" (wording says module for a type/function path). MISSES: (a) invented modules under the `open = true` module: `use gpui_kit::component::radial::RadialMenu;` and `gpui_kit::component::context_ring::Ring::new()` were NOT flagged; (b) invented METHODS on real types: `PopupMenu::radial(window, cx)`, `DataTable::with_radial_menu(&state)`, `PopupMenu::radial_ring` not flagged (only paths are checked, the rule text says "never invent an API"). The two most plausible hallucinations for this task (a `radial` module, a `.radial()` method) are exactly the ones not caught. All severity info.
- `code-pattern`: `state-outside-render` caught `cx.new(|_| 0u32)` inside render (line 24); `stable-identity` caught `.id(("row", ix))`, and ALSO caught my own real mistake `.id(("radial-entry", index))` in radial.rs (legit hit, I fixed it to an action-derived id). `state-in-entities` did not fire on RefCell/Mutex struct fields (only "reached from render"), fine. False positive: subscriptions (bug 7).
- `registry`: `key-bindings-unique` caught the same `cmd-d` twice in scope `Probe` (`bound again — first at ...:17`). `actions-are-handled` caught an unused action (`Orphan` is declared but never used). Gap: an action that is only bound to a key and never handled (`Unhandled`) counts as "used", since "dispatched" includes being bound.
- ownership of `gpui-kit:menus` (container `components`, crates/ui/src): error-level and correct: in the feature, `PopupMenu::build(` -> `[!ownership] ... calls `PopupMenu::build(`, which container `components` owns`; `fn context_menu(` (the natural TableDelegate hook) and `.dropdown_menu(` also flagged (the definition of the hook method counts as a "call" - good here). The honest implementation (events -> MenuHost) validated clean. `gpui-kit:raw-input` capability (`on_mouse_down(`, `on_mouse_down_out(` ...) is declared by the plugin but the profile assigns no owner, so `.on_mouse_down(MouseButton::Right, ..)` in the feature was NOT flagged: capability exists but nothing enforces it unless the project declares `owns`.
- `views-have-ui-tests`: see bug 6. `layer-boundary` + `no-sideways`: a feature importing `services::ledger::Ledger` -> two errors: `[!no-sideways] ... skips the transiter commands (features -> services) with a direct edge` and `[!layer-boundary] ... cross-layer import `services::ledger::Ledger` reaches layer(s) services not in features's allowed set`. Good, clear, blocking.
- `menus-from-contributions`, `ui-integration-tests`, `no-pub-fields-across-the-seam` are prose-only (no check): no enforcement of "a feature never builds a PopupMenu" beyond ownership above, which covers it.

## gpui-kit/API notes that cost time (not harna)
- Inside one `cx.update_window(...)` closure, `TableEvent` subscribers do not run until the update ends; my first UI test failed silently ("menu offers Edit"). Fix: right-click in one update_window, assert in the next.
- Local crates.io index was stale: `failed to select a version for gpui-fast = "^0.1.10"` after adding the `test-support` dev-feature; `cargo update -p gpui-kit` fixed it.
- Full workspace test build of gpui-kit with test-support took ~3 min; incremental ~1-40 s.

## Final state
- `harna validate`: PASS, 0 errors, 16 info (12 doc-claims from generated AGENTS.md, 1 manifest-path platform, 1 subscription false positive, 2 tests-declared false positives), 2 acknowledged.
- `harna preflight`: PASS (validation, doctor 0 fail/1 warn (no pre-commit hook), parity, map, views). Tests run outside preflight: `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` green; `cargo fmt` applied.
- `harna status`: score 94 (A), 14 components, 12/14 tested, 0 open debts.
