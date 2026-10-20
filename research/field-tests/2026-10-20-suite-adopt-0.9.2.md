# Field test 0.9.1 — adopt an existing repository (codex-rs)

Date: 2026-10-20. harna 0.9.1, Windows 11 + Git Bash. Brief: `research/field-tests/suite/adopt-existing.md`.
Target: kept clone `scratchpad/codex`, workspace `codex-rs/` (127 entries, 5336 .rs files, ~160 crates).
Start state of the clone: branch `main` @ 54f9901, clean except two pre-existing untracked
items at the git root (`.harna/`, `AGENTS.md` — leftovers of an earlier run; backed up with sha1 list
and left untouched). Work branch: `harna-ft092`. harna root = `codex-rs/` (a subfolder of the git root).

Because `parity open`/`diff` do not work when the harna root is a git subfolder (bug 1), part 3 was
re-run on a scratch copy `ft092-rs` (codex-rs copied, `git init` so that harna root == git root),
deleted afterwards.

## Summary

| Break | Result |
|---|---|
| Breaking change to `codex_protocol::items::is_safe_plugin_relative_path` | CAUGHT (when harna root == git root): parity targets exactly the 3 files naming it, 68 auto-resolved |
| Function re-exported via `pub use` (`codex_protocol::intersect_effective_permission_profiles`) | MISSED by parity and `dependents` |
| Same break, before any `scan` | CAUGHT by `validate --changed` only when a contract is frozen; MISSED without one |
| `affected --base main` after change to widely used crate | CAUGHT for containers + transitive crates; PARTIAL for services (transitive service missed) |
| Lockfile / root `Cargo.toml` change | CAUGHT ("test everything" + reason) |
| `.harna/` file with `schema_version = 99` | CAUGHT (refused everywhere) |
| `.harna/` file with unknown key | PARTIAL: `migrate --check` names both; `doctor` names only services.toml; every write command silently drops the key |
| Owners on a container cover crate root | CAUGHT (`Cargo.toml`, `tests/`) |

Caught 5, partial 2 (counted as missed below), missed 1 → 5 caught / 3 missed.

Timings (codex-rs, ~6k modules): adopt 10.5 s; first scan 3.0 s (re-scans 4–6 s); full validate
8.8–17.6 s; one-file `validate --changed` 1.2–1.3 s, but 2.6–2.9 s once services are declared
(over the 2 s target); doctor 0.77 s; parity open 0.35 s (bugged) / 10 s (in the copy); affected 2.4 s;
render 0.25 s; contract freeze 0.18 s; baseline record 9.2 s.

## Commands log

| Command | Time | Helped? | Notes |
|---|---|---|---|
| `harna init` (in codex-rs) | 0.06 s | yes | |
| `harna adopt --by-dir --exclude '**/tests/fixtures' --exclude shell-command/src/command_safety/fixtures` | 10.5 s | partly | 114 layers / 162 containers / 6073 modules; fixtures excluded correctly. Prints a 95-layer `depends_on` cycle and says "the edges are real; decide the direction" — misleading, see friction 1. Calibrated budget `add-rule` lines are a nice touch. |
| `harna scan` | 3.0 s | yes | 6073 modules, 20417 signatures. Incremental re-scans always re-scan 767 files ("reused 5321, scanned 767"), even back to back. |
| `harna validate` (full) | 8.8 s (17.6 s later) | partly | 2518 violations: 220 errors, all `no-cycles` on intra-crate module rings (legal in Rust); 1043 copy-paste, 397 granularity, 377 suppression-reason, 250 URLs infos. BLOCKED out of the box. |
| `harna doctor` | 0.77 s | yes | clear |
| `harna container set codex_protocol --owners @openai/protocol-team` | 0.09 s | yes | |
| `harna owners protocol/Cargo.toml protocol/tests/ protocol/src/lib.rs core/src/lib.rs` | 0.07 s | yes | crate root covered |
| `harna render` | 0.25 s | partly | wrote `codex-rs/.github/CODEOWNERS` (see bug 2); respects a hand-written CODEOWNERS |
| `harna owners --changed --base main` | 0.27 s | no (in subfolder) / yes (in copy) | in the subfolder, paths are git-root-relative (`codex-rs/.harna/...`) and all listed as unowned |
| `harna contract freeze codex_protocol` | 0.18 s | yes | 1063 names; some signatures contain junk (bug 5) |
| `harna validate --changed` after the break | 1.2–1.3 s | yes | `[!contract]` names old -> new signature, live, without scan |
| `harna dependents protocol/src/items.rs` | 1.2 s | yes | 71 direct importers with a `via` item |
| `harna dependents protocol/src/permission_profile_intersection.rs` | – | no | "nothing imports ... directly" — the root re-export importer is missed |
| `harna parity open break-protocol-fns` (subfolder) | 0.35 s | no | `error: no committed digest at ref HEAD` although it is committed (bug 1) |
| `harna parity open` (copy, root == git root) | 10 s | yes | 3 exact targets |
| `harna parity resolve <id> <path> --status ack` | 0.35 s | yes | one target per call; no glob / bulk |
| `harna affected --base main` | 2.4 s | yes | containers, cargo test line, transitive crates, services |
| `harna service add …` | fast | yes | |
| `harna migrate --check` | 0.13 s | yes | |
| `harna baseline record` | 9.2 s | yes, with a footgun | swallows contract/parity errors (bug 6) |
| `harna deny` (in the copy) | fast | harmful | overwrites codex's hand-written cargo-deny `deny.toml` (bug 3) |

