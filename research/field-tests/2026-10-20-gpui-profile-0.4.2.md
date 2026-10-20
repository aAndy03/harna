# harna 0.4.2 field test: profile `rust-gpui-desktop` on a gpui-kit 0.8.0 app

App: a personal-finance desktop app (transactions table + add dialog, monthly summary, JSON persistence).
Workspace: `crates/{shared,platform,ui,transactions,summary,app}`, one shared `target/` (4.6 GB after check+clippy+test+one debug build).

Gate status at the end: `cargo check --workspace --all-targets` OK, `cargo clippy --workspace --all-targets -- -D warnings` OK,
`cargo fmt --all --check` OK, `cargo test --workspace` 18 tests OK (shared 8, platform 4, transactions 4, summary 2),
`harna validate` 0 errors (2 infos), `harna preflight` PASS. gpui-kit 0.8.0 built fine on Windows 11 (first `cargo check` 2m41s).
The debug binary starts and stays alive 10 s against a seeded ledger with no panic; I did NOT visually verify the window or click the dialog.

## 1. Profile fit

| Part | Verdict | Notes |
|---|---|---|
| Layer `shell` -> container `app` (`crates/app/src`) | Fits | Matches `AppRoot` + `main`. It composes features and wires the one cross-feature flow. |
| Layer `features` (isolated, **no containers declared**) | Fits, needs a manual step | The profile declares no feature container; they appear only through `harna scaffold --layer features --container X`. Worked, and the container `path` was inferred from `--path`. `sibling-isolation` then catches source imports between features. |
| Layer `ui` -> `components` (`crates/ui/src`) | Fits, thin | Only one reusable piece emerged (`amount_text`). Fine. |
| Layer `shared` -> `core` | Fits | Domain types + `TransactionStore` port. `depends_on` nothing is right. |
| Layer `platform` -> `os` with `gateway = "fs, net, process"` | Fits | `JsonStore` (fs) and `clock` live there; no gateway complaints. I would drop `net, process` for an offline app (they are not wrong, just broader than needed). |
| Rule `one-gpui-dependency` (`!toml:dependencies.gpui`) | Works | Verified: a `gpui = ...` dep in ui is an error. Right for 0.8.0: gpui-kit re-exports `gpui::*`. |
| Rule `headless-below-the-ui` | Works | Verified: `gpui-kit` in shared `[dependencies]` is an error. It does NOT see `[dev-dependencies]`. |
| Rule `features-by-capability` | Guidance only | Nothing enforces it; `folder-budget`/`no-name-clusters` (harna's own) would have to. Fine. |
| Rule `never-invent-an-api` | Guidance only, but true | In practice: 2 near-misses caught only by the compiler (`FluentBuilder` is `gpui_kit::prelude::FluentBuilder`, not at root; `Div::id` needs `InteractiveElement`). Nothing in the profile points at where to look. |
| Rule `stable-identity` (scope `crates/app`, `crates/ui`) | **Wrong scope** | The ids that matter live in feature crates (rows, form fields). Scope should include the feature crates (`crates`). Also unenforceable: no check. In this app the DataTable owns row identity, so the rule had nothing to bite. |
| Rule `state-outside-render` (scope `crates/app`, `crates/ui`) | **Wrong scope** | Same: all my retained state (`Entity<TableState>`, `Entity<InputState>`) lives in feature crates. Real gotcha I hit: `Dialog` builder is a *re-run render closure*; entities must be created before `open_dialog`. The profile does not mention it; the gpui-kit docs do. |
| Rule `no-pub-fields-across-the-seam` | Fits | Followed (private fields + getters). Unenforced; a `pub-field` check would be easy. |
| Rule `platform-behind-seams` | Fits | Followed (`clock` fallback, `JsonStore` missing-file fallback). Unenforced. |
| Quality `ui-thread-never-blocks` | Fits, **actionable** | Drove the design: load/save on `cx.background_executor().spawn`. Nothing checks it (a banned-api rule on `std::fs` outside platform would). |
| Quality `keyboard-and-focus-contract` | Fits but unmeasurable | I set initial focus in the dialog; no way to record "done" for it. |
| Terms | Fits | Right words. Missing: `Root` / `open_window` (the thing that enables dialogs), `delegate` (TableDelegate/ListDelegate pattern), `Render vs RenderOnce`. |
| Next steps 1-4 | 1 ok, **2 broken**, 3 ok, 4 ok | See finding F1. |

Missing from the profile for gpui-kit 0.8.0:
- Feature crates need `ui` and `shared` but also benefit from a rule "siblings never share via Cargo.toml either" (see F4).
- No rule/pointer about gpui-kit `Dialog` builders being re-run closures, `Entity` + `Subscription` ownership (`_subscriptions: Vec<Subscription>`), or `Root` (use `gpui_kit::open_window`, not `cx.open_window`). These are the real traps.
- Nothing about dev-dependencies, tests: the gpui-kit testing story (`#[gpui_kit::test]`) is absent. I tested only non-UI crates.

## 2. Tools used

| Tool | Learned from | Caught something real? |
|---|---|---|
| `harna init --agents --profile rust-gpui-desktop` | task | Produced a usable skeleton. |
| `harna digest`, `profile show` | task | Orientation. |
| `harna place` | AGENTS.md | Said `CODE MAP MISSING - run harna scan first` (right), but before any code exists that is all it can say; useful only after the first scan. |
| `harna scaffold` | next step 2 / AGENTS.md | Yes: wired `pub mod` into `lib.rs`, registered components, inferred container path. Starter stubs were generic `struct X { // TODO }`; I replaced them all. |
| `harna scan`, `validate`, `validate --changed` | AGENTS.md | `manifest-path` error when I deleted stub files: correct. `sibling-isolation` real catch (summary importing transactions). `cargo-layering` real catch (shared -> platform). `crate-conformity` real catch (gpui in ui, gpui-kit in shared). `banned-api` real catch (`f64`). |
| `harna add-rule --check banned-api` | `harna tools` | Yes: `money-never-float` (`f64`/`f32 => Money`) fires on a bare `f64`. |
| `harna quality add` | next step 3 | Recorded durability, exactness, privacy. Only shown in digest; nothing verifies. |
| `harna checks` | `harna --help` | Needed to learn what `--check` ids exist. |
| `harna deny` | `validate` info | Generated deny.toml; cargo-deny is not installed here, so unverified. See F4. |
| `harna hooks install` | `doctor` warn | Installed git hook, Claude hooks, CI workflow. Not exercised. |
| `harna parity open` | AGENTS.md | With no commit the baseline is empty, so all 20 targets are "origin"; nothing to resolve. Gives no signal on a first burst. |
| `harna log-decision`, `command add`, `render`, `preflight`, `doctor`, `metrics` | `harna --help` | Worked. `preflight` PASS. |
| `harna tools` | task | Good catalogue; "reach for it" lines were accurate. |

## 3. Findings

### F1 (bug, profile) next step 2 prints a command that fails
```
$ harna scaffold --layer features --container transactions --name transactions-view --path crates/transactions/src/transactions_view
error: the following required arguments were not provided:
  --kind <KIND>
```
The profile's `next 2:` omits the required `--kind`. (The extension-less `--path` is fine: verified, harna appends `.rs`.) Fix: add `--kind primitive` to the profile's next step.

### F2 (friction) component names are global, and scaffold refuses to recreate a registered-but-missing file
I deleted the stub files, kept the manifest entries, then:
```
$ harna scaffold --layer ui --container components --kind primitive --name amount-text
error: component `amount-text` already exists in the manifest - component names are unique across the whole manifest, not per container
$ harna validate --changed
[!manifest-path] crates/ui/src/amount_text.rs:- - component `amount-text` (primitive): path ... does not exist
  fix: run `harna scaffold` or fix the manifest
```
The `fix:` hint tells you to run the command that just refused. Suggest `scaffold --force` / "recreate missing" or change the hint to `harna container remove`/write the file. Global-unique names also forced prefixed names (`transactions-model`, `summary-model`) while the files are `model.rs` in both crates.

### F3 (friction) "lacks !toml:..." message for a *forbidden* key
```
[!crate-conformity] crates/shared/Cargo.toml:- - crate `finance-shared` lacks !toml:dependencies.gpui-kit (rule `headless-below-the-ui`)
```
It means "has a forbidden dependency", but reads as "is missing". The rule text carries the meaning, the line does not. Suggest: `has forbidden toml:dependencies.gpui-kit`. (The check itself works. My first test appended the dep under `[dev-dependencies]`, which is not flagged; dev-deps are a bypass, arguably intended.)

### F4 (gap) isolation holds for source imports, not for Cargo.toml, and deny.toml explicitly permits siblings
Adding `finance-transactions.workspace = true` to summary's `[dependencies]`:
```
$ harna validate        # -> verdict: PASS, 0 errors
```
Same edit as a `use finance_transactions::...` line:
```
[!sibling-isolation] crates/summary/src/view.rs:64 - import `finance_transactions::view::TransactionsView` reaches sibling container `crates/transactions/src` of isolated layer `features`
```
`cargo-layering` does catch shared -> platform (`dependency finance-platform reaches layer(s) platform that shared do(es) not declare`) but treats same-layer isolated siblings as allowed, and generated `deny.toml` lists them as wrappers of each other:
```
{ crate = "finance-summary", wrappers = ["finance-app", "finance-transactions"], reason = "layer(s) features only" },
{ crate = "finance-transactions", wrappers = ["finance-app", "finance-summary"], ... }
```
So `isolated = true` is not enforced at compile-time layering. A declared-but-unused sibling dep is the cheap way to open the door. Fix in `cargo-layering` and `harna deny` (drop same-layer wrappers for isolated layers).

### F5 (false positive, info) `no-copy-paste` on idiomatic code
```
[?no-copy-paste] crates/shared/src/transaction.rs and crates/transactions/src/form.rs share 4 normalized block(s)
[?no-copy-paste] crates/app/src/app_root.rs and crates/transactions/src/view.rs share 2 normalized block(s)
```
These are `use` blocks / `impl Render` boilerplate / `fn fmt` bodies. Info only, but it is noise that agents are told to act on ("decide the single true home"). Import lists should be excluded.

### F6 (gap) first-burst ritual has nothing to say
`harna place` before the first scan: `similar: CODE MAP MISSING`. `harna parity open` with no commit: 20/20 "origin". Both are correct and both tell you to run something else first (scan; commit). Reorder the profile next steps: `scan` and an initial commit before `place`/`parity`.

### F7 (gap) rules in the profile that cannot be checked
`stable-identity`, `state-outside-render`, `no-pub-fields-across-the-seam`, `features-by-capability`, `platform-behind-seams`, `never-invent-an-api` have no `check`. Candidates that the existing check kit could cover: `banned-api` for `std::fs` outside platform (the `ui-thread-never-blocks` quality), a `pub-field` check; `gpui_kit::` `cx.new(` inside `fn render` is a pattern check.

### F8 (friction, Windows) scaffold/`Write` interaction
Scaffold-created stub files must be `Read` before an editor tool can overwrite them; I deleted them instead (leading to F2). Not harna's fault, but scaffolding stubs that are always replaced makes this a common path. A `--empty` flag (file with only the doc comment) would help.

### F9 (good)
- `scaffold` wiring `pub mod` into `lib.rs`/`main.rs` and inferring the container path.
- `validate` catches: sibling import, gpui in ui, gpui-kit in shared, `f64`, shared->platform, deleted manifest paths, all with file:line and a `fix:` line.
- `banned-api` with `=> replacement` is exactly right for "money is never a float".
- `harna tools` and `harna checks` as discovery.
- `preflight` one-screen PASS/FAIL with the honest "tests NOT RUN" line.
- The profile's `mirrors: gpui-kit 0.8.0` and rule rationales cite the Coding Guides: easy to trust.

### Things in gpui-kit 0.8.0 worth knowing (not harna bugs)
- `FluentBuilder` is `gpui_kit::prelude::FluentBuilder`.
- `DataTable`: `TableDelegate::{columns_count, rows_count, column, render_td}`; refresh with `table.delegate_mut()` then `table.refresh(cx)`.
- `Dialog` builder is a re-run closure: create `Entity<AddTransactionForm>` before `window.open_dialog`; one-time focus guard via `Cell<bool>`; `DialogButtonProps::default().ok_text("Add").show_cancel(true)`; `on_ok` returns `bool` (false keeps it open, used for validation errors).
- `Select` over `Vec<&'static str>`: `selected_value()` yields `Option<&&str>`.
