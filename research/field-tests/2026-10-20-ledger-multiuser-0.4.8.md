# Field test 049 - ledger (multi-user offline bookkeeping), harna 0.4.8

## What I built
Cargo workspace with profile `rust-gpui-desktop` (gpui-kit 0.8):
- `crates/shared` (ledger-core, layer shared): `Money` (i64 minor units, checked arithmetic, exact parse/Display), `PasswordHashString` (argon2id, caller-supplied salt), `User`, `Account`, `Category`, `Transaction`, `Ledger` (owner-checked add/balance). 8 unit tests.
- `crates/platform` (ledger-store, layer platform): `atomic_file` (tmp + fsync + rename), `UserStore` (register/authenticate, OS-RNG salt), `AccountStore` (per-user list). 6 unit tests + 1 integration test (`tests/login_flow.rs`: two users, isolation, reopen).
- `crates/accounts` (feature): `LoginView` (password input, argon2 verify on `background_spawn`, one error message for unknown user/wrong password), `AccountsView` (rows keyed by AccountId).
- `crates/app` (shell): `AppRoot` switching login -> accounts; `main`.
- UI crates were only `cargo check`ed (compiled once; 2 trivial import errors fixed). Not run in a window. No gpui UI integration tests written (time-box).
- Not built: transaction persistence, category UI, account creation screen, password-change.
- 2 git commits; pre-commit hook installed and ran. Final tests: all 15 pass.

## Commands used and verdict
| command | helped? |
|---|---|
| `harna --help`, `profile list/show` | Yes. Profile show lists layers/rules/next steps clearly. |
| `harna quality add` x6, `quality list` | Yes, cheap and the qualities then appear in `place` output and digest. See friction 1, 2. |
| `harna init --profile rust-gpui-desktop --agents` | Yes. Wrote manifest/rules/qualities/terms + AGENTS.md. |
| `harna slot add` x6 + `scaffold` | Slots were auto-filled by scaffold (nice, `slot list` then showed none). |
| `harna scaffold` | Mixed: registers in manifest, but writes a stub I had to delete and rewrite; no Cargo.toml/lib.rs. See friction 4, 5, 6. |
| `harna add-rule --scope-signature ... --check banned-api --value f64` | Excellent. Worked on first try; fired on a `pub fn ... -> f64` appended to transaction.rs (file referencing Money), not on comments, not on files without Money. |
| `harna scan`, `validate` | Yes. Found a real gateway-ownership violation (std::fs in app main). |
| `harna deny` | Produces deny.toml; staleness reminder (friction 8). |
| `harna test-kind add` (integration reach=shared, unit) | Yes; `test-reach` fired correctly on a dev-dep from accounts to app. |
| `harna add-rule`/`validate` crate-conformity + cargo-layering probes | Both fired on a Cargo.toml edit (gpui-kit in shared; shared->platform). Good. |
| `harna place` | Gave rules + qualities; before scan said "CODE MAP MISSING" (clear). Only used once; mostly boilerplate. |
| `harna parity open ui-slice` | "0 open - nothing owed" - correct but added no value here. |
| `harna log-decision`, `handoff`, `hooks install`, `render`, `preflight`, `status`, `digest`, `doctor`, `tests` | Worked. |

## Friction / bugs (exact commands and outputs)
1. **Qualities cannot be written before init; the requested order is impossible.**
   `harna quality add money-exact --statement "..." ` in a fresh git repo ->
   `error: writing qualities.toml: staging C:\...\.harna\qualities.toml: The system cannot find the path specified. (os error 3)`.
   Meanwhile `harna quality list` works without .harna. Should either create `.harna/` or say "run harna init first". Then init --profile writes the profile's own qualities (only 2: ui-thread-never-blocks, keyboard-and-focus-contract), so "qualities first" = init first anyway.
2. `harna quality add` prints `quality \`money-exact\` recorded (3)` - the parenthesised number (3,4,5...) is unexplained (total count? it includes profile ones).
3. `harna scaffold --layer features --container accounts --name a --name b --kind primitive` on a workspace with no crate yet:
   `error: layer \`features\` has no container in a crate yet and this is a Cargo workspace — pass --path once for the first component (e.g. --path crates/platform/src/accounts/login_view)`.
   The suggested example path points into the *platform* crate (wrong). Same for shell: layer `shell` already declares container `app` with path `crates/app/src` in the profile, yet the error claims "no container in a crate yet" (the crate dir simply did not exist). Then `--path` with 2 `--name`s: `error: --path names one file: pass one --name with it`. Needed 3 attempts.