## Break list — exact outputs

### 1. Breaking change to `krate::item` — CAUGHT (only where harna root == git root)
Change: `protocol/src/items.rs` `pub fn is_safe_plugin_relative_path(path: &str)` -> `(path: &str, strict: bool)`.
Real importers: `analytics/src/reducer.rs`, `core-plugins/src/plugin_metrics.rs`, `core-plugins/src/script_attribution.rs`.
```
parity `break-protocol-fns` gen1: 3 open — owed: ack or noop each (of 72 checked: 1 origin (your change), 68 auto-resolved, 0 hand-resolved)
  ! open      analytics/src/reducer.rs  imports a module with a breaking change: protocol/src/items.rs: signature removed: pub fn is_safe_plugin_relative_path(path: &str) -> bool {
  ! open      core-plugins/src/plugin_metrics.rs  ...
  ! open      core-plugins/src/script_attribution.rs  ...
```
Exactly the files naming it. In the real layout (harna root `codex-rs/`, git root one up):
```
warning: the map is stale for 2 changed file(s) (...) — run `harna scan` first
error: no committed digest at ref `HEAD` — commit the digest first (it is a harna convention)
```
(`git cat-file -e HEAD:codex-rs/.harna/digest/index.json` succeeds.)

Related MISS: a rustfmt-style multi-line signature. Adding a parameter on line 4 of
`pub fn intersect_effective_permission_profiles(\n authority…,\n requested…,\n cwd…,\n) -> …`:
```
structural diff `HEAD` -> `working-tree`:
contents changed (surface stable): protocol/src/permission_profile_intersection.rs
  (body-only edits: governed by validate --changed, not the parity ledger)
```
The digest signature is only the first line (`pub fn intersect_effective_permission_profiles(`), so most
breaking changes in a rustfmt'd codebase are invisible to parity. (The contract check does see it.)

### 2. Function re-exported via `pub use` — MISSED
`protocol/src/lib.rs: pub use permission_profile_intersection::intersect_effective_permission_profiles;`,
imported by `core/src/agent/control/spawn.rs` as `use codex_protocol::intersect_effective_permission_profiles;`.
Renamed the fn on its first line (so the digest sees it):
```
signature removed: protocol/src/permission_profile_intersection.rs :: pub fn intersect_effective_permission_profiles(
signature added: protocol/src/permission_profile_intersection.rs :: pub fn intersect_effective_permission_profiles_v2(
parity `break-protocol-fns` gen2: 3 open — ... (of 73 checked: 2 origin (your change), 68 auto-resolved, 0 hand-resolved)
```
The 3 targets are still only the `is_safe_plugin_relative_path` importers; `core/src/agent/control/spawn.rs` is not a target.
`harna dependents protocol/src/permission_profile_intersection.rs` -> `dependents: nothing imports ... directly`.
README line 56 claims "importers through a crate's re-exports too".

### 3. Same break, before any scan — CAUGHT only with a frozen contract
With `contract freeze codex_protocol`, no scan, `validate --changed` (1.17 s):
```
[!contract] .harna/contracts/codex_protocol.toml — breaking change to contract `codex_protocol` v1.0.0: `is_safe_plugin_relative_path` changed: `pub fn is_safe_plugin_relative_path(path:&str)->bool` -> `pub fn is_safe_plugin_relative_path(path:&str,strict:bool)->bool`
  fix: keep it compatible, or accept the break: harna contract freeze codex_protocol --version 2.0.0 (and tell its importers: harna dependents)
```
Without a contract and before `parity open`, `validate --changed` says nothing about the signature
change: no error, no info, no "run parity open" hint (only unrelated service-talk/no-cycles errors).

