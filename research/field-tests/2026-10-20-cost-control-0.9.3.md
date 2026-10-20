# Cost experiment — control half (no architecture tool), 2026-10-20, vs harna 0.9.3

**Cost** (from the run result): 86,159 tokens, 10 tool calls, 2 min 46 s.

Built with cargo / rustc / clippy and judgment only. No `harna` command was run.
Scratch project: `<scratchpad>/cost-control` (deleted at the end).

## Product

Workspace with 6 crates (`crates/*`):

| crate   | kind | depends on                | role |
|---------|------|---------------------------|------|
| domain  | lib  | (none)                    | `Money { minor: i64 }`, `Posting`, `DomainError` |
| storage | lib  | domain                    | `Ledger` trait + `InMemoryLedger` |
| events  | lib  | (none)                    | the single `PostingCreated { account: String, amount_minor: i64 }` + line wire format |
| app     | lib  | domain, storage, events   | `PostingService::create_posting` / `balance_minor` |
| api     | bin  | app, storage, events      | stdin commands -> ledger -> events on stdout |
| worker  | bin  | events                    | events on stdin -> per-account totals |

Results: `cargo build` ok, `cargo test` ok (7 tests, at least one per crate), `cargo clippy --all-targets -D warnings` clean.
End-to-end: `printf 'cash 100\nbank 7\ncash -30\n' | api | worker` -> `bank 7`, `cash 70`.

## Commands run (one line each)

1. `bash -c <inline heredoc scaffold>` — failed: shell quoting error in the tool wrapper (no files written beyond nothing useful)
2. `grep -n "'" /dev/null; echo ok` — shell sanity check
3. Write `scratchpad/setup.sh` (scaffold script)
4. `bash setup.sh` -> inside: `mkdir -p`, `git init -q`, write manifests + sources, `cargo fmt --all`, `cargo build --offline`, `cargo test --offline`, `cargo clippy --offline --all-targets -- -D warnings`, `cargo run -q --offline -p api | cargo run -q --offline -p worker`, `git add -A`, `git commit -qm baseline`
5. Write `scratchpad/probes.sh` (probe driver)
6. `bash probes.sh` -> per probe: `sed`/`printf` mutation, `cargo build --offline`, `cargo test --offline`, `cargo clippy --offline --all-targets -- -D warnings`, `git checkout -q -- .`, `git status --short`
7. probe 5 redo (first sed missed the rustfmt-wrapped signature): `grep -n -A4`, `sed -i` x2, `git diff --stat`, `cargo build --offline`, `cargo test --offline`, `cargo clippy --offline --all-targets -- -D warnings`, `git checkout -q -- .`, `git status --short`, `cargo test --offline` (baseline green again)
8. Write this log
9. `rm -rf <scratchpad>/cost-control` (+ the two helper scripts)

## Call count

- Shell calls (Bash): 6 (1 failed quoting, 1 sanity, setup, probes, probe-5 redo, cleanup)
- File-tool calls (Write): 3 (setup.sh, probes.sh, this log)
- Total tool calls: 9 (plus the final hand-back)
- Wasted: 2 (the failed inline heredoc and the sanity check) and the first probe-5 attempt inside call 6 (sed did not match after rustfmt).

## Probe table

| # | break | caught by | exact message |
|---|-------|-----------|---------------|
| 1a | domain `use storage::Ledger;` (no manifest edit) | compiler (crate boundary) | `error[E0432]: unresolved import `storage`` |
| 1b | same + `storage` added to domain's Cargo.toml | cargo resolver | `error: cyclic package dependency: package `domain v0.1.0 (...)` depends on itself. Cycle:` |
| 2a | worker `use api::handle;` (no manifest edit) | compiler | `error[E0432]: unresolved import `api`` |
| 2b | same + `api` added to worker's Cargo.toml | compiler (api is bin-only, so cargo drops the dep) | `warning: ... ignoring invalid dependency `api` which is missing a lib target` then `error[E0432]: unresolved import `api`` |
| 3 | worker Cargo.toml depends on `api`, no import | **nothing** (build/test/clippy exit 0) | only a cargo warning: `ignoring invalid dependency `api` which is missing a lib target` |
| 3b | (control) worker depends on lib crate `storage`, no import | my guard: `unused_crate_dependencies = "deny"` | `error: extern crate `storage` is unused in crate `worker`` |
| 4 | `amount_minor` -> `amount_cents` in one service's copy | cannot happen: both services use the single `events` crate; there is no second copy. Renaming it in the shared crate is caught by the compiler in every user | `error[E0560]: struct `PostingCreated` has no field named `amount_minor`` (app) / `error[E0609]: no field `amount_minor` on type `PostingCreated`` (worker) |
| 5 | `PostingService::create_posting` gains `memo: &str` | compiler, at every caller (api crate + app's own tests) | `error[E0061]: this method takes 3 arguments but 2 arguments were supplied` |

Caveat on 4: the wire format is positional (`account\tamount`), so the field name is not on the wire; a
cross-version deploy (old worker, new api) would not be caught by anything at build time — out of scope here.

Caveat on 3: the dependency is inert today only because `api` has no lib target. If `api` ever grows a
`lib.rs` (common, to make handlers testable), the 3b guard starts covering it; until then nothing flags it.

## Guards added and their cost

| guard | what | cost |
|-------|------|------|
| crate-per-layer workspace | domain/storage/app/events/api/worker as separate packages so Cargo enforces the import graph | design choice, no extra calls; it is what caught 1, 2 and 5 |
| single shared `events` crate | one definition of `PostingCreated` for both services | ~25 lines incl. test; makes probe 4 impossible |
| `[workspace.lints.rust] unused_crate_dependencies = "deny"` + `[lints] workspace = true` in 6 manifests | rejects declared-but-unused lib deps | ~15 manifest lines, 0 extra calls (written in the scaffold); caught 3b, not 3 |

No architecture test (e.g. a `cargo metadata` allow-list of edges) was added; that is the guard that would
have caught probe 3, and in a normal project of this size I would not have written it.
