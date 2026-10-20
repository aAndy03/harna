# Cost experiment, control arm (no architecture tool), 0.9.5

**Cost** (from the run result): 85,915 tokens, 9 tool calls, 2 min 31 s.

Date: 2026-10-20. The agent built the product with cargo, rustc, clippy and its own judgment. It ran no `harna` command.
Scratch project: `scratchpad/cost-control-095`. It was deleted after the run.

## Product

The workspace has 7 crates under `crates/`:

- `domain`: `Money(i64)` in minor units, plus `Posting` and `DomainError`. It has 2 tests.
- `storage`: `MemLedger`, an in-memory store that depends on `domain`. It has 1 test.
- `app`: the `Ledger` use cases. It depends on domain, storage and events. It has 1 test.
- `events`: the single shared `PostingCreated { account: String, amount_minor: i64 }`, with a line wire format. It has 1 test.
- `api` (bin): reads commands on stdin and emits encoded events. It depends on app and events.
- `worker` (bin): consumes the encoded events. It depends on events only and has 1 test.
- `arch-guard`: the layering test only (see Guards).

`cargo build`, `cargo test` (7 tests) and `cargo clippy --workspace --all-targets -- -D warnings` were all green on the first try.

## Commands run (one line each)

1. `mkdir -p cost-control-095 && git init` (first attempt with an inline heredoc script; the Bash tool rejected it with a quote-parse error, so nothing ran)
2. `ls scratchpad/` (checked that nothing had been created)
3. Wrote `cc095-setup.sh`, which scaffolds the workspace, manifests, sources and guard
4. `bash cc095-setup.sh`
5. `cargo build --offline`
6. `cargo test --offline`
7. `cargo clippy --offline --workspace --all-targets -- -D warnings`
8. Wrote `cc095-probes.sh`
9. `git add -A && git commit -m baseline` (inside the probe script)
10. For each probe (1a, 1b, 2a, 2b, 3, 4, 5): apply the edit with `sed`/`printf`, then run `cargo build --offline`, `cargo test --offline`, `cargo clippy --offline --workspace --all-targets -- -D warnings`, then `git checkout -- . && git clean -fd crates`. That is 21 cargo invocations and 7 reverts, all in one script run.
11. `git status --short` (confirmed the tree was clean after the probes)
12. Wrote this log
13. `rm -rf cost-control-095` and the two helper scripts

## Call count

There were 9 tool calls in total:

- 5 shell calls (Bash): the failed inline setup, `ls`, the setup+build+test+clippy run, the probe run and the cleanup.
- 3 file writes: the setup script, the probe script and this log.
- 1 hand-back.

Batching was deliberate: one call scaffolds and verifies, and one call runs all probes.

## Probe table

| Break | Caught by | Exact message |
|---|---|---|
| 1a. The domain imports storage (`pub use storage::MemLedger;`, no Cargo dep) | compiler (build/test/clippy) | `error[E0432]: unresolved import \`storage\`` --> crates\domain\src\lib.rs:2:9 |
| 1b. Same, plus `storage = { path = "../storage" }` in domain/Cargo.toml | cargo resolver | `error: cyclic package dependency: package \`domain v0.1.0 (...)\` depends on itself. Cycle:` |
| 2a. The worker imports from api (`use api::main`), no dep | compiler | `error[E0432]: unresolved import \`api\`` --> crates\worker\src\main.rs:3:48 |
| 2b. Same, plus the dep on api | compiler (cargo also warns) | `warning: worker ... ignoring invalid dependency \`api\` which is missing a lib target` + `error[E0432]: unresolved import \`api\`` |
| 3. worker/Cargo.toml depends on api, no import | **my guard test only** (build and clippy exit 0 with a warning) | test: `layering violations: worker/Cargo.toml depends on \`api\` (allowed: ["events"])`; build/clippy only: `warning: ... ignoring invalid dependency \`api\` which is missing a lib target` |
| 4. `amount_minor` -> `amount_cents` in one service's copy | cannot happen: both services use the one `events` crate, so there is no per-service copy. Renaming the only copy fails in the compiler | `error[E0609]: no field \`amount_minor\` on type \`&PostingCreated\``, `error[E0560]: struct \`PostingCreated\` has no field named \`amount_minor\`` |
| 5. `app::Ledger::post` gains a `memo: &str` parameter | compiler, at the caller in the api crate and in app's own tests | `error[E0061]: this method takes 3 arguments but 2 arguments were supplied` --> crates\api\src\main.rs:10:22 |

Notes:

- Probes 1 and 2 are caught structurally by Cargo/rustc. Probe 1b is a cycle because storage already depends on domain. Probe 2 is unresolvable because `api` is a bin-only crate.
- If `api` had a `lib` target, probe 2b would compile cleanly. Only the guard test would catch it, since it scans both Cargo.toml and the `api::` paths in source.
- Probe 3 is the one break that the toolchain lets through: it is a warning, and `cargo build` exits 0.
- Probe 5 is caught because every caller is inside the workspace. A caller in another repository would not see the break until it upgraded. Nothing here guards a public API (no semver check).

## Guards added and their cost

1. **The `arch-guard` crate with `tests/layering.rs`.** It holds a hand-written allow-list of internal deps per crate and checks two things: (a) the `[dependencies]` keys in each Cargo.toml, and (b) a substring scan for `<crate>::` in each crate's `src`.
   - Cost: 1 extra crate, about 60 lines, written inside the setup script, so it added 0 extra calls. Test runtime is about 0.00s.
   - Limits: the TOML and source parsing are naive. The allow-list must be updated by hand whenever the layering changes, and nothing enforces that.
   - It was the only thing that caught probe 3.
2. **`[workspace.lints]`** (`unsafe_code = "forbid"`, `clippy::all = warn`), plus running clippy with `-D warnings`. Cost: 5 lines. It caught none of the probes.
3. **Architecture choices that act as guards:**
   - One shared `events` crate, which makes probe 4 impossible.
   - Services as bin-only crates, so neither can be imported.
   - Crate-per-layer, so the module boundaries are enforced by Cargo.

   These cost nothing beyond the normal design.