### 4. `affected --base main` — CAUGHT, services PARTIAL
```
containers changed: codex_protocol
containers importing them: codex_analytics, codex_app_server, codex_app_server_protocol, codex_core, codex_core_plugins, ... codex_tui, codex_web_search_extension
run: cargo test -p codex-analytics -p codex-app-server ... -p codex-protocol ...
crates depending on them (Cargo, transitive): app_test_support, codex-agent-extension, ... codex-cli, ...
services to redeploy: app-server
```
Services declared: `app-server` (codex_app_server), `cli` (codex_cli), `responses-proxy`.
`codex-cli` is in the transitive list, yet service `cli` is not in "services to redeploy" — a binary that
links the changed crate transitively must be redeployed. In the subfolder layout `--json` "changed"
mixes `.harna/services.toml` (harna-relative) with `codex-rs/.harna/...` (git-relative), 172 files.

### 5. Lockfile / root Cargo.toml — CAUGHT
```
test everything:
  Cargo.lock: dependencies or build settings for every crate/package
containers changed: none
```
Same for root `Cargo.toml`. (No "services to redeploy: all" line in that case.)

### 6. `schema_version = 99` — CAUGHT
rules.toml stamped 99: `migrate --check` rc=1, `rules`, `add-rule`, `migrate` all refuse:
```
error: this repository's .harna data uses schema v99, but this harna understands v5 - update harna ...
```
`validate`: `[!harna-data] .harna/rules.toml — unreadable harna data — checks that depend on it cannot run`; doctor FAIL.
Older stamp (manifest 4 -> 2): `migrate --check` "WOULD migrate schema v2 -> v4 ... nothing written (--dry)" rc=1;
doctor warns `manifest uses schema v4 fields but is stamped v2`. Good. (Wording: "--dry" shown under --check;
"understands v5" while the files are v1/v4 is confusing.)

### 7. Unknown key — PARTIAL (MISSED in doctor, data loss on write)
Added `replicas = 3` to services.toml and `ownr = "@typo"` to a manifest container.
`migrate --check` (rc=1) names both:
```
unreadable: manifest.toml: harna does not know layers[83].containers[0].ownr — the next save would drop it (a typo, or a newer harna's field)
unreadable: services.toml: harna does not know services[0].replicas — the next save would drop it ...
```
`doctor`: only `FAIL readable 1 file(s) do not read: services.toml: ...` — the manifest one is missed.
`validate`: nothing about either.
Then `harna container set codex_core --owners @core` and `harna service set app-server --description x` both
succeed and silently erase `ownr` and `replicas` (grep count 0 afterwards) — the exact loss the message warns of.

### 8. Owners cover crate root — CAUGHT
```
@openai/protocol-team — 3 path(s)
  protocol/Cargo.toml
  protocol/tests/
  protocol/src/lib.rs
```

## Bugs (with repros)

1. **Subfolder root breaks committed-digest lookups.** Repo where harna root is a git subfolder:
   `cd codex/codex-rs && harna init && harna adopt --by-dir > .harna/manifest.toml && harna scan && git add .harna && git commit -m x && <edit a .rs> && harna parity open e1`
   -> `error: no committed digest at ref HEAD`. Same for `harna diff`. Lookup uses `HEAD:.harna/...` instead of
   `HEAD:<prefix>/.harna/...`. `owners --changed --base` and `affected --json` show git-root-relative paths mixed with
   harna-relative ones (`codex-rs/.harna/...` listed as unowned).
2. **CODEOWNERS rendered where GitHub never reads it, with wrong paths.** Same layout: `harna render` writes
   `codex-rs/.github/CODEOWNERS` containing `/protocol/ @team`. GitHub reads only `<git root>/.github|docs|/CODEOWNERS`
   and the path would need to be `/codex-rs/protocol/`. The real hand-written `/.github/CODEOWNERS` at the git root is
   neither merged nor reported. (In-root case is fine: a hand-written CODEOWNERS is left alone and reported.)
   Also `render --check` says "views are current" rc=0 while it reports the declared owners are not in CODEOWNERS.
3. **`harna deny` overwrites a hand-written cargo-deny `deny.toml`.** `cd ft092-rs && harna deny` -> 336-line
   cargo-deny config (licenses/advisories/sources, 17 matching lines) replaced by a 169-line generated bans file
   (0 licenses/advisories). `validate` also reports the hand-written file as `[?deny-sync] deny.toml is stale`.
   `render` protects hand-written CODEOWNERS; `deny` should do the same (or merge into `[bans]` only).
4. **Parity/diff see only the first line of a signature.** Multi-line (rustfmt) `pub fn f(\n a,\n b,\n)`: add a param ->
   `contents changed (surface stable)`, zero targets. Repro in break 1 above.