4. `harna scaffold ... --name x user account` (positional) -> `error: unexpected argument 'user' found` (my error, but `--name` repeatable-vs-multi-value is easy to trip).
5. Scaffold output always ends `not wired into the module tree: no crate root (src/lib.rs or src/main.rs) above it` and never creates Cargo.toml/lib.rs/main.rs, nor adds `pub mod`. For a profile workspace, a `harna scaffold --crate` that makes the crate skeleton would remove the most manual work.
6. Scaffold stubs (`pub struct Money { // TODO }`) are written to the path I then wanted to fill; the Write tool refuses overwrite without reading, so I deleted them. Intent was "born modular", but the stub content is generic and not what you keep. Option: `--empty`.
7. `harna scaffold` re-serialises manifest.toml: the profile's header comment block ("Layers of a gpui-kit desktop app ... Prune freely.") got reformatted/partly dropped, and inline arrays were expanded to multi-line. Mild, but hand comments in the manifest do not survive.
8. `deny-sync` info is noisy and oddly formatted: `[?deny-sync] deny.toml:- — deny.toml not generated yet` then, after `harna deny`, `deny.toml is stale — the manifest's layer topology changed` after *every* scaffold that adds a container, even if topology of layers did not change in a way relevant to deny. The `:-` in `deny.toml:-` looks like a missing line number placeholder (same in `crates/shared/Cargo.toml:-`).
9. `harna init --agents` then `harna preflight` immediately fails after normal manifest edits: `[FAIL] views: stale: AGENTS.md — run \`harna render\` and commit`. Correct, but `scaffold`/`slot add` could auto-render or warn. I'd also like `validate` (not only preflight) to flag it.
10. `gateway-ownership` message: `module touches \`fs\` (std::fs::create_dir) outside its registered gateway` while the code called `std::fs::create_dir_all`. Message trims the name. The finding itself was a TRUE positive and the fix hint was good.
11. `banned-api` fix text for rules without a sanctioned replacement: `use the sanctioned path instead` although none is declared. Likewise `crate-conformity` for a *forbidden* key says `every crate in scope carries these` (reads as "required").
12. Meaning-scoped rule: scope is "declares \bMoney\b". Docs say "declaring"; in practice it also matched a file that only *uses* `Money` in a signature (transaction.rs). That is the better behaviour but the wording is misleading. A file that does arithmetic on money via `minor()` (i64 -> `as f64`) without naming `Money` in any signature would escape; no type-level alternative.
13. Integration test `crates/platform/tests/login_flow.rs` is not registered: `harna tests` says `6 target(s)` (only inline unit tests). `status` says `tests: 6/12 components`. The declared test kind `integration` only guards reach; it does not make the test discoverable. Also a `unit` test kind on `crates/*/src/**` with no reach works but is trivially declared; unclear whether declaring it is needed (I did to avoid unit tests being "uncovered").
14. `no-test-update` fired in the pre-commit hook for atomic-file (I added `ensure_dir` and did not extend a test) - true positive, informational, fine. Its fix hint (`record in the decision log why`) is heavy for a 3-line fn.
15. `parity open ui-slice` produced "8 targets: 0 open, 6 origin, 2 auto-resolved". Zero friction but also zero information; for a greenfield burst the ritual step is ceremony.
16. CRLF warning spam on `git add` on Windows (40 lines) - not harna's, but `harna init` could write a `.gitattributes` (`* text=auto eol=lf`) or the generated files could use platform EOL. The digest shards under .harna/digest are committed; they churn at each scan.
17. `harna digest` output has `order: unordered (layer.sequence empty)` x5 - noise in onboarding for profile-init projects.
18. Tooling (not harna): a very long heredoc written via Bash failed with `unexpected EOF while looking for matching` and nothing ran; irrelevant to harna but note `harna` printing no multi-file helpers.

## Ignored tools and why
`adopt` (greenfield), `decompose`/`move` (no oversize files), `transiter`, `baseline`, `acknowledge`, `deferred`, `roadmap`, `non-goal`, `lesson`, `templates`, `graph`, `metrics`, `dependents/whereis`, `validate-intent`, `container/layer` editing, `archive/migrate/restore`, MCP server. Mostly because a ~600-line slice has no need; `non-goal` (e.g. "no sync/network") and `roadmap` (next steps) would have been natural and I should have used them - the handoff covered it instead.

## Not verified
- UI was only type-checked, never opened, no `#[gpui_kit::test]`. The rule `ui-integration-tests` is unscoped prose, so nothing flagged the missing UI tests (an obvious candidate for a check bound to the features layer: "view files need a test").
- Nothing checks the quality statements themselves (`no-network`, `money-exact`) against code. `no-network` could be a `banned-api`/cargo dependency rule; harna gave no hint to bind a quality to a rule (a `quality add --rule` link would be a real addition). I did not bind them.
- `ui-thread-never-blocks` quality: I satisfied it by hand; nothing enforces it (the `role-async` check goes the other way).

## Final state
- `harna validate`: `no violations found / verdict: PASS` (after fixing the real fs gateway finding).
- `harna preflight`: PASS (validation, doctor 16 ok/1 warn->hook installed, parity, map, views). Tests (`cargo test --workspace`): 15 pass.
- `harna status`: score 85 (B), 12 rules, parity 1 event 0 open, untested: account, accounts-view, app-root, category, login-view, user, handoff recorded with 3 next steps.
- Last commit 79ab6f8; `.harna/handoff.toml` untracked at the end (recorded after commit).