5. **Contract signature extraction captures the previous item's tail.** `harna contract show codex_protocol`:
   `AdditionalContextKind: pub service_tier:Option<Option<String>>,pub collaboration_mode:..., } #[derive(Debug,Clone,Copy,PartialEq,Eq)]pub enum AdditionalContextKind`
   (also `AgentMessageItem`). A field change in the neighbouring struct would read as a break of this enum.
6. **`baseline record` freezes contract breaks and open parity debts.** With a pending break:
   `harna baseline record` -> `harna baseline show` lists `[contract] .harna/contracts/codex_protocol.toml — 2` and
   `[parity-open] ... — 1`; `validate` then PASSes. These should be un-baselineable like errors are un-acknowledgeable.
7. **Write commands silently drop unknown keys** (break 7) and doctor misses the manifest's unknown key.
8. **`pub use` re-export importers not found** by parity or `dependents` (break 2).
9. **Services: transitive Cargo dependents not redeployed** (break 4).
10. Minor: incremental `scan` re-scans the same 767 files on every run (`reused 5321, scanned 767`, three runs in a row).
11. Minor perf: declaring services adds ~1.4 s to every `validate --changed` (1.23 s -> 2.7–2.9 s on a one-file change, 3 runs each), pushing it over the 2 s target.

## Friction

1. **Adopt's 95-layer cycle is an artifact of `--by-dir`, not real edges.** `utils/` and `ext/` hold crates on both
   sides of `protocol` (utils/cli -> protocol -> utils/string; protocol -> ext/extension-items, ext/agent -> protocol).
   Cargo's crate graph is acyclic by construction, so the advice "decide the direction" is wrong; the fix is to split
   those folder layers. Strict layering is then skipped for 95 of 114 layers — most of the tool is off after adoption.
2. **Out-of-the-box BLOCKED on 220 `no-cycles` errors** for intra-crate module rings, which are legal and common in Rust.
   `baseline record` fixes it in one command, but neither adopt nor the BLOCKED verdict points to it.
3. **`validate --changed` prints repo-wide noise**: "checking 1 changed file(s)…" followed by 195 `name-cluster` infos
   from untouched folders and the 95-name cycle line, ~400 lines. The signal (`[!contract]`) is line 2, but easy to miss.
4. Hand-written CODEOWNERS is not imported into owners at adopt (`core/` is owned in the real file; `harna owners core/src/lib.rs` says unowned).
5. `parity resolve` takes one path per call; no glob/`--all-in <crate>`; a 70-target fan-out is 70 calls.
6. Parity targets are files; the brief/CI think in crates — a per-crate roll-up line would help.
7. `affected` output with ~90 transitive crates is one unreadable line; `--json` exists but the text form could count + truncate.
8. adopt prints the draft with `schema_version = 1` though it is a v4 manifest; the first write re-stamps it.

## Suggestions (priority order)

1. Resolve the harna root against the git top-level (prefix) everywhere: committed digest (`parity open`, `diff`),
   `owners --changed`, `affected` paths, and render CODEOWNERS at the git root with prefixed paths (merging with or
   reporting the existing one). Monorepo-subfolder adoption is the common case for big repos like codex.
2. Never overwrite a hand-written `deny.toml` (detect the missing generated header; refuse or write a separate file / merge `[bans]` only); stop flagging it as stale.
3. Capture the full item signature (to the `{`/`;`) in the digest, normalized as the contract check does, so multi-line signature breaks reach parity.
4. Follow `pub use` re-exports when resolving importers (parity + `dependents`), as the README promises.
5. Forbid baselining `contract` and `parity-open` errors; forbid write commands from dropping unknown keys (refuse the write, as for v99) and make doctor list every unreadable file.
6. adopt `--by-dir`: when a folder layer creates an SCC, split it per crate (or propose the split) instead of telling the user to reverse edges; suggest `harna baseline record` after adoption when only module-ring errors block.
7. `validate --changed` without a contract: emit an info "public signature changed in X — run `harna parity open`" so the break is visible live.
8. `affected`: include services whose crates depend transitively on a changed crate; on "test everything", say "redeploy all services".
9. Scope `validate --changed` infos to the changed files; make the services check incremental to keep the 2 s target.
10. Bulk `parity resolve` (glob / per crate); import an existing CODEOWNERS at adopt.

## Clone restoration

Restored: branch `main` @ 54f9901, `harna-ft092` deleted (`git branch -D`), my stash dropped, `codex-rs/.harna` (incl. empty decisions/), `codex-rs/.github/CODEOWNERS`, ARCHITECTURE.md, SURFACE.md gone. `git status --porcelain --ignored` shows only the two pre-existing untracked items `.harna/` and `AGENTS.md`, whose 170 files match the pre-test sha1 list byte for byte. `git diff` and `git diff --cached` empty. Scratch copy `ft092-rs` and all my temp files deleted.
