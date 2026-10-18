# Changelog

All notable changes to harna. Data format (`.harna/`) changes are called out
separately — they are the protocol between tool and repo; the tool migrates
them with `harna migrate`.

## 0.3.40 - 2026-10-18

Craft gates: harna is formatted, lint-clean, and keeps it that way.

### Added

- **`harna hooks install --craft`** (+ MCP `craft`): the pre-commit
  hook runs `cargo fmt --check` and `cargo clippy --workspace
  --all-targets -D warnings` before conformance (bypass once:
  `HARNA_SKIP_CRAFT=1`); the CI workflow gains a `craft` job — fmt,
  clippy as errors, and `cargo test --workspace`, the gate `preflight`
  names as NOT RUN. `hooks status` says which variant is installed.
  harna installs both on itself.
- Scenario: unformatted code is refused at commit with the exact fix,
  and goes in once formatted.

### Changed

- **One-time `cargo fmt`** across the workspace (its own style-only
  commit; 849 → 0 diffs).
- **Clippy: 87 → 0 warnings**, now gated with `-D warnings`. 62 were
  machine-applicable; the rest fixed by hand (checked divisions, type
  aliases for complex types, doc-list indentation, `sort_by_key`,
  `Copy` on a value enum). One deliberate `allow`:
  `open_from_digests` keeps its nine inputs (the seam tests drive), with
  the reason written beside it.
- Formatting pushed five files over 600 lines; split by concern, not
  trimmed: parity tests → `parity/{fixtures, auto/tests}`, digest-shard
  tests → `store/digest_io/tests`, transit tests out of line, MCP memory
  tools (rules + decisions) → `tools/memory.rs`, CLI gate arms →
  `commands/gates.rs` (one shared block/exit helper).

### Known

- One core test failed once under a full workspace run and could not be
  reproduced in 21 further runs (12 isolated, 9 full). Tracked in the
  handoff as a watch item, not claimed fixed.

## 0.3.39 - 2026-10-18

harna's own structure: folders, not file names — enforced on itself.

### Added

- **Folder budgets** (`validate/sizing/folders.rs`):
  - `folder-size` — a folder holds at most `limit` source files (its
    `mod.rs`/`tests.rs` excluded); silent until a rule binds a limit.
  - `name-cluster` — 3+ files in one folder sharing a name token
    (`*_data.rs`, `cmd_*.rs`, `x.rs` beside `x_*.rs`) are a folder that
    was never created; advisory by default, runs even before a repo has
    a manifest.
  - harna binds both on itself as **errors**: `folder-budget` (8 per
    folder) and `no-name-clusters`.
- **`harna move <from> <to>`** (+ MCP `harna_move`): relocate a file or
  folder (with its children folder) and keep every link — manifest
  component paths, tests and container paths, rule scopes (all through
  the typed models, never regex), the `mod` declaration (removed from
  the old parent, wired at the new place), and `crate::old::path` /
  `other_crate::old::path` uses across the workspace. String and char
  literals are never rewritten (fixtures keep their text); what text
  cannot see (`super::`, grouped `use a::{…}`) the report hands to the
  compiler.
- Scenario: an agent's three `*_data.rs` files are named as a missing
  folder, fixed with three `harna_move` calls, and the crate builds.

### Changed

- **harna restructured with `harna move`** (every move through the
  tool; only relative paths fixed by hand, as its report predicts):
  `schema/{model, registers, journal, store}` + `map.rs` (no more
  `*_data.rs`), `enforcement/parity/{policy, statuses}`,
  `validate/{boundaries, coherence, sizing, content}`,
  `generation/{structure, views}`, `composition/{briefing, gates,
  hooks}`, CLI `commands/registers/`. Every folder has a header saying
  what it holds.
- Budget checks are registry data: `takes_limit` in `checks.toml`;
  CLI and MCP add-rule share it (a `born-size` or `depth-budget` limit
  was wrongly rejected before).
- Typed manifest saves keep the human comment header.
- MCP catalog budget is per tool (≤ 480 bytes on average).

### Fixed

- The CLI overflowed Windows' 1 MB main-thread stack in debug builds
  once the command enum grew; it now runs on a 16 MB thread.

### Decided

- Sub-containers are folders (nested container paths and folder
  budgets), not a new manifest type — logged with its reasoning.

## 0.3.38 - 2026-10-18

Linkage builds: a new file is part of the build and the manifest the
moment it exists.

### Added

- **`scaffold` wires the module tree** (`generation::linkage`): the new
  file is declared `pub mod x;` at every level up to its crate root
  (`lib.rs`/`main.rs`), creating a `mod.rs` for a fresh folder and
  respecting an existing `x.rs` parent; idempotent, never writes
  outside a crate, refuses names Rust cannot resolve. The scaffold
  message says what was wired (or why not).
- **Tests are registered at birth and on every scan.** Scaffolded
  components carry their template test in `tests`; `scan` fills every
  empty `tests` field with the tests that exist (inline, or the sibling
  `tests.rs` of an out-of-line `mod tests;`) — a declared field is
  never overwritten, a broken manifest never touched.
- Scenario: an agent scaffolds a module into a real crate and
  `cargo check` passes with no hand-wiring.

### Fixed

- **Scaffolded paths compile.** Generated files were named after
  kebab-case component names (`billing-engine-core.rs`) while the
  compound template declared `pub mod billing_engine_core;` — Rust
  could not resolve it. Generated segments are now snake_case
  (`user-repo` → `user_repo.rs`, loaders `*_loader.rs`); component
  names stay as written.

### Changed

- CLI `scan` and MCP `harna_scan` share one core (`composition::scan`)
  instead of twin copies.

## 0.3.37 - 2026-10-18

Dogfood standards: no source file past the 600-line budget.

### Changed

- **Hotspots split by concern, behaviour unchanged.** `store.rs`
  (1483 → 444 + registers / digest I/O / tests), `parity.rs` (1334 →
  478 + auto / resolve / views / tests), `digest.rs` (1242 → 367 +
  resolve / extract / tests), MCP `tools.rs` (1167 → 541 + requests /
  flow router / resources), CLI `main.rs` (1012 → 547 + `cli.rs` clap
  declarations + `commands/rules.rs`). Five more files past 600
  (`gateway`, `doctor`, `scaffold`, `score`, `test_coverage`) had their
  test modules moved to sibling `tests.rs`. Every moved line was
  checked byte-for-byte against the original; `validate` reports no
  `component-granularity` advisory left. Public paths are unchanged
  (re-exports); all 351 tests pass unmodified in substance.
- **Whole test files are test code** (extractor v4, maps rebuild once):
  `tests.rs`, `*_test(s).rs` and anything under `tests/` carry no deps
  or signatures in the map and skip validate's source checks — the
  exemptions an inline `#[cfg(test)]` section always had. Before, moving
  tests out of line produced boundary/gateway false errors, and a `use`
  inside a test's string literal became a dependency edge.
- **A split is a move, not a breakage.** Parity reads a removed
  signature that reappears identically inside the same module family
  (`store.rs` → `store/*.rs`) as `signature moved to …`, which owes
  importers nothing. Found by dogfooding this release: the split first
  showed 44 false "breaking" importers; with the fix, 0 of 74 targets
  owed an ack. Moves to unrelated modules stay breaking.

### Added

- `crates/harna-cli/tests/rules.rs`: the CLI rule arms end to end (every
  validation branch, the lifecycle) — they moved, so they got a test.

## 0.3.36 - 2026-10-18

Hooks: harna installs its own enforcement.

### Added

- **`harna hooks install`** (and `harna hooks status`, MCP
  `harna_hooks`): one command wires the gates so they run without anyone
  remembering them —
  - **git pre-commit** (`.githooks/pre-commit` + `core.hooksPath`):
    `validate --changed` before every commit. It **fails closed**: no
    harna binary means the commit is refused with the fix (the old
    hand-written hook silently passed); `HARNA_SKIP_HOOK=1` bypasses
    once, visibly.
  - **Claude Code hooks** merged into `.claude/settings.json` (your keys
    and hooks kept): `SessionStart` injects onboarding, `PostToolUse` on
    edits reports violations while they are fresh, `Stop` sends a blocked
    change back once (it honours `stop_hook_active`, so no loops).
  - **CI workflow** (`.github/workflows/harna.yml`): validate + preflight.
  Every file carries a `harna-managed` marker; a file without it is the
  user's and is never overwritten without `--force`. Reinstalling is a
  no-op; the pre-0.3.36 hand-written gate is recognised and upgraded.
- **`harna hook <event>`** — the runtime those hooks call
  (`session-start | post-edit | stop`): quiet when there is nothing to
  say; exit 2 with the errors when a change is blocked.
- Scenarios (`crates/harna-cli/tests/hooks.rs`, real git repos): a bad
  edit reported while fresh, a blocked change sent back once, every
  session starts briefed, install respects the user's own files, and the
  commit gate holds even without harna installed.

### Changed

- `doctor`'s commit-gate hint now says `harna hooks install`.
- `agent-context` declares `construction` in `depends_on` (decision
  logged): the hook orchestrator composes the generator with git wiring.

## 0.3.35 - 2026-10-18

Parity asks for a human only where an importer is affected.

### Changed

- **Origins resolve themselves.** A module the change was made in
  resolves as the new `origin` status: its commit is the record, and an
  "I changed this file" ack added nothing. Importers of an additive
  change (only added modules, signatures or deps upstream) resolve
  `auto` at open time.
- **Breaking changes keep their callers owed.** An importer of a module
  that lost a signature (or was removed) opens `breaking`, with the
  removed items in its reason, and `parity auto` no longer waves it
  through just because its own surface is unchanged. A review carries
  across generations only while the breakage is the same.
- Net effect on this repo: the 0.3.35 event itself owed **0 acks**
  (both targets `origin`); the tier events of 0.3.33 owed 5–12 each.
  Data format: one optional `breaking` field on targets (serde-default;
  existing ledgers load unchanged).

### Added

- **Rubber-stamp rate.** `parity show` ends with the share of hand
  resolutions (ack/noop) that have a blank note or a note copied
  verbatim across targets — on this repo's ledger today: 29 hand
  resolutions, 5 copied (17%).
- Scenarios: a breaking rename owes exactly its caller (and blocks
  validation until the call site is answered); an additive helper owes
  nobody.

## 0.3.34 - 2026-10-18

Tool surface and vocabulary optimized; every tool and its logic kept.

### Changed

- **The tool catalog costs less context.** Every session loads
  `tools/list` before it does any work: it went from ~31 KB to ~19 KB
  (-38%) with all 41 tools kept. Descriptions now say when to call a
  tool and what comes back (11.9 KB -> 4.6 KB, max 200 chars); argument
  docs are one line; `$schema`/`title` boilerplate is stripped. A
  protocol test holds the budget.
- **`HARNA_TOOLS` profiles.** Unset/`full` advertises everything (the
  default, unchanged behaviour); `core` advertises the 15-tool session
  ritual (~8.5 KB); a comma list picks tools by name.
- **The vocabulary is defined once.** A built-in concept map
  (`composition/concepts.toml`, 19 concepts) rides onboarding when a
  repo has recorded no glossary of its own, so tool descriptions no
  longer re-explain parity, transiters or the advisory ledger.

### Added

- **Human-agent scenario tests** (`crates/harna-mcp/tests/scenarios.rs`):
  real collaboration stories replayed over the MCP binary, one fresh
  agent process per step — a preference stated once survives into the
  next session, a refused feature is flagged when a later agent plans
  it, a dead end and its evidence surface before re-suggestion, a
  compaction resumes from the handoff, a reversed rule retires
  visibly, a newcomer on the `core` profile walks the ritual, and an
  accepted warning never silences an error.

## 0.3.33 - 2026-10-18

Review hardening release: a full code review's three tiers, each with
tests and a closed parity event (`tier1-fail-closed`,
`tier2-review-fixes`, `tier3-surface-and-checks`). Data format:
unchanged (no migration); `.harna/` writes are now atomic.

### Fixed (review tier 1 — fail-open gates and data loss)

- **Gates fail closed on unreadable data.** `validate` no longer
  defaults a manifest or rules file that exists but will not load
  (syntax error, newer schema) — that silently skipped every structural
  check. It is now a `harna-data` Error, emitted after severity
  overrides so no binding can downgrade it. One unparseable
  `.harna/parity/*.toml` (e.g. a merge conflict) used to hide every
  open debt in validate *and* preflight; both now fail on it.
- **`validate --changed` with no changed `.rs` files still gates.** It
  used to exit 0 before the manifest-level checks (parity debts,
  manifest paths, decisions, roadmap). Progress lines moved to stderr,
  so `--changed --json` stdout is pure JSON.
- **Crash-safe `.harna/` writes.** New `schema::atomic` component:
  every store save, handoff and parity event stages to a unique sibling,
  fsyncs, then renames over the live file — a crash never leaves a
  truncated manifest.
- **No silent overwrites.** Same-day decisions with one title take
  `_N`; archiving a name that already exists in the archive takes `_N`
  instead of clobbering it; `scaffold` refuses an unreadable manifest
  instead of overwriting it with a one-component default.
- **Multi-line handoffs survive.** Newlines and control characters are
  TOML-escaped; they used to produce an unparseable file that read as
  "no handoff".
- **Parity event ids are plain names.** `parity open ../manifest` is
  refused before touching the disk.
- `validate` deduplicates after sorting (adjacent-only `dedup` let
  duplicates through); the `never_loop` test that failed
  `clippy --all-targets` is fixed.

### Fixed (review tier 2 — MCP surface, paths, git refs)

- **`harna_migrate` / `harna_restore` are callable over MCP.** They took
  a bare bool/string, so their input schema was not an object and every
  call failed with `-32602`. Both now take object arguments
  (`{"dry": bool}`, `{"name": "..."}`).
- **MCP tool failures are `isError` results**, not JSON-RPC `-32603`
  protocol errors — clients show the message to the model.
- **`harna-mcp --version`** answers without serving stdio, so doctor's
  `agent-wiring` probe can see the binary on PATH.
- **`scaffold` stays inside the repo.** `path`, `layer`, `container` and
  `name` with `..`, an absolute path or a drive prefix are refused
  before anything is written or registered.
- **`--changed` sees every changed file.** Paths git quotes (`"`, `\`,
  control characters) are decoded, non-ASCII paths are read verbatim,
  and a harna root in a monorepo subdirectory gets paths relative to
  itself (they used to be relative to the work-tree top and were all
  skipped).
- **Scopes match whole segments.** New `schema::scope` component: a
  rule or container scoped to `src/api` no longer covers `src/api_v2/`,
  and `./src/api/`, `src\api` and `src/api` are one scope.
- **Option-shaped refs never reach git.** A baseline like
  `--output=<file>` is refused before `git show` runs.
- **`scan` drops orphaned shards** when a container is renamed or
  removed, so stale `shards/*.json` no longer linger in git.
- The `place` tests no longer share a temp dir (the parallel-load
  flake noted at 0.3.32).

### Changed (review tier 3 — one surface, honest checks)

- **The CLI's gates and registers are MCP tools now** (31 → 41 tools):
  `harna_status`, `harna_preflight`, `harna_validate_intent`,
  `harna_acknowledge`, `harna_decompose`, and the action-dispatched
  `harna_non_goal`, `harna_lesson`, `harna_roadmap`, `harna_terms` —
  the AGENTS.md ritual no longer needs a shell. `harna_adopt` is the
  correctly spelled twin of `harna adopt`; `harna_adapt` stays as an
  alias. `harna_place` keeps returning the full plan briefing (the
  0.3.0 decision).
- **One verdict everywhere.** `harna_validate` applies the advisory
  ledger like the CLI, takes `strict`, and every validate report ends
  with `verdict: PASS` or `verdict: BLOCKED` (shared
  `acknowledge::gate`).
- **Rule origins are ISO dates on every surface** (MCP wrote
  `unix-<days>`).
- **Pattern rules respect `scope`**, and an un-compilable pattern is a
  `harna-data` Error instead of an Info borrowed from
  `manifest-depends` (which a binding could re-grade away).
- **Rename linkage stops crying wolf.** `mod.rs`/`lib.rs`/`main.rs`
  match only with their parent folder (no more every-`mod.rs`
  candidate lists), compound children are linked too, and a move
  candidate replaces the `manifest-path` finding instead of doubling it.
- **`metrics` errors on a broken manifest** instead of reporting every
  module unhomed at 0% pressure.
- Removed the stray `.commandcode/` agent-tool folder from the repo.

## 0.3.32 - 2026-10-18

### Fixed (dogfood batch 1 of 3 — findings 1, 2, 5, 9, 11 + own catch)

- **`validate --changed` sees untracked files.** `git status` now runs
  with `--untracked-files=all`: a brand-new `src/` used to collapse to
  one `?? src/` line that the parser (correctly) dropped, so unstaged
  births went blind while staged adds were always seen. One shared
  porcelain reader feeds both change selection and `born-size`.
- **First-change baseline.** `diff`/`parity open` against an unborn
  HEAD no longer errors: the empty tree is the honest baseline, so the
  very first structural change has a clean story. Genuinely bad refs
  still fail loudly (the empty tree is for unborn HEAD only).
- **Re-resolve warns.** `harna parity resolve` (CLI + MCP) now names
  the previous verdict before overwriting a settled target — audit
  trail loss goes noticed.
- **Preflight agrees with validate on the ledger.** The advisory
  partition composes into the release gate; counts read
  `E error, I info, A acknowledged` everywhere.
- **Granularity names its number.** The advisory now fires "past 600",
  not just "has grown".
- **Doctor's `agent-wiring` is satisfiable.** The shipped `harna-mcp`
  binary on PATH counts alongside a config mention; the hint names
  both halves.

### Decisions (logged, not coded)

- Finding 8: `no-code-execution` stands — test-running checks stay
  out; semantic invariants remain the suite's jurisdiction.
- Finding 6: `deny-sync` stays info — small-repo noise accepted.
- Batch plan: 0.3.33 = scan test-registration + scaffold mod-wiring;
  0.3.34 = numeric size budgets + parity amendment history + adopt
  refresh.

## 0.3.31 - 2026-10-17

### Added

- **Rename linkage (`rename-linkage` check, registry #30).** The
  manifest's paths track moves, not just existence: a declared
  component path missing on disk with the same file name alive
  elsewhere is a stale linkage — error with the candidate, never a
  rewrite (the move may be deliberate). The plain-missing case stays
  silent here; `manifest-path` already owns it, and two checks never
  nag the same file twice.
- **Advisory ledger lock (`.harna/acknowledged.toml`).** Accepted Info
  findings (check + file) locked away with a reason: new schema
  component `acknowledged-data`, pure partition in
  `enforcement/validate/acknowledge.rs` (errors are never lockable —
  even a covered error stays live), `harna acknowledge add/remove/list`,
  doctor audit. `harna validate` renders locked advisories as one-line
  `ack` receipts and excludes them from strict-mode gating; JSON gains
  an `acknowledged` array with counts.
- **`harna decompose`: the split planner.** New composition component:
  top-level item spans (comment/string-aware, approximate and stated)
  with dispatch-split (>50% single item → the `*Cmd` enums are the split
  units) vs prefix-cluster recommendations, plus the manifest sketch —
  because an unregistered split is a `manifest-path` violation waiting
  to happen. Application stays manual; the cut lines are visible, the
  judgment is yours. harna applied it to itself this burst (below).
- **Self-application: `harna-cli` grows a `commands/` container.**
  The eight `*Cmd` engines (parity, templates, notes, terms, non-goal,
  lesson, roadmap, deferred) move enum + arms into their own modules
  behind one-line dispatches; `main.rs` keeps the `Command` enum and
  the one-liner arms. The planner's dispatch-split recommendation,
  executed verbatim.

## 0.3.30 - 2026-10-17

### Added

- **`harna validate-intent`: truthfulness reconciliation between claims
  and state.** New composition component `intent` (briefing container):
  the four mechanically verifiable claims — package version vs
  CHANGELOG head, package version vs version-shaped roadmap NOW,
  version-shaped DONE milestones vs release tags, DONE milestones vs
  parity silence — cross-checked, each rendered HOLD/DRIFT/SKIP with
  both sides named. Non-version milestone ids are destinations, never
  forced into releases; unverifiable checks SKIP out loud (outside git,
  tags cannot be proven). DRIFTs exit 2 (the gate precedent); mid-burst
  drift is normal, claiming coherence with drift is not.

## 0.3.29 - 2026-10-17

### Added

- **`harna preflight`: the release gate as one command.** New
  composition component `preflight` (briefing container): validation
  errors, doctor failures, open parity debts and map freshness compose
  into a single PASS/FAIL report; FAIL exits 2 (the gate precedent
  `validate` set), so hooks and CI can gate on it. The fifth section —
  the test suite — is the honest boundary: harna never executes project
  code (non-goal `no-code-execution`), so preflight names the suite as
  NOT-RUN and tells the caller to run it, rather than pretending it
  passed. Warns never gate; errors, failing doctor checks, open parity
  targets and a missing/stale-extractor digest do.

## 0.3.28 - 2026-10-17

### Added

- **Roadmap as declared data (`.harna/roadmap.toml`)**: milestones with
  stable ids, a title, and one of four states (declared | current | done
  | dropped). Declared by the human and the agent — never generated by
  the tool — but load-bearing: new schema component `roadmap-data`
  (upsert by id, case-insensitive `find`, `reached()` = state done,
  coherence over duplicate ids / unknown states / multiple `current`).
  Onboarding opens with "roadmap: NOW `<id>` — `<title>`"、 status
  carries the same line, so a fresh session knows today inside the arc,
  not just what exists. CLI grows `harna roadmap add/set/remove/list`.
- **Deferred reviews can key to a milestone**: a review spec with
  neither date nor version syntax is now a MILESTONE id — due exactly
  when the roadmap declares that milestone done (via `overdue(...)`
  composing `roadmap.reached`). status' overdue count and score's debt
  costing grade through it; the CLI/MCP deferred lists mark milestone
  specs explicitly and quote the supersession path on add. Coherence:
  new validate check `roadmap` (registry #29, error-severity) polices
  the roadmap's internal soundness AND the pointer class — a deferred
  entry naming an undeclared milestone is a ghost error; a done
  milestone's still-open keyed reviews surface as due (Info).
- Doctor audits the declared roadmap: unparseable FAILs, incoherent
  (duplicate ids / unknown state / multiple current) WARNs, "now is
  `<id>`" when named. MCP resource `harna://roadmap.toml` (read-only).
  harna declares its own 0.3.26–0.3.31 train in the file it now
  polices — the roadmap dogfoods itself.

## 0.3.27 - 2026-10-17

### Added

- **Decline as data (`.harna/non-goals.toml`)**: deliberate product
  refusals (the register states what is refused and why) — distinct
  from rules.toml, which polices code conventions; this polices
  *proposals*. New schema component `non-goals-data` (upsert/remove by
  id, token matching with an exactness guarantee). `place`/`plan`
  render a "refused (non-goals in force — do not propose)" block for
  any overlap, and `plan`
  SUPPRESSES outright when an active refusal covers the idea's whole
  token set: no placement, no candidates, just the refusal, the reason
  and the deliberate supersession path (user agrees -> `harna non-goal
  remove` -> decision logged). A partial token overlap never suppresses
  — sibling work stays plannable. New schema component; CLI grows
  `harna non-goal add/remove/list`; doctor audits the register
  (unparseable FAILs, hollow refusal text WARNs).
- **Dead ends as data (`.harna/lessons.toml`)**: tried-and-failed
  records — claim, verdict, evidence pointers (decisions, commits) —
  so failure is owned ONCE and never re-suggested by a fresh session.
  New schema component `lessons-data` (hay matching, ranked overlap,
  capped at 3). `place`/`plan` mine them into a "prior attempts
  (recorded dead ends)" block with evidence, strongest overlap first.
  CLI grows `harna lesson add/remove/list` (commands demand reasons:
  removal without a superseding verdict refused to users by its own
  hint); doctor audits the register (unparseable FAILs, verdict-less
  lessons WARN). harna seeds its own register with real incidents
  (the 0.3.25 Windows-encoding corruption is now a queryable lesson,
  not a paragraph in a handoff).
- MCP resources `harna://non-goals.toml` and `harna://lessons.toml`
  (read-only), alongside the 0.3.26 glossary resource.

## 0.3.26 - 2026-10-17

### Added

- **Glossary as data (`.harna/terms.toml`)**: agent vocabulary ->
  harna vocabulary. A term carries harna's `word`, the agent
  `aliases` it absorbs, and `means` — the meaning as harna defines it.
  New schema component `terms-data` (data structs + collision logic);
  the store persists it (a missing file is the adopted empty state);
  `onboarding` renders it compressed at session start; `harna doctor`
  audits it (unparseable FAILs, alias collisions WARN — one spelling
  may not mean two things); the MCP surface exposes it read-only
  (`harna://terms.toml`); the CLI grows `harna terms add/remove/list`.
  Data-first: agents and humans edit the file, never a generated view.
- **Autonomous ritual emission**: the stock AGENTS.md gains the
  born-modular contract, the vocabulary note, and the autonomous-ritual
  paragraph (plan -> scaffold -> implement -> scan -> parity open ->
  resolve -> validate --changed -> handoff -> atomic commit) — an
  auto-mode loop whose every step is an audit point a human can replay.
- **Born-modular enforcement (`born-size`)**: a rule may now bind
  `check = "born-size"` with a `limit` — of the `.rs` files BORN in the
  working tree (`git status` adds/untracked; renames are history, not
  births), any arriving over the strictest applicable limit is an ERROR:
  split per-scope before the burst's commit. No grandfathering is
  needed because births are exactly what's gated — files with history
  stay with the advisory `component-granularity`/`hotspot` checks. The
  `place`/`plan` briefings carry the slice-before-write guidance so the
  check is the backstop, not the teacher. harna holds itself to it
  (rule `born-modular`, limit 600).

### Changed

- Check registry 27 -> 28 (`born-size`, error default); `place` and
  `plan` both gain the born-modular line (tests updated); parity.rs test
  dead-assignment warning fixed with a real assertion.

## 0.3.25 - 2026-10-17

### Added

- **Layers as data (Phase F capstone)**: layers declare `role` — the
  vocabulary lives in `schema/manifest.rs` as data (`ROLES`): base (owns
  the primary facility + the data everything reads through), engine
  (sync computation; gateways deliberate), gateway (owns facilities for
  the other layers), orchestration-sync / orchestration-async (drives
  engines; async flows in the -async form), transit (thin corridors),
  interface (faces humans/agents; carries the async runtime boundary),
  context (composes agent briefings). Containers inherit the layer's
  role unless they declare their own (`effective_role`); adoption stays
  data-first — an undeclared role owes nothing. harna's own six layers
  are stamped with their real roles.
- **Async inventory** (map extractor v3): `async fn` declarations are
  their own inventory kind (`async-fn`, pub and priv captured) —
  extracted once by `scan`, read by the role gates. Old maps rebuild on
  the next `scan` without ceremony.
- **`role-async` check (error)**: a container whose effective role
  cannot carry async but whose modules declare `async fn`s is one
  finding per container — async lives only in `orchestration-async`
  and `interface` roles; engines, bases, gateways, transiters and sync
  orchestration stay sync.
- **`rules-coverage` check (info)**: a declared container no live rule
  watches is an advisory island, named per container with its layer
  (deprecated or env-scoped rules stop covering). Check registry 25 → 27.
- **Role scaffolds**: `harna scaffold --role <ROLE>` (CLI flag + MCP
  `harna_scaffold` param) validates against the vocabulary and stamps
  the container; transiter scaffolds stamp `role = "transit"` on their
  corridor container by construction.

### Real interplay with everything shipped so far

- **Gateway ownership narration is role-aware** (collects the deferred
  register entry `gateway-ownership-narration`, review 0.3.25, now collected
  and removed from the register): the multi-owner Info reads as "whose, in
  what role" — e.g. `schema (memory, role base)` — and the hint names the
  legitimate shape: a split is deliberate when the owners' roles differ,
  and the `role` next to the `gateway` declaration says it.
- The check registry, rule bindings, parity fan-out, score/status/doctor
  surfaces and the manifest all inter-operate unchanged: the two new
  checks ride the same `Violation` shape, bind through rules.toml like
  every other check id, and their findings flow into `validate`,
  `parity` templates and the score parts without new plumbing.

### Honest notes (not shipped, deliberately)

- `role-async` gates at error by default — truthful for this repo
  (harna-core is fully sync; the only `async fn`s live in the MCP
  interface layer, the allowed role). Mapped modules outside every
  declared container are not gated (the map's adoption carve-out).
- Layer/container roles are declared data, not inferred: harna states
  what your architecture claims, and enforces only what is claimed.

## 0.3.24 - 2026-10-17

### Added

- **`migrate --dry`** (CLI flag; MCP `harna_migrate(dry=true)`): the
  preview without writes — a file that would move reports `WOULD migrate
  schema vN -> vM`, naming the backup that would be kept **and** the
  decision entry that would be recorded, then `nothing written`. A dry
  run writes nothing and records nothing — pinned by test.
- **`harna restore <name>`** (+ MCP `harna_restore`, tool 31): the
  newest `.harna/backup/<name>.vN` copy rolls back over the live file.
  Plain data-file names only — `..` and separators are refused before
  any path lookup, no live file guessed at.
- **Backup/restore verification**: every backup is parse-verified
  (readable, non-empty, TOML) **before** the transform ever touches the
  live file — a backup that cannot be read back is not a backup — and
  restore refuses an unreadable backup outright, leaving the live file
  untouched. The swap itself stages to a temp sibling and renames (a
  half-copied restore never becomes the truth).

### Edge cases (pinned by tests)

- Dry → real → restore round trip: a v0-stamped manifest migrates with
  its backup kept and decision recorded; restore returns it to v0 with
  the backup verified first. Traversal — `no/such/../file.toml`, missing
  backups — are refused, never guessed.
- A backup corrupted after migration (`not [ valid toml`, then blank)
  is refused on restore and the live file survives — refusing a
  half-written shelf is the whole point of restore.

### Honest notes (not shipped, deliberately)

- Path normalization: `Store::open` now lexically resolves the root
  (`canonicalize` when it exists, dot-cleanup otherwise) and applies the
  Windows verbatim `\\?\` prefix only past 240 characters — deep
  application paths stay within classic MAX_PATH reach without changing
  how everyday paths display (canonicalize's own verbatim form is
  reduced back to the plain form first). Relative `..` heads survive the
  fallback when nothing exists to resolve against — dropping them would
  silently change the path's meaning.
- No automatic restore-on-*anything*: restore is an explicit command
  reading the shelf you already have; git remains the primary undo
  surface (data is tracked, the diff is the review).

## 0.3.23 - 2026-10-17

### Added

- **The debt register**: deliberate deferrals become tracked data in
  `.harna/deferred.toml` — what collecting needs (component), why it was
  deferred (reason), when it must be revisited (review: an ISO date or a
  version the tool must reach), and when it was recorded. Distinct from
  parity debts *by construction*: parity is unresolved change
  propagation, the register is a promised reconsideration.
  `harna deferred add` (same id replaces) / `remove` / `list` + MCP
  `harna_deferred_add` / `harna_deferred_list` (tools 29–30).
- **Status surfaces the register**: the health line gains
  `· deferred register: N open` (and `N open, M OVERDUE` once any
  review date arrived — the loud form).
- **Score counts overdue reviews as debts**: each deferred entry past
  its review trigger costs one `DEBT_COST` from the debts part, the same
  weight class as an unresolved parity target — a missed review date is
  lapsed follow-through. Unparseable review specs are never overdue (an
  unpinnable date cannot demand a review); the entry still counts in
  status. Old score snapshots parse back with the new field defaulted.

### Edge cases (pinned by tests)

- Review arithmetic: ISO dates sort lexicographically and the trigger day
  itself is *due* (owed now); versions are due once the shipped tool
  reaches them with zero-padding (`0.4` due at `0.4.9`).
- Register upsert replaces by id (never duplicates); a later review spec
  replaces the overdue one — re-recording a promise un-flags it.
- Status renders the register only when entries exist; removing the
  register file returns the health line to parity-only.

### Honest notes (not shipped, deliberately)

- **First three entries are the register's own dogfood** — the standing
  "no ask yet / Info-only noise" prose notes from prior decision logs
  turned into data: `fixture-builder-dedup` (review 0.4.0),
  `archived-decisions-viewer` (0.4.0), `gateway-ownership-narration`
  (0.3.25 — fold a formal gateway-ownership story into the layers-as-data
  capstone). A deferral collected is *removed*, never kept: history is
  the decision log's, not this ledger's.
- The debt register does not validate `component` against the manifest —
  the pointer is free-form on purpose; scored overdue entries are the
  enforcement (Info weight, never an error).
- Score honesty, recorded on the ledger rather than tuned away: mid-burst
  the digest-stale read was 64 with the ▼ flag; the fresh scan settled
  the composite back to 67 (= last) — but the shifts **inside** the parts
  are real and named: coverage ticked up with the register tests
  (47/52), `schema/manifest.rs` crossed the coupling-outlier threshold
  as the digest grew (6 outliers — the project's true hub now visible
  under the mean+2σ statistic), and boundary pressure sits at 75% with
  briefing at 98%. Truer measurements, not worse code — the 0.3.19
  class, not drift to tune away.

## 0.3.22 - 2026-10-17

### Added

- **Parity templates**: a recurring change type's fan-out shape becomes
  declared data in `.harna/templates.toml` — a scope (module-path prefix,
  empty = everywhere), a ceiling (`max_targets`), and the rationale of why
  the shape is what it is. `harna templates add` (same id replaces) /
  `remove` / `list` + MCP `harna_template_add` / `harna_template_list`.
  The new **`fanout-shape` check** (registry, Info by default — a bound
  rule may promote it) prints an orientation when an open parity event
  carries *more targets inside the template's scope than the declared
  ceiling*: either the change type exploded (ask why) or the ceiling is
  stale (replace the template). Templates are data, not verdicts; no
  templates declared = nothing to notice.
- **`harna notes review`** (+ MCP `harna_notes`): the review checklist —
  from a parity event's fan-out (`id`) or the diff from a ref (`--from`),
  the scoped rules whose scope covers the changed paths (with their
  rationale), and the registered tests over the changed components,
  mapped to runnable cargo invocations through the coverage map. The
  three sources every review needed open, on one page.
- **`harna notes release --from <ref>`**: the release/PR note draft —
  decisions recorded since the ref (live log and the filed archive both
  read) and parity events opened since the ref, each with its shape.
  "Since the ref" is computed against the *committed* ledgers via git
  ls-tree (the briefing container owns the process gateway) — uncommitted
  work is included, ref-committed history is not.

### Edge cases (pinned by tests)

- The shaped fan is the module count **inside** the template's scope —
  a template declares one change type's shape, not the whole event's
  reach. A sibling directory sharing the scope's prefix (`schema2/` under
  `schema/`) is not inside it.
- Template add/remove are round-tripped through the store; unreadable
  templates documents invent no findings (advisory check, not error).
- Release draft without git history says so instead of pretending;
  committed decisions/events never re-appear as "fresh".

### Honest notes (not shipped, deliberately)

- No validation-severity jump for fanout-shape without somebody binding a
  rule — a declared shape you don't watch is your choice, but it is yours.
- Notes are **stdout-only drafts** (the export doctrine): to read, prune
  and speak — never written into the repo, no committed format.
- No template removal in MCP (edit `templates.toml` — the data file is
  the interface); no template history.

## 0.3.21 - 2026-10-17

### Added

- **Profiles: `init --from <path>`**: another repo's `rules.toml` +
  `manifest.toml` become a starting point for a fresh `.harna/`. The
  source may be the repo root (reads its `.harna/`) or a `.harna/`
  directory itself. **Copied verbatim** — a profile is a starting point
  to prune, not a template to re-render. A piece the source does not
  have leaves the starter in place; **a source piece that fails to
  parse is refused before anything is overwritten** (a broken profile
  must not poison `.harna/`). The init report names both interplays:
  the copied manifest's paths point at the *other* tree (prune and
  re-point them — edit the file or `harna scaffold`), and copied rules'
  scopes that miss this tree simply apply nowhere.

### Edge cases (pinned by tests)

- Verbatim copy verified through the loaders (the copied rule and layer
  load back with the source's ids — not a re-render).
- Nothing under the source → clear refusal; garbage manifest → profile
  refused and the starter rules stay intact.
- After a profiled init, `harna adopt` correctly refuses the non-empty
  manifest — the skeleton is explicitly full; `harna scaffold` and
  direct edits are the tools from there.

### Honest notes (not shipped, deliberately)

- No MCP tool for profiles: agents arrive at already-initialized
  repositories (init is a bootstrap, human-owned step). If agents ever
  need to bootstrap, that is a separate ask.
- Decisions, scores, parity and digest are **never** copied — a profile
  carries the other repo's conventions, not its history.
- No profile registry/format of its own: a profile is just another
  repo's `.harna/` data — no second representation to keep in sync.

## 0.3.20 — 2026-10-17

### Added

- **`harna adopt`** (+ `harna_adapt` MCP tool): draft a manifest
  proposal from the existing folder tree — one layer per crate (named
  after the layer's package, read from its `Cargo.toml`, root
  crate folded first), cross-crate import direction proposed as
  `depends_on` via `metrics::resolved_edges` (the one shared edge
  resolution), one container per crate `src`, one primitive component
  per module with its line count in the summary (`draft:` marked — a
  marker to replace with prose, not prose pretending to be a
  decision). Prints manifest TOML to **stdout** (summary to stderr —
  redirecting yields paste-ready TOML); **never writes anything**: the
  adopter prunes, renames and only then commits the draft into
  `.harna/manifest.toml`. Refuses on repos that already carry a
  non-empty manifest (a draft is a proposal about an unadopted repo;
  there, `harna scaffold` and manifest edits are the tools).

### Edge cases (pinned by tests)

- Layer order is deterministic (root crate first, then A–Z by crate
  dir; package names ride as data via the crates' own Cargo.toml).
- Component names unique per container: leaf first, full stem on
  collision, numbered fallback — all deterministic.
- The draft round-trips through the manifest loader: a draft that
  fails to load would waste the adopter's time.
- Missing `.harna/` = the fresh start adopt exists for; a malformed
  manifest still errors.

### Honest notes (not shipped, deliberately)

- One container per crate, even for big crates: grouping *inside* a
  crate is intent, not structure — the draft has no basis to invent
  it. Component grain per module with sizes gives the adopter the
  material to group (the 51-module harna-core draft shows every size
  at a glance).
- The draft proposes `depends_on` but no gateways/`provides`/budgets —
  those are decisions a draft has no evidence for, and empty stays
  ungated by design.
- Size counts are a second read over the files (one-time, report
  only) instead of a new `lines` field in the digest — the map's data
  format stays untouched.

## 0.3.19 — 2026-10-17

### Added

- **Graph export** (`harna graph` + `harna_graph` MCP): the dependency
  graph as DOT or Mermaid — map modules as nodes, resolved internal
  imports as edges, and the manifest's layer/container topology as
  nested clusters **in manifest order**. Layer ranks are pinned by a
  per-layer anchor chained with invisible high-weight edges (clusters
  alone only *suggest* vertical order). Unhomed modules cluster apart —
  adoption debt is visible, not hidden in somebody else's box. The CLI
  summary line goes to **stderr**: redirecting stdout yields a clean,
  import-ready file.

### Fixed

- **Cross-crate edges were silently dropping** from every consumer of
  the shared dependency resolution (`resolved_edges` → metrics ledger,
  and now the graph export): dep targets carried the import-site
  spelling (`harna_core::…`) while the crate→dir host lookup keys on
  **package names** (`harna-core`). The resolved target now carries the
  matched workspace package name. Dogfood effect: the metrics ledger
  jumped from a lopsided view to 165 edges — `cli` and `mcp` modules
  finally report their real outbound edges (−23/−20), so boundary
  pressure and coupling are true now. One resolution, fixed at the
  source; every consumer benefits.

### Edge cases (pinned by tests)

- Homes resolve by manifest longest-prefix; the stray lands as unhomed,
  node order is manifest-ordered (unhomed last).
- DOT: layer clusters in manifest order + the invisible anchor chain;
  resolvable cross-crate edge present; unresolvable deps never render.
- Mermaid: positional ids (`m0..`) with the same view and same edges.

### Honest notes (not shipped, deliberately)

- The graph is a **stdout export**, not a committed view: no staleness
  gate (SURFACE/ARCHITECTURE remain the generated-and-committed pair) —
  no `--out` flag either, so the interface layers never gain a
  filesystem writer; redirect stdout instead.
- Edges are the metrics ledger's same resolution — module-level,
  deduplicated, self-imports excluded. Symbol-level edges were not
  built (the map's deps are module-grain; finer grain needs a different
  substrate and had no ask).
- Mermaid cannot pin subgraph rank order the way DOT's anchor chain
  does — its nesting order is the manifest's order, best-effort by the
  renderer.

## 0.3.18 — 2026-10-17

### Added

- **Archive & compaction of state** (`harna archive` + `harna_archive`
  MCP tool): closed state files off into `.harna/archive/<kind>/` — the
  live surface stays lean forever. Nothing is deleted: a same-volume
  rename, the bytes never change, the git-tracked ledger stays greppable.
  - **Closed parity events** → `archive/parity/`: an event with no open
    targets owes nothing; archived ones leave `parity show`, status and
    the onboarding debt summary. Open events stay live no matter how old
    — an open target is a promise, not history.
  - **Superseded decisions** → `archive/decisions/`: the visibly-dead
    tail files off whole (whoever supersedes a decision is newer than
    it, so a dead decision's own target is dead too — the live log keeps
    exactly the current heads). The `decision-chain` check resolves
    pointers into live ∪ archive: a filed decision is history, **not a
    ghost**; a pointer at a decision that is neither live nor filed is
    still an Error.
  - **Score snapshot retention** (the 0.3.14 deferral, collected):
    `SNAPSHOT_RETENTION = 12` — the newest 12 recordings stay live (a
    year of bursts is the trend window), older ones archive to
    `archive/scores/`. `--keep <N>` overrides; trends keep reading the
    live window.

### Edge cases (pinned by tests)

- Parity: closed event files off, open stays, second pass is a no-op.
- Decisions: the dead tail moves whole; the live head's pointer resolves
  through the archive (no ghost); a truly missing target still errors.
- Retention: 14 recordings, keep 10 → the 4 oldest archive in order,
  the newest window stays live, and within retention nothing moves.

### Honest notes (not shipped, deliberately)

- "Compress" is of the **live surface**, not the bytes: archive files
  stay plain TOML/markdown — gzip would save kilobytes and cost
  greppability, the wrong trade for a ledger whose whole point is
  staying readable (said plainly, per house style).
- Archived decisions leave `harna decisions` and the chain walk (live
  views only); their titles still resolve as supersedes targets. A
  dedicated `harna decisions --archived` viewer was not built — no ask
  yet, git history serves.
- The archive directory gets no doctor check of its own: it is a plain
  subdirectory of git-tracked files, and doctor checks data health, not
  filing.

## 0.3.17 — 2026-10-17

### Added

- **Fast paths** (Phase D scale concludes; store growth in place, no new
  component):
  - **One scan, many consumers**: `score::full` (the `harna metrics`
    machine shape) reads the committed map **once** and passes it down —
    the whole-map era stacked four reads (metrics streamed it, score
    loaded it, the CLI loaded it again, hotspots joined the third). The
    shared core is `score::build_with` (coverage, parity debts, freshness
    from inputs a caller already holds); the plain `harna metrics` path
    renders the same three sections from the same one read.
  - **Parallel scan**: `build_incremental` fans the per-file work (hash-
    verified reuse or a fresh parse) out over one thread per chunk of the
    file list — `std::thread::scope`, no new dependency; the final path
    sort makes the result identical whichever way the chunks land, and
    small repos stay on the calling thread (spawning costs more than
    parsing a dozen files, floor: 16 files/thread).
  - **`harna diff` reuses the map** (CLI and MCP): the working-tree scan
    was a full rescan of every file; now it is the same incremental
    build `harna scan` runs — hash-verified reuse is content-addressed,
    so the diff is identical, the parsing a fraction.
  - **Incremental validate (body-hash reuse)**: the pattern rules' name
    extraction reuses the map's inventory when a file's live body hash
    equals the recorded one (extractor-gated like the scan's reuse) —
    the inventory catalogs exactly what `declared_names` derives (the
    same filtered parse loop), so a matching hash means the catalog IS
    the parse. Changed or new files parse as before. Validate's map read
    joins the one-scan discipline too (one `load_digest` per run, moved
    above the file loop it now feeds).

### The two-level index checkpoint (deferred from 0.3.15, declined in 0.3.16)

- **Answered: one level is enough — now with evidence.** The checkpoint
  asked whether body-hash reuse, the first consumer built on the index's
  compact records, wants per-shard sub-indexes. It does not: the reuse
  consumer (validate) consumes whole modules' inventories from the one
  map read its other checks already needed, and the scan's reuse reads
  the previous map wholesale for the same reason. The hash lookups ride
  reads that exist anyway; a second index level would index a lookup
  nobody performs standalone. The compact records keep their job as the
  diffability/substrate layer, not a query path.

### Edge cases (pinned by tests)

- Parallel scan over 24 files: digest order holds, module-for-module
  identical, reuse counts exact (24 reused, 0 rescanned on the second
  pass).
- `reusable_declared_names`: no hash → None; foreign hash → None; the
  recorded hash → names equal `declared_names` (duplicates deduped).
- `full()`: metrics/score/hotspots agree with the standalone whole-map
  results (two modules, zero edges, fresh map).

### Honest notes (not shipped, deliberately)

- The parallel scan caps out at one thread per 16 files and needs
  `available_parallelism` — a single-core machine runs the sequential
  path unchanged; no thread pool, no runtime, nothing to tune.
- Validate's body-hash reuse skips one parse pass per unchanged file;
  the body is still read for URL/accretion/copy-paste checks — the
  reuse is the parse, never the read. A violations cache keyed by hash
  would need persisted state and is nobody's ask yet.
- `score::build` (standalone) still reads the map itself; only the
  multi-consumer call paths share the read. A standalone grade is one
  read by definition — nothing to share with.

## 0.3.16 — 2026-10-17

### Added

- **Bounded queries** (Phase D scale continues; store growth in place, no
  new component): `whereis`, `dependents`, `metrics` and onboarding answer
  from the index plus one shard at a time — the parse unit is a container,
  never the map. Memory stays proportional to the answer, not the
  codebase.
  - `Store::for_each_shard`: the bounded module stream — digest order
    across shards, stamps (`generated_at`, extractor version) riding every
    visit, visitor early-stop. The legacy monolith joins the stream as a
    single pass (bilingual, like `load_digest`); a map-less repo reads
    `Ok(false)` and the caller owns the scan refusal.
  - `Store::load_index` / `Store::load_shard`: the index alone (stamps +
    compact path records) and one shard file, for consumers that never
    need modules in full.
- `dependents` resolves its target from the **index path list** (the
  0.3.15 enabler) — no shard parsed for resolution — then streams shards
  twice (ring 1, ring 2), holding edges only. First-match-in-digest-order
  semantics pinned.
- `whereis` streams up to three passes (exact, case rescue, substring
  rescue capped at the render limit), keeping matching declarations only.
- `metrics::build_store` streams shards into the **graph projection**
  (path + deps): signatures and inventory — the byte bulk — never
  materialize; edge resolution keeps the one shared machinery
  (`resolve_dep` + `hosted_module`) via `build`, so the parse unit
  changes, the semantics do not.
- Onboarding renders the overview from counts plus the leading modules:
  totals ride the whole stream, detail caps at ten, and the text is
  byte-identical (`Digest::render_overview` now delegates to the same
  `render_overview_from`).

### The two-level index design decision (flagged in 0.3.15)

- **Decision: one-level index stays.** The index's compact records are
  linear in modules (a path plus a hash each); bounded queries stream
  shards, so the index parse is the only whole-list read, and it is
  orders of magnitude below the map. A two-level index would pay off only
  when the index itself becomes the bottleneck at millions of modules —
  revisit alongside 0.3.17's body-hash reuse, which reads the index per
  changed module and is the first consumer that could want per-shard
  sub-indexes. No mechanism was built for a need that does not exist yet
  (constants-are-data's honest sibling: structure follows evidence).

### Edge cases (pinned by tests)

- Bounded stream: digest order across shards, stamps present, early-stop
  honored, legacy monolith as one pass, `Ok(false)` on no map.
- Dependents over a legacy monolith finds the same edges (bilingual
  bounded reads).

### Honest notes (not shipped, deliberately)

- `metrics::build_store` still ends with the graph resident (paths +
  deps): fan/pressure measurement is *of* the import graph, so the
  resident set is the subject itself. The bounded win is the parse unit
  and the dropped signature/inventory bulk, not an O(1) footprint — an
  honest bounded, not a marketing one.
- `whereis`'s rescue passes re-stream shards (up to three passes) instead
  of holding the world: IO spent in place of memory, the right trade at
  the scale Phase D targets.
- The other `load_digest` consumers (status, place, plan, score, hotspots,
  diff/validate) stay whole-map on purpose — their slot is 0.3.17's
  one-scan-many-consumers fast path.

## 0.3.15 — 2026-10-16

### Added

- **Digest sharding + compaction** (store growth in place, no new
  component): the map persists as `.harna/digest/index.json` (stamps +
  compact per-module records: path + body hash) plus one shard per
  container under `.harna/digest/shards/` holding its modules in full.
  Digest order is preserved inside each shard and across the index — a
  sharded round-trip is module-for-module identical to the monolith
  (pinned). Modules outside every container ride the `_unhomed` shard.
- **Bilingual persistence**: loading reads shards first, legacy
  `digest.json` second (pre-0.3.15 repos work until their next scan
  migrates them); saving writes shards and removes the monolith (one
  truth, never two); `digest_at_ref` probes the ref for the index and
  falls back to the monolith — history stays readable both ways
  (verified against real history: `harna diff HEAD` read the committed
  monolith while the tree already ran shards).
- `module_homes` / `module_layers` / `ModuleHome` moved to
  `schema::manifest` — they join two memory types, so the store can
  shard, search can measure, and checks can gate, all sharing the one
  resolution with zero caller changes (re-export chain intact).
- `save_digest` now takes the manifest (modules need homes to shard):
  7 call sites updated (2 prod scan paths, 5 tests).

### Deferred items reviewed (this task's harvest)

- **None pending for this task**: fixture-builder dedup (0.3.10),
  retention/pruning (0.3.18), registries (0.3.25) all remain
  deliberately out of scope; no digest-specific item was ever
  deferred. The neighboring weights evidence (0.3.12 note) keeps
  accruing snapshots instead.

### Edge cases (pinned by tests)

- Nested containers shard by longest prefix; empty digest writes an
  empty index; slug collisions suffix `_N` (scores-rule reuse).
- Sharded `digest_at_ref` over a real temp git repo: legacy commit
  reads at its ref, sharded commit assembles order-kept at its ref,
  and the old ref still reads the old layout.
- Legacy monolith loads until the next save migrates it forward (then
  the file is gone).
- Stability: 185/185 (new sharding fixtures tag `ThreadId` and clean
  up front, per the standing lesson).

### Honest notes (not shipped, deliberately)

- Per-module bytes are **unchanged by design**: wire stability beats
  micro-trimming (`skip_serializing_if` already omits empties; the
  structs are the tool↔repo protocol). The 0.3.15 win is structural —
  parse units shrink from the whole map to one container — and the
  index's compact records are the substrate 0.3.16 bounded reads and
  0.3.17 body-hash reuse build on. "Smaller per-module serialization"
  in the roadmap means the index row, not the module: said plainly.
- Old tool versions cannot read new repos (forward migration). The
  reverse (new tools read old repos/refs) is covered both live and at
  git refs.
- The index duplicates paths + hashes (location data alongside shard
  data) — the price of locating a module's shard without parsing them
  all; at millions of lines a two-level index may be needed, and that
  is 0.3.16's design decision, flagged here first.
- Remaining dogfood advisories (Info, 27 total, unchanged in kind):
  the known read-guidance cluster.
- No new MCP tool, no new CLI command, no new check, no manifest
  component: persistence reshaped underneath an unchanged surface.

### Data

- New layout `.harna/digest/index.json` + `.harna/digest/shards/*.json`
  (additive capability, `schema_version` stays 1 — the loader's
  bilingualism IS the migration, no `migrate` step needed).
- Decision recorded: `2026-10-16-digest-sharded-per-container--homes-owned-by-memory--bilingual-history.md`.

## 0.3.14 — 2026-10-16

### Added

- **Hotspots: git churn × digest size** (new `hotspots` component in
  code-map + new `validate-hotspot` check component; registry 23 → 24).
  Churn is commits touching the path, size is signatures + inventory
  (no file reads); score = churn × items; modules past
  `max(50, ceil(mean + 2σ))` flag (the 0.3.12 outlier discipline,
  applied to accretion — the floor rationale: 5 commits × 10 items
  earns attention). No git history, or an untracked file, scores 0:
  without evidence nothing flags, never a guess.
- **The accretion advisory is data-driven**: the new `hotspot` check
  (Info) names `N commits × M items (score S, threshold T)` per
  flagged module. The 600-line `component-granularity` guidance stays
  byte-identical as the cheap static signal — the hotspot names which
  big files actually accrete pain. `mod.rs` orchestrators stay eligible
  by design (evidence over exemption). Scoring is pure over
  (churn-map, digest), so the ranking pins without a repository; one
  real-repo test (12 calm files + 1 furnace over 10 commits) pins the
  git half end to end.
- **Trend part-attribution** (the 0.3.13 deferral, collected): trend
  rows name which sub-scores carried each move (`coverage -15, debts
  -8`), nonzero parts only, fixed order. Snapshots always stored parts,
  so attribution works retroactively over every recording.
- `harna metrics` prints the hotspot table (top 10 + threshold);
  `--json` gains the report beside measures and grade.
- **harna dogfoods 2 hotspot advisories**: `tools.rs` (20 × 51 =
  1020) and `digest.rs` (13 × 56 = 728) past the 473 threshold. Note
  what evidence beats: `main.rs` is the biggest file (918 lines, 28
  commits) but carries only 8 items (score 224) — size heuristics
  would nag it first; churn × items correctly does not.

### Fixed — dogfood caught a real layering slip

- The first 0.3.14 cut put `check()` in `search/hotspots.rs` and
  `layer-boundary` fired 4 Errors on the search→integrity import
  (`Violation`, `checks::spec`). Measurement stayed in search,
  enforcement moved to `validate/hotspot.rs` — the layering the tool
  enforces applies to the tool first. Same lesson as 0.3.12's
  `module_homes` move, learned again at the next boundary.

### Deferred items reviewed (this task's harvest)

- **Collected**: trend part-attribution (0.3.13's explicit deferral).
- **Still deferred, deliberately**: fixture-builder dedup across
  validate test mods (0.3.10 future work — a multi-file refactor for
  Info-only advisories and zero behavior change; the advisories stay
  as read guidance); test-function granularity (0.3.11 bound stands);
  snapshot retention/pruning (0.3.18); registry constructs (0.3.25
  territory).

### Edge cases (pinned by tests)

- Duplicate deps are one edge each (inherited); self-imports and
  unhosted deps never score; empty digest scores nothing at the floor.
- Threshold pins: empty → floor, quiet repo → floor, 12-calm + furnace
  → statistics between (names the furnace only).
- `check()` with no digest is silent; collisions on snapshot stems
  still append (0.3.13 `_N` rule, reused by delegation).
- Stability: 181/181 (new git tests tag `ThreadId` and clean up front
  per the 0.3.11 lesson; 10 commits spin up in a fraction of a second).

### Honest notes (not shipped, deliberately)

- No new MCP tool, no new CLI command: `harna metrics` grows the
  hotspot table, `harna validate` grows the check (same bounds as
  0.3.11–0.3.13).
- The `HOTSPOT_FLOOR` (50) is judgment, like the grade weights — a
  named constant with rationale, one edit from disagreement, now with
  two releases of snapshot evidence behind the neighboring weights.
- Remaining dogfood advisories (Info, 27 total, +2 hotspots): the
  known read-guidance cluster; the two new hotspot advisories are the
  feature demonstrating itself, not debt.
- The mid-burst grade wobbled 70 → 69 → 70 as the map gained a module
  (pressure 73% ↔ 74% at the integer boundary); the trend records the
  releases, not the wobble — snapshot discipline, not noise.

### Data

- Registry 23 → 24 checks (`hotspot`, Info); `schema_version` stays 1.
- Decision recorded: `2026-10-16-hotspots-as-data-driven-accretion--trend-attribution--check-lives-in-validate.md`.

## 0.3.13 — 2026-10-16

### Added

- **Score snapshots, persisted per recording** (score growth + three
  `Store` snapshot methods; no new component): `harna metrics --record`
  writes the current grade as timestamp-slugged TOML under the new
  `.harna/scores/` ledger (git-tracked, committed — the release
  recording is the trend baseline). Same-stamp collisions take `_N`
  (never `-N`: dash sorts before `.toml`'s dot and would put the newer
  recording first — caught by test, fixed before it ever shipped).
- **Trends with flagged regressions**: `harna metrics --trend` prints
  every recording with its move since the previous one; a drop of 5+
  (`REGRESSION_DROP`) carries `▼ REGRESSION`, recoveries never flag.
  The status line carries the drift vs the last recording (`= last`,
  `+N`/`-N vs last`, regression flag) — regressions surface where
  agents look, at session start.
- Snapshot IO lives in `Store` (`save_score_snapshot`,
  `score_snapshot_names`, `read_score_snapshot`); `score.rs` names
  stems, serializes, and assembles trends. A missing scores dir is an
  empty history, not an error; a corrupt file fails loudly (silent rot
  would poison every trend).

### Fixed — dogfood caught a real layering slip

- The first 0.3.13 cut had `score.rs` writing snapshots with `std::fs`
  directly — and `gateway-ownership` fired on it (composition declares
  only `process`). The check worked exactly as designed: instead of
  widening briefing's declaration to `fs:write`, persistence moved into
  the storage gateway. The violation appeared, was read, and is gone —
  the 0-error gate below is earned, not arranged.

### Edge cases (pinned by tests)

- Slugs sanitize timestamps filesystem-safe; empty stamps fall back to
  `score` (still collision-suffixed); recordings append, never replace.
- Trend deltas pin at the threshold (-4 drifts, -5 flags, +6 recovers);
  the first recording has no delta; empty history renders the
  record-first hint.
- Status drift pins both directions (+30 improvement shown plainly,
  -15 vs a better recording flags `▼ REGRESSION`) and the no-history
  case (bare `85 (B)`, 0.3.12 render preserved exactly).
- Stability: 175/175 across three consecutive runs (the 0.3.11
  pid-reuse lesson holds — new temp fixtures tag `ThreadId` and clean
  up front).

### Honest notes (not shipped, deliberately)

- Retention/pruning waits for 0.3.18 (archive & compaction): snapshots
  are ~500 bytes each, so unbounded growth is a known, bounded cost —
  named here, solved there.
- No snapshot diffing of *inputs* (which part moved): the trend shows
  score deltas; part-level attribution is 0.3.13-future work, not
  pretended.
- Remaining dogfood advisories (Info, 25 total, unchanged in kind):
  the known read-guidance cluster (granularity signals incl. the grown
  `score.rs`/`store.rs`, fixture-builder `no-copy-paste` pairs, the
  19-block `main.rs`↔`tools.rs` originals).

### Data

- New ledger `.harna/scores/*.toml` (additive data, `schema_version`
  stays 1 — no migration; scores are computed views, stored for trends).
- Decision recorded: `2026-10-16-score-snapshots-persisted--trends-flag-regressions--snapshot-io-in-store.md`.

## 0.3.12 — 2026-10-16

### Added

- **Metrics over the import graph** (new `metrics` component in the
  code-map container): per-module fan-in/fan-out, per-container
  internal/inbound/outbound ledgers, boundary pressure, coupling
  outliers — all resolved with the one shared host machinery
  (`resolve_dep` + `hosted_module`, deduplicated importer→imported
  pairs, self-imports excluded).
  - Boundary pressure is crossing edges as a share of **homed** edges
    (floored; 0 with none). Unhomed modules contribute their fan counts
    but no pressure signal — and their count surfaces beside the number
    instead of silently diluting it.
  - Outliers flag past `max(8, ceil(mean + 2σ))` of per-module coupling
    (fan-in + fan-out), busiest first. The floor of 8 keeps small repos
    honest: a 6-spoke star never manufactures a villain.
  - New `harna metrics` command (with `--json`, which emits measures +
    grade together): container table sorted by pressure, outliers named,
    unhomed count noted.
- **Composite health grade** (new `score` component in the briefing
  container; `harna status` carries `score: N (letter)`): coverage 30,
  debts 20, freshness 15, boundaries 15, coupling 20 — named constants,
  argued in the open. Freshness buckets at 5m/1h/1d/7d with a missing
  digest scoring 0; each open debt costs 4 (5+ zero the part), each
  outlier costs 3; letters at 90/80/70/60. Assembly is pure over
  store-loaded inputs (`assemble`), so every weight decision pins
  without a filesystem.
- **harna dogfoods 70 (C)**: 88% coverage (26/30), no open debts (20),
  fresh map (15), 73% pressure (4/15), 5 outliers (5/20). The C is
  honest — the interface layer genuinely leans on everything
  (cli/mcp/structure containers at ~100% pressure), and the five hubs
  (`main.rs`, `validate/mod.rs`, `schema/manifest.rs`,
  `schema/store.rs`, `search/digest.rs`) are the map's load-bearing
  walls, named as such.
- `module_homes` / `module_layers` / `ModuleHome` moved down to
  `search::digest` (search may reach memory; a metrics view reaching
  into `validate` would have inverted the layering), re-exported from
  `validate/mod.rs` so no check component changed its import path.
  Layering, transit, gateway-`base`, and metrics now share the home
  resolution by construction — the 0.3.10 consolidation lesson applied
  before the copy, not after.

### Edge cases (pinned by tests)

- Duplicate deps into one module are one edge; self-imports never form
  edges; unhosted deps (imports the map never saw) never form edges.
- Empty digest → 0 edges, 0 pressure, no outliers; score still assembles
  (freshness 0 without a digest).
- Grade letters break exactly at 90/80/70/60; freshness buckets are
  exclusive below (299s → full, 300s → next bucket); debt/outlier costs
  saturate at zero instead of underflowing; coverage floors through the
  assembly (66% → 19/30).
- End-to-end over a temp store (2 components, 1 tested, fresh digest,
  no deps, no debts) scores exactly 85 (B).
- New components registered with their tests in the manifest's explicit
  sequences (`metrics` after `whereis`, `score` before `doctor`) —
  the `manifest-sequence` advisories fired once, then went quiet.

### Honest notes (not shipped, deliberately)

- No new MCP tool: agents read the grade from `harna_status` and the
  breakdown is one CLI call away (same bound as 0.3.11).
- The grade weights are judgment, not measurement — they are constants
  with rationale comments, one edit away from disagreement. 0.3.13
  persists scores per recording, which is when weight debates get
  evidence.
- Remaining dogfood advisories (Info, 25 total): the new files join the
  known read-guidance cluster (`metrics.rs`/`score.rs` granularity +
  fixture-builder `no-copy-paste` pairs with gateway/structure/transit —
  the shared-resolver edge builder they all honestly share).
- `lib.rs` crate roots sit outside every container by design (containers
  map subtrees), so the crate-root module shows as unhomed with no
  pressure signal — home semantics unchanged, shared by all consumers.

### Data

- No manifest schema change (`schema_version` stays 1): metrics and
  scores are computed views, not stored data (0.3.13 persists them).
- Decision recorded: `2026-10-16-metrics-and-composite-health-grade--homes-shared-from-search.md`.

## 0.3.11 — 2026-10-16

### Added

- **Coverage ratio in status** (test-coverage growth, no new component):
  `harna status` now reports `tests: X/Y components (Z%)` plus the
  untested component names (capped at 10 in prose with an "…and N more"
  overflow, full list in `--json`). The percent floors — the line never
  flatters — and an empty map is vacuously 100 (nothing to cover).
  Coverage counts every component including nested children; a component
  counts as covered when its `tests` field is non-empty.
- **Registered tests map to runnable `cargo test`** (new `harna tests`
  command, with `--component <name>` and `--json`). Each registered test
  path maps to the invocation that runs it, structurally (path shape to
  command shape):
  - `crates/<m>/src/…/*.rs` → `cargo test -p <pkg> --lib <mod::path>`
    (inline tests; `mod.rs` collapses to the parent module);
  - crate roots (`main.rs`/`lib.rs`) and `src/bin/*` → `cargo test -p
    <pkg>` (no module filter exists there — the whole package runs);
  - `tests/*.rs` → `cargo test -p <pkg> --test <stem>` and
    `benches/*.rs` → `cargo test -p <pkg> --bench <stem>`;
  - anything else (non-Rust assets, paths outside any workspace crate
    dir, crate dirs with no readable package name, nested `tests/x/*.rs`
    which are no cargo target) stays unmappable with its reason printed —
    never a guessed package.
  - Crate membership resolves through the workspace's own Cargo.tomls by
    reusing the `crate_name_dirs` read (one canonical package read).
- **harna dogfoods 38/43 (88%)**: every registered test maps runnable;
  the 5 untested are the two TOML data components (`checks-data`,
  `statuses-data` — data files hold no tests) and the three binary entry
  points (`cli-main`, `mcp-main`, `mcp-tools`).

### Edge cases (pinned by tests)

- Percent floors (`2/3 → 66`, never 67); empty manifest → `(0, 0, 100)`.
- `mod.rs` collapses to its parent (`src/x/mod.rs` → `--lib x`); `main.rs`,
  `lib.rs`, `src/bin/*` run the whole package.
- Nested integration files (`tests/nested/extra.rs`) are unmappable —
  directories are not cargo targets.
- Unknown component name to `harna tests --component` is an error naming
  the problem, not an empty list.
- Fixture hygiene (dogfood finding, fixed in-burst): the scaffold
  `tmp_dir()` and the status test reused pid-only temp dirs with no
  cleanup, so OS pid reuse across runs resurrected stale manifests
  (scaffold refuses to overwrite) and stale handoffs (status asserts
  none) — 6 tests flaked red after a green run. Both clean up front now
  (status additionally tags `ThreadId`); three consecutive green runs
  pin the stability.
- The mapping is structural, never verification: existence + real test
  code stays `tests-declared`'s business; passing stays the job of
  running the printed command (spot-checked: the mapped gateway filter
  runs its 11 tests).

### Honest notes (not shipped, deliberately)

- No new MCP tool and no new manifest component: `harna tests` is CLI +
  library surface only (in-place growth of `test-coverage`, `status`,
  `cli-main`). Agents on MCP read coverage from `harna_status` and the
  mapping stays one CLI call away.
- Test-function granularity (`--lib mod test_fn`) is not inventoried:
  the module filter is the stable contract; fn-name extraction would
  couple the map to test-body parsing that already belongs to
  `test_region`.
- Remaining dogfood advisories (Info, 22 total, +1 from 0.3.10): the new
  `test_coverage.rs` granularity signal (630 lines — the mapping tables
  and their pins live there now) joins the known read-guidance cluster.

### Data

- No manifest schema change (`schema_version` stays 1): no new fields,
  only new computed views over the existing `tests` declarations.
- Decision recorded: `2026-10-16-coverage-ratio-in-status--registered-tests-map-to-runnable-cargo-test.md`.

## 0.3.10 — 2026-10-16

### Added

- **Boundary facilities are owned by the container that declares them**
  (new `gateway` validate component; registry 20 → 23 checks). A manifest
  container may carry `gateway = "fs"` / `"fs:write"` / `"net"` /
  `"process"` / `"base"` (comma list). Declaring a facility is the
  adoption switch — data, never a hidden default, same discipline as
  `provides` and `role`: while a facility is unowned, nothing is gated;
  once it is owned, modules outside the registered gateway(s) may not
  touch it (`gateway-ownership`, Error default).
  - `fs` with the `:write` qualifier is "storage write-only through its
    gateway": reads (`fs::read`, `fs::metadata`, `File::open`, the
    `std::fs`/`std::io` heads) stay open to everyone; writes (`fs::write`,
    `fs::create_dir*`, `fs::remove*`, `fs::rename`, `fs::copy`,
    `File::create`, `OpenOptions`, `tempfile`, `BufWriter`, `write!`) are
    the gateway's monopoly. A full `fs` gateway owns both halves.
  - `net` gates network/DB-over-wire clients (http + sql clients); no
    read/write halves — the wire matters, not the direction.
  - `process` gates subprocess spawns (`std::process`, `tokio::process`,
    the Windows/Unix `os::process` spawn APIs).
  - `base` is graph-only: an import into a rank-0 layer (from 0.3.9's
    rank derivation) from a module that is neither a base-layer module nor
    inside a registered `base` gateway container is a violation; a
    declared `depends_on` does **not** exempt — the gateway narrows the
    base reach from layer granularity to registered containers, which is
    exactly the "only the base's API is public" claim, nameable in data.
  - Live fs/net/process touches are scanned token-wise from the run's
    source set (the digest's `deps` are internal-only, so import-style
    scanning cannot see std/external uses); `base` reads the committed
    import graph instead (stale-digest honesty, like every graph check).
    Without a digest, `base` degrades silently and the source-token half
    still runs.
  - Token discipline: a token hits only as a real path-word — no `::` glue
    from either side and no identifier continuation, so `my_std::fs`,
    `self::fs`, and `crate::fs::read` never score for `std::fs`. Test
    sections (`#[cfg(test)]`…)` are exempt — fixtures legitimately create
    files and dirs (same carve-out discipline the URL check introduced).
    Unmapped files (outside every container) stay ungoverned: the map does
    not reach them (yet) — adoption by declaration, never by bludgeon.
- **Transiter corridors as manifest data** (new `transit` validate
  component). A container declares the layers it bridges with
  `connects = ["api", "billing"]`; scaffolding a transiter
  (`harna scaffold transiter`) stamps `connects` on the fresh corridor
  container (a hand-written `connects` is never clobbered — pinned).
  - `no-sideways` (Error default): a DIRECT cross-layer import between
    corridor-declared layers bypasses the transiter — the corridor exists
    exactly so the coupling shapes itself through a thin pass-through, so
    an edge skipping it calls two architectures true. Imports landing
    inside the corridor, the corridor's own outgoing imports, and pairs no
    corridor names stay silent: no corridor, no gate.
  - `transiter-thinness` (Info default, **rule-gated**: the budget is the
    `limit` on a rule bound to the check; no rule, no gate — the 0.3.7
    budget discipline). Thinness = declared items + signatures per module;
    the strictest applicable limit wins.
- **harna's own map adopts the construct** (dogfooded for real): schema,
  generation=`structure` and conformance containers declare
  `gateway = "fs:write"` / `"fs:write, process"`; code-map, briefing and
  cli declare `gateway = "process"` (git subprocess calls; the CLI owns
  its process lifecycle). The checker itself then narrates the deliberate
  multi-owner split as Info ("facility `fs` has 3 registered gateways…")
  — single-owner spirit stays advisory when the split is named. The
  transit half ships **without adoption**: harna has no corridor by
  design — its cross-layer imports ARE the layered architecture it
  verifies; no fake corridor was constructed for the dogfood.
- **`harna validate --changed` and full runs now exercise both new
  components** (`run_on` wires `transit::check`, `gateway::check`,
  `gateway::check_declarations`); declaration coherence advisories run
  digest-less (corridor typos and unknown gateway words stay visible even
  in cheap runs).

### Fixed — the module graph was silently order-blind

- The registry↔consumer chain (`checks.rs → validate::Severity` while
  every validate child calls the registry) is a REAL Rust module ring that
  the old copy-pasted dep resolver never surfaced — its tie-break compared
  the host *stem* length against the best-so-far **full path** length, so
  the first filesystem-ordered host always won and ring edges terminated
  at module `enforcement/mod.rs` (a dead end). The 0.3.9 dogfood "clean"
  verdict was an artifact of that blindness.
- Consolidation into `search::digest::{crate_name_dirs, hosted_module}`
  (one copy of crate-name and host resolution — layering/budgets/structure
  can no longer disagree) surfaced all cycles: `Violation` + `Severity`
  moved to their own home `enforcement/violation.rs` (registry and
  checker may not ring each other; validate re-exports so every check
  component keeps using `super::{Severity, Violation}`), and
  `decisions::list` now takes the decisions **directory** instead of the
  whole store (the record format is the decision log's own; persistence
  calls it, never the reverse). Both rings are broken; `no-cycles` is
  green on the live map.
- `layering::rank_layers` and the module→home mapping moved to shared
  `pub(crate)` helpers in `validate/mod.rs` (`module_homes`, `module_layers`)
  — gateway's `base` half, transit's corridor logic and strict layering
  resolve homes identically by construction.
- Test fixtures no longer race: temp-dir tags now carry the unique
  `ThreadId` (thread-name fallbacks collided across parallel tests in
  different components).

### Edge cases (pinned by tests / dogfood findings)

- **Facility vocabulary is tool-side data, with sharpened rounding:**
  `std::io::Write`/`std::io::Seek` were dropped as write tokens — they
  also score terminal flushes (`stdout().flush()` needs the `Write` trait
  in scope) which are not storage; `BufWriter` + the fs-anchored words +
  `write!` carry the write side (pinned: the flush stays open, `write!`
  scores).
- The brace form `use std::fs::{write, …}` hides which symbols the brace
  opens — the line counts as its READ side (`std::fs` head fires only
  under a full `fs` gateway, never under `:write`). Documented limitation,
  pinned by test (same discipline as 0.3.3).
- Malformed `gateway` declarations (`"fs:writing, banana"`) never engage
  enforcement; each unrecognized part is named as a per-container Info
  advisory. A partially valid declaration engages its valid half
  (`"fs, banana"` → fs owned, net stays ungoverned) and still names the
  junk.
- Two containers claiming the same facility: both remain substantially
  exempt (subtree rule), the doubling is narrated once as Info with both
  paths named — "keep one owner unless the split is deliberate".
- `gateway-ownership` fires with the first offending token's line, the
  facility, and the registered gateways; the `:write` gateway catches
  writers and never readers (pinned across write/read/blanket-use files).
- `no-sideways` needs no layer ranks (pure layer-name pairs, so declared
  `depends_on` cannot accidentally change its shape) and never fires when
  no corridor covers the pair; corridor self-imports and landings are
  exempt by design; rings with an unrankable `depends_on` cycle surface
  the layering check's Info ring note.
- Corridor declarations naming unknown layers, their own layer, or fewer
  than two distinct layers are Info-named (hand-edited data stays visible,
  never silently half-ignored).
- The `connects` stamp from scaffolded transiters never clobbers a
  hand-edited corridor (pinned).
- Container serde additive: new `gateway`/`connects` fields default via
  serde (`#[serde(default)]` semantics) — old `.harna/manifest.toml`
  files scan unchanged (pinned).

### Honest notes (not shipped, deliberately)

- **Registries as boundary constructs** (the third 0.3.10 roadmap item:
  name→implementation dispatch staying in registries) is **not** faked as
  a check — inventing a fake registry scanner would be a check with no
  rule and no reader. It stays on the roadmap (0.3.25's role-keyed
  checks land in that territory); the honest marker is this changelog
  section, not pretending a check exists.
- The **transit half is tested, adopted nowhere**: harna itself has no
  transiter corridor (no corridor, no gate) — corridor behavior is
  exercised by its 6+ dedicated tests with synthetic manifests/digests.
- Remaining dogfood advisories (Info, 21 total, unchanged in kind from
  0.3.9): the checker narrates harna's own near-duplicate test-fixture
  blocks across sibling components (gateway/transit/budgets/layering
  share manifest+digest fixture builders), granularity signals for
  `digest.rs`/`parity.rs`/`main.rs`/`tools.rs`/`scaffold.rs`/`gateway.rs`,
  and the known 19-block `main.rs`↔`tools.rs` pairs. Read guidance, not
  debt — the fixture-collection dedup candidate is future work.
- Vocabulary coverage notes (documented, bounded): `fs::read_exact`,
  `fs::File::read`, `.write_all(…)` forms and `tokio::fs` alias-call
  sites beyond the listed tokens are not separately inventoried; the fs
  token list is data — extending it is a list edit, not a code change.

### Data

- `.harna/manifest.toml` schema: two optional container fields
  (`gateway: Option<String>`, `connects: Vec<String>`) with serde
  defaults — additive, no migration; `schema_version` stays 1.
- Decision recorded: `2026-10-16-boundary-constructs--gateway---connects-as-manifest-data--ring-fixes.md`
  (why `gateway` is container metadata next to `role`, not an enum pivot
  on it; why the rings got fixed rather than silenced).

## 0.3.9 — 2026-10-16

### Added

- **Strict layering over the depends_on DAG** (new `layering` component,
  20-check registry): ranks derive from the graph — rank(layer) = 1 +
  max(rank of its depends_on), 0 for bases.
  - `depth-budget` — how far an import may reach down the ladder; the
    budget is the `limit` on a bound rule (`harna add-rule --limit 1
    --check depth-budget`); a 3-rank reach under a budget of 1 is named
    with its distance.
  - `no-bypass` — an import skipping ranks is flagged (Error default),
    **gated by rule data**: no rule bound to it, no gate — because
    hub-shaped layers (an interface reaching every layer at once) are
    legitimate and a blanket Error would have flooded harna's own tool
    UI with 150+ phantom bypasses in the first dogfood run. The
    flooding run is the design evidence, not the accident.
- **depends_on cycles are ranked-dead visibly**: layers in a cycle
  cannot be ranked — one Info violation names them instead of the
  strict checks silently skipping (pinned by test).
- **harna's own layer DAG was cyclic and now is not**: the first
  dogfood run of the rank check found memory ↔ search mutual — the
  digest DATA structs (`Digest/Module/Item/EXTRACTOR_VERSION`) moved
  home to the memory layer (`schema/digest_data.rs`); the store
  persists them, search reaches memory, memory does not reach search.
  Manifest updated (`memory` depends on nothing — it IS the base),
  deny.toml regenerated. The data structs also carry pinned serde
  tests now (old maps load, empty fields stay invisible).

### Edge cases (pinned by tests)

- **Sideways and upward imports are not bypasses**: rank distance 0
  (siblings/upward) is other checks' business; only downward jumps of
  ≥ 2 count.
- **Rule-gated like budgets**: no `no-bypass` rule → silent; a bound
  rule switches the layer-discipline on; deprecated rules stay out
  (same discipline as every policy check since 0.3.3).

## 0.3.8 — 2026-10-16

### Added

- **Reuse checks** (new `reuse` component, 18-check registry):
  - `no-copy-paste` — near-duplicate code blocks across files, named as
    the pair sharing them (5-line normalized shingles; comments,
    whitespace and literal contents stripped). Reads the LIVE sources of
    the run's file set — the digest stores signatures, never bodies —
    so `--changed` scopes to the changed pair and a full run compares
    the whole repo.
  - `single-owner` — one symbol, one home: the digest inventory watches
    for the same declared symbol in several modules. The first dogfood
    run proved it works on harna itself: `main.rs` and `tools.rs` share
    19 normalized blocks (the add-rule argument handling, duplicated at
    the CLI and MCP edges) — a genuine reuse finding, now visible.

### Edge cases (pinned by tests / dogfood findings)

- **"One symbol one home" is a within-crate type discipline**: the first
  dogfood run flagged 15 phantom collisions — fn names repeat
  idiomatically across containers (`check`, `build`) and cross-crate
  type names are normal namespacing (each crate's request DTOs). The
  check narrowed to declared types (`struct | enum | trait`) inside ONE
  crate; both refinements pinned by tests (Advisory; Info by default).
- **One shared 5-line truth is not a copy**: serial ids and pragmas
  that repeat once do not fire; a pair needs ≥ 2 shared runs.
- **Literal edits cannot dodge the detector**: strings inside the body
  are stripped before shingling, so renamed copies stay near-duplicates.
- **Test-fixture races hardened again**: the plan briefing fixture used
  a shared temp repo where two parallel tests deleted each other's
  `.harna` — thread-name tagging now (the same fix as 0.3.5's scaffold).

## 0.3.7 — 2026-10-16

### Added

- **Size budgets** (new `budgets` component, 16-check registry):
  - `container-size` — components per container, budget carried by the
    bound rule's `limit` in rules.toml.
  - `fan-out-cap` — outgoing imports per component, same limit discipline.
  `harna add-rule --limit N --check container-size` (and the MCP `limit`
  field) set the budget; thresholds are per-scope rule data, so different
  containers can carry different budgets. Registry default Info — a
  budget crossing is guidance unless a rule's `severity` promotes it.

### Edge cases (pinned by tests)

- **No budget declared, no gate**: a container or module without an
  applicable `limit` rule is never flagged — budgets are adopted as
  data, never a hidden global default.
- **The strictest applicable limit wins**: several rules set the same
  check, the lowest budget applies; a rule whose `scope` does not cover
  the container/module stays out.
- **Fan-out counts hosted targets, not ids**: two symbol imports into
  the same module are one target (longest-host resolution); fully
  unresolved dep ids fall back to their own stem (visible, not dropped).
- **Deprecated rules owe no budget**: a retired cap cannot keep gating.

## 0.3.6 — 2026-10-16

### Added

- **`no-cycles` check** (14-check registry): the import graph stays
  acyclic. Cycles are found by DFS over the digest graph (same dep
  resolution as dependents/parity) and reported as **the ring they form**
  (`a.rs -> b.rs -> a.rs`), deterministically normalized so the same ring
  reports exactly once (Error). harna's own map clears it.
- **`engine-own-container` check**: containers may declare
  `role = "engine"` in the manifest (additive serde default, opt-in
  before 0.3.25 makes roles mandatory): a module whose file leaf starts
  `<engine>-` may not live outside that engine's container path (Error)
  — variants and extensions live at home.

### Edge cases (pinned by tests)

- **Self-references are structural no-ops**: a module citing itself is a
  collapsed edge, never a one-member "cycle"; plain ancestor chains stay
  silent.
- **One ring, one report**: ring rotation makes the lexicographically
  smallest member open the chain, so whichever member's DFS closed the
  cycle, the message is stable.
- **Engines are opt-in**: containers without `role = "engine"` gate
  nothing; `<engine-name>-` marker prefixes keep false positives scoped
  to genuinely family-shaped names.
- **Whole-crate cross edges resolve for cycles too**: cross-crate cycle
  detection maps crate package names back to directories via the
  workspace manifests.

## 0.3.5 — 2026-10-16

### Added

- **Surface report — `SURFACE.md`** (sibling of ARCHITECTURE.md): the public
  API per crate/container as a generated, committed, diffable view. The
  manifest supplies the grouping and each container's declared `provides`;
  the committed digest supplies the observed public surface. Deterministic,
  staleness-gated: `harna render --surface` writes it alone; plain
  `harna render` refreshes every generated view the map can produce
  (honestly skipping the surface view when no code map exists, with a
  note); `harna render --check` gates staleness for CI. New MCP tool
  `harna_render` (22 tools now).
- **Manifest layering grew with the feature**: the `construction` layer now
  declares `depends_on = ["memory", "search"]` — the surface view reads the
  map, so the manifest says so, and `harna deny` compiles it.

### Edge cases (pinned by tests)

- **Ungoverned modules get an honest bucket** instead of silently dropping
  out of the surface view.
- **Container-nesty paths**: longest container path wins when containers
  nest; the declared `provides` line prints once per section.
- **Staleness is content, not mtime**: a changed digest moves the view even
  when its area did not change.
- **Flaky scaffold fixtures fixed** (render-adjacent, discovered on
  Windows): scaffold tests stamped their temp repos with wall-clock nanos —
  same-tick tests shared one repo and infected each other's manifests.
  The test thread's name (a collision-free id under cargo) replaces it,
  sanitized for Windows paths (`::` are legal Rust, illegal directories).
  Previously file times raced only occasionally; now it cannot race.

## 0.3.4 — 2026-10-16

### Added

- **Encapsulation — declared surface** (new `declared-surface` check, 12th
  registry id): containers and components can declare `provides`
  (manifest data, additive serde default). Cross-container imports may
  then consume only declared symbols — one check from two sides:
  declared in the manifest (with components adding to the container's
  own list), observed on the live import graph (shared resolution with
  the layer-boundary check). Separately, `priv` inventory items never
  cross containers — declared surface or not (Error).
- Adoption is additive: a container that never declared `provides` is
  held only to the private-symbol rule, so existing manifests phase in
  encapsulation container by container.

### Edge cases (pinned by tests)

- **Whole-module imports are not symbol references**: a dep whose stem
  lands exactly on a target module (e.g. `use crate::engine::billing;`)
  is exempt from the provides gate — no symbol can be named, so the
  check cannot misread it.
- **Brace-group imports collapse to their head module** (pre-existing
  extractor behavior, now documented for the surface check): a
  flat-path or inline reference carries symbol detail and gets gated;
  `use crate::engine::billing::{A, B}` is consumed at module level.
- **Innermost container wins** when stem boundary-matching reaches
  nested containers; same-container imports stay exempt (siblings know
  their own container's reality).
- **Missing digest degrades honestly**: without a code map only the
  declared-side gate runs; the private-symbol side needs the inventory.
- **Impl blocks are not private symbols** (found while dogfooding the
  check on harna itself): `impl Manifest {` is inventory-stamped `priv`
  and shares the type's name — the private-symbol set excludes impl
  kinds, or every cross-container use of a type with an impl block was
  flagged as "reaching a private symbol".

## 0.3.3 — 2026-10-16

### Added

- **Policy as data — `--env` on validate** (`harna validate --env dev` /
  `harna_validate` `env`): env-tagged rules now participate in enforcement
  when the declared environment matches — bindings (annotations), severity
  overrides and pattern rules all scope per environment; env-less rules
  always apply. The same rule set relaxes or intensifies per env without
  touching code.
- **Pattern rules (naming conventions as lint-as-data)**: a rule may carry
  `pattern = "<regex>"` — matched over file paths and declared names (pub
  and private, the same names the digest inventory catalogs) during
  validate. The rationale renders as the violation's hint; `severity`
  overrides the registry default (`pattern` check, Info by default).
  `harna add-rule --pattern` and the `harna_add_rule` `pattern` field set
  it at the edges; rule rendering shows it; the doctor's `rule-bindings`
  check also validates the regex compiles. New `pattern-rules` component
  (`enforcement/validate/pattern.rs`); the check registry grows to 11 ids.

### Edge cases (pinned by tests)

- **Bad regex is a violation, not a crash**: an un-compilable pattern in a
  hand-edited rules.toml is an Info violation with the regex engine's
  message, and the rest of the validate run carries on.
- **Pattern/check disagreement is visible**: a rule with both `pattern`
  and an explicit `check` other than `pattern` warns; `check`
  unchanged-valid still binds, so annotations survive.
- **Env filter covers every rule surface**: an env-tagged rule binds no
  checks, carries no severity overrides and matches no patterns outside
  its environment (dev rule invisible in a prod run; case-insensitive).
- **Unknown severity words fall back to the registry default** in pattern
  compilation (`severity = "warn"` → Info, never an error crash).
- **`use` line exemption kept**: bare sibling regex matching can only
  reach expression-position rules; pattern matching over *names* is
  inventory-aligned, never wholesale line scanning.

## 0.3.2 — 2026-10-16

### Added

- **`harna whereis <symbol>` / `harna_whereis`** (new `whereis` component):
  the symbol-level locator over the digest inventory (pub **and** private) —
  every file that declares the symbol (declaration as written, kind,
  visibility) plus the modules that reach it (ring-1 importers of the
  defining module, from the dependents graph). Queries accept a bare name
  (`Bindings`), module-qualified (`schema::Manifest`), or a trailing `()`.
- **Plan matching is inventory overlap**: the placement map and the full
  plan briefing now match tokens against the declared *item names* (the
  inventory), not only paths and public signatures — a token matching a
  private item name lands the right module even when no signature spells it
  (shared `module_hay` hay builder in place.rs).

### Fixed — real import-graph blind spots (found while dogfooding whereis)

- **Bare sibling-module references are dep edges now**: a call like
  `rule_bindings::Bindings::from_rules(...)` inside `validate/mod.rs` —
  legal Rust in expression position, unrooted — was invisible to the
  extractor, so those importers never reached parity fan-out, dependents or
  whereis. The extractor records them as `self::`-relative edges, in both
  the scan and the boundary check (declared mods of the live source);
  `harna dependents rule_bindings` proves the edge. `use` lines are exempt
  (bare sibling `use` cannot compile in Rust 2018+).
- **Incremental reuse is extractor-verified**: the digest now stamps its
  `extractor_version`; a map from an older extractor is rebuilt in full —
  file hashes say the file moved, never that the extractor came to see a
  new dependency shape. Old maps upgrade on the first scan after upgrading
  harna (no data migration; additive serde default).

### Edge cases (pinned by tests)

- **Symbol collisions are all listed**: the same name declared in several
  modules renders every definition, each with its own referencing set.
- **Miss → spelling rescue**: no exact (or case-insensitive) match falls
  back to name-contains suggestions (`fuzzy`, capped at 20 with a "narrow
  with module::symbol" hint); a full miss says so with the honest next
  actions instead of printing nothing.
- **Qualified queries are segment-scoped**: `schema::Manifest` must not
  reach `structure::Manifest` — the module stem has to carry the qualifier
  as whole path segments.
- **Stale/absent map refuses with the fix**: no digest → error telling the
  agent to run `harna scan`; whereis reads the map, never the files.

## 0.3.1 — 2026-10-16

### Added

- **Check registry as data** (new `check-registry` component): every
  rule-bindable check id now carries its description and default severity in
  `crates/harna-core/src/enforcement/checks.toml` — one canonical source
  loaded through `enforcement/checks.rs`. `BUILTIN_CHECKS` in
  `schema/rules.rs` is gone; rule bindings, the annotations, the doctor's
  binding coherence and the CLI/MCP add-rule edges all read the registry.
- **Severity overrides**: a bound rule may set its check's severity
  (`severity = "error" | "info"` in rules.toml) — one repo can demote a
  check to advisory or promote one to gating without touching harna. Additive
  serde default; no data migration. `harna add-rule --severity` and the
  `severity` field of `harna_add_rule` set it at the edges, and rule
  rendering shows it (`enforced by check: X (severity: info)`).
- **Doctor check `rule-bindings`**: bound check ids resolve and severity
  values are valid — the same registry the validators read, so a hand-edited
  rules.toml drift is visible at session start (Warn; never a fail).

### Fixed

- **Annotations now cover the whole report**: `decision-chain` and
  `parity-open` violations were emitted below the annotate point and silently
  missed their `[bound rule(s): ...]` annotation (0.2.9's promise). Override
  resolution and annotation now run after every check.

### Edge cases (pinned by tests)

- **Conflicting overrides do not pick a side**: two bound rules setting
  different severities for the same check produce an Info violation naming
  both, and the check keeps its registry default — enforcement stays
  conservative.
- **Unknown severity words** (`severity = "warn"` and friends) never silently
  no-op: Info violation with the valid set (`error | info`).
- **Severity without a bound check** has nothing to override — Info violation
  in validate, rejected outright at the add-rule edges.
- **Deprecated rules are dead**: they contribute no binding, no annotation
  and no severity override; a demotion via a deprecated rule cannot survive
  the rule being retired.


## 0.2.12 — 2026-10-14

### Added
- **`harna init` writes AGENTS.md** (when absent, or appends the harna
  section if the file exists blind): agents get the usage contract — the
  flow, the tools, the commit discipline — at initialization time, from git
  install to first session.
- **Doctor check `agent-instructions`**: AGENTS.md present and mentioning
  harna; WARN otherwise with `harna init` as the fix.
- **Portable install docs**: install directly from the git repo (tags =
  releases; private repos work via git-auth; `--tag` pins). README config
  examples now use PATH-based `harna-mcp` (no absolute paths) — cloning,
  branching and machine-moves are safe.

### Fixed
- `opencode.json` in this repo no longer hardcodes an absolute binary path.

## 0.2.11 — 2026-10-14

### Added
- **MCP resources**: agents can now *read* harna's memory without tool calls
  — `harna://manifest.toml`, `harna://rules.toml`, `harna://digest.json`
  exposed as read-only resources (`resources/list` + `resources/read`).
- **`deny-sync` check**: `deny.toml` staleness is visible instead of silently
  enforcing an OLD layer graph — Info-level "not generated" / "stale"
  violations with the next action. With the README CI recipe
  (`harna deny && git diff --exit-code deny.toml`) staleness fails the job
  explicitly.
- **CI recipes in the README**: a complete GitHub Actions job (doctor,
  full validate with exit-2 gating, deny regeneration + stale check,
  cargo-deny layering, workspace tests).

### Closed
- The **0.2.x patch train is complete** (0.2.2 → 0.2.11): machine output,
  parity ergonomics, placement guidance, dependents, digest v2,
  ignore-respect + incremental scan, rule-driven checks, doctor, release
  engineering. Next: **0.3.0** (tests-as-components, decision supersession,
  generated views, `harna plan`), then **0.4.0** (the language/stack plugin
  frontier).

## 0.2.10 — 2026-10-14

### Added
- **`harna doctor`** (`harna_doctor`, `--json`): the installation self-audit —
  `.harna` present, schema currency (offer `migrate` when behind; FAIL when
  data is from a newer harna), rules/manifest parseability, decisions,
  **digest staleness vs file mtimes** (warns with the count of changed
  modules), git presence, the commit gate (hook wired to harna), and agent
  wiring (local MCP config mentions harna). FAIL items exit non-zero so CI
  and shell scripts can gate; every check carries its next action.

## 0.2.9 — 2026-10-14

### Added

- **Rule-driven checks v1**: rules gain `check = "<check-id>"` — documentation
  (why) and enforcement (what) share one id. Known checks today:
  `no-hardcoded-urls`, `layer-boundary`, `manifest-path`,
  `manifest-sequence`, `manifest-depends`, `parity-open`,
  `component-granularity`. Violations of a bound check cite the rule
  (`[bound rule(s): id]`) so the agent gets the rationale with the fix.
- **Rule lifecycle**: `harna deprecate-rule / activate-rule / remove-rule`
  and MCP `harna_rule_lifecycle`. Deprecated rules stay in `rules.toml`
  visibly dead (`(DEPRECATED — do not follow)` in agent rendering) until
  removed or restored.
- **Binding coherence** (new `rule-bindings` component): unknown check ids
  are rejected at the CLI/MCP edge and warned as Info in validate.

### Re-scoped

- ~~0.2.8 frontend surface~~ → **language-plugin frontier (0.4.0 planned)**:
  TS/TSX + stack-guidance (tauri/gpui/egui) as *plugins* feeding the same
  digest schema; Rust stays the built-in scanner.

## 0.2.7 — 2026-10-14

### Added

- **Ignore-respect**: the scan and validate file lists now come from **git
  itself** (`git ls-files --cached --others --exclude-standard`) —
  `.gitignore`, `.git/info/exclude` and global excludes rule what harna
  sees, with zero ignore-parser drift. Fallback walk (internal skip list)
  only when git is unavailable. Applies to `harna scan`, `harna validate`,
  parity fan-out — everything downstream of the map.
- **Incremental scan**: `harna scan` reuses modules whose body hash is
  unchanged and re-parses only what moved; the CLI reports
  `reused N, scanned M, took T ms`. `--full` (CLI) / `full` (MCP) disables
  reuse. Digest.json remains the single artifact.

### Fixed (design, pre-shipped)

- **Reuse is hash-verified, never mtime-gated**: wall-clock ticks lie — file
  mtime and the map stamp can share a timer tick, so an mtime-based design
  silently reused changed files. Content hashes don't. (Found while writing
  the incrementality tests; the flaky first design never shipped.)
- Module ordering is now sorted in the built digest — stable diffs.

## 0.2.6 — 2026-10-14

### Added

- **Digest v2 — full inventory + hierarchy**: every module now records
  `inventory` (all declared items — pub **and** private — with visibility tag,
  kind, name; `impl` blocks included) and `mods` (declared submodules — the
  codebase's own hierarchy as written). `pub(crate)` is correctly read as a
  `pub`-boundary form (the first 0.2.6 build parsed it as private; tests now
  pin the behavior).
- **`harna tree`**: the map as the codebase's own directory hierarchy, each
  file annotated with public-surface and inventory sizes.
- **`private` diff class** (`items_changed`): private-inventory moves are
  visible in `harna diff --json`/prose as informational — private symbols
  cannot be imported across modules, so they never open fan-out targets.

### Unchanged by design

- Parity fan-out and layer analysis remain keyed on the public surface;
  the inventory widens the *map*, not the propagation rules.

## 0.2.5 — 2026-10-14

### Added

- **Dependents query** (`harna dependents <module>` / `harna_dependents`):
  "who breaks if I change this?" answered before the change — direct
  importers (ring 1) with the dep that creates each edge, expected knock-on
  (ring 2, deduped to one edge per module), and the manifest containers
  hosting the module. Accepts digest paths, crate-relative stems, or file
  names; requires a current code map (`harna scan`) and says so when absent.

### Fixed

- **Crate-name hyphen normalization**: package `harna-core` is imported as
  `harna_core`, but the graph compared the spellings literally — cross-crate
  importers were silently invisible to parity fan-out and the layer checks.
  One shared comparator (`crate_name_eq`) now normalizes everywhere.
- **Graph semantics hardened**: dep-to-module matching is now exact-bounded
  (equal stems, or one contained in the other's module boundary) — sibling
  modules no longer inherit each other's edges (`crate::schema::manifest`
  no longer reaches `schema/store`).
- **Ring-2 dedupe**: dependents render one edge per importing module.

## 0.2.4 — 2026-10-14

### Added

- **Guidance before code** (`harna place <idea>` / `harna_place`): the PLAN
  step of harna's flow. One briefing returns where similar things live (map),
  the layers/containers hosting them, applicable rules, the data convention
  and the engine-container principle — so the agent decides *where home is*
  before writing. Advisory only; `harna place` helps decide placement, the
  cram signal refines after.
- **Advisory granularity signal** (`component-granularity`, Info-level):
  component files that keep accreting get a split suggestion — harna imposes
  **no line limits**; the signal is guidance to read, never a gate.
- **Constants-are-data**: `harna scaffold --kind data` generates the value-set
  `.toml` + the **one loader** pair (code has zero literals; data files may
  split further as sets diverge). The principle is also recorded as the
  placement briefing's data convention.

### Changed

- **Dogfooded the granularity principle on harna itself**: `validate.rs`
  (~760 lines of varied concerns) became the `validate/` container with
  `urls`, `boundary`, `git`, `manifest` components under one API; parity's
  status vocabulary (`open`/`ack`/`noop`/`auto`) moved to
  `enforcement/statuses.toml` + `statuses.rs` loader — the first repo
  value-set living in a data file, exactly the constants-are-data pattern.

## 0.2.3 — 2026-10-14

### Added

- **Parity sweep**: `harna parity auto` with no id (CLI) or `id` omitted
  (MCP) re-verifies every event in one pass — one fresh scan backs the sweep.
- **Bulk resolution**: `harna parity bulk <id> --all-open` or `--paths <file>`
  (or stdin) applies `--status`/`--note` to many targets atomically, reporting
  missing ones; resolution provenance (who, date) now renders under manual
  `ack`/`noop` decisions.
- **Debt view**: `parity show --open` / `harna_parity` `open_only` renders only
  unresolved targets ("nothing owed" when clear).

## 0.2.2 — 2026-10-14

### Added

- **`harna status`**: one-shot health line (version, schema currency, map
  size, rules, parity debts, digest age) in text or `--json`.
- **Machine output**: `--json` on `validate`, `diff`, and `parity show` —
  CI and agents consume structured reports without parsing prose. The JSON
  validate report includes counts and a `blocks` verdict.
- **`--strict` on validate**: Info violations gate too (default: only Error
  blocks; the fix also corrects 0.2.1's behavior where Info rode along with
  Error gating).

### Changed

- The briefing container (`harna-core/src/composition`) is named `briefing`
  now; `status` is its second component (registered in the manifest).

## 0.3.0 — 2026-10-14

### Shipped

The full 0.3.0 feature front (five independently shipped feature sessions):

- **`harna handoff`** ✅: the burst journal — the agent records the
  current-burst state before compaction (`harna handoff --summary .. --next
  ..` / MCP `harna_handoff`, `--clear`/`clear: true` when the burst is
  resumed), and every session-start surface carries it: `harna_onboarding`
  renders the summary and ordered next steps, `harna status` reminds while
  one is open ("handoff on disk: N declared next step(s)"), doctor checks
  parseability. The data file is `.harna/handoff.toml`, overwritten per
  burst, git-tracked; captures the believed-open parity count for
  cross-checking. AGENTS contract updated.
- **`harna plan <idea>` full** ✅: the pre-code briefing, one call (CLI
  `harna plan`, MCP `harna_place`) reads as a decision proposal: placement
  candidates (similar modules + the containers to extend), decision context
  that is supersession-aware (a superseded decision is named VISIBLY DEAD
  with its successor — dead guidance cannot be followed), the rules that
  constrain the change (with bound check ids), the dependency fan-out to
  expect (dependents query on the matched modules), and the test
  obligations (registered tests of those components + the `no-test-update`
  advisory and `tests-declared` gate). `harna place` remains the quick
  placement map and shares the token matching.
- **Generated views** ✅: `harna render` emits `ARCHITECTURE.md` from the
  manifest — full layer/container/component tour, always-current, committed
  and diffable; never hand-edit it. `harna render --check` is the CI
  staleness gate (same recipe shape as the deny regeneration). This also
  formalized two fields the manifests had been ignoring: `summary`
  (component one-lines) and `description` (container one-lines) are now real
  schema data — previously serde silently dropped them on any manifest
  rewrite, and lost views would have had nothing to render from. Scaffold
  fills both from its request.
- **Decision supersession** ✅: decisions may carry `supersedes: <file>`; a
  superseded decision stays referenced but visibly dead (the log names its
  successor). `harna decisions` lists the log (superset chains visible) and
  `harna decisions <file>` renders one chain (successors, itself, ancestors).
  The `decision-chain` check (rule-bindable in rules.toml) keeps pointers
  real: unknown targets, self-references and cycles are Errors. `supersedes`
  is supported by `harna log-decision --supersedes` and the
  `harna_log_decision` MCP tool (additive, markdown-only — no schema change).
- **Tests as registered components** ✅: components may register their tests
  in the manifest (`tests = [...]`, optional — components without declared
  tests owe nothing). `validate` gains the `test-coverage` check: registered
  test paths must exist and hold real test code, and in the `--changed`
  enforcement loop a changed component whose registered tests were untouched
  in the burst gets the `no-test-update` advisory (inline tests compared by
  test region against `HEAD`; sibling test files by burst membership — the
  same governance `contents_changed` routes body-only edits through).
  Both `tests-declared` and `no-test-update` are bindable check ids in
  rules.toml.

### Packaging

- **First crates.io release**: all three crates publish
  (`harna-core`, `harna-cli`, `harna-mcp`) with workspace metadata
  (readme, keywords, categories); the version crank rides the front's
  completion (0.2.12 -> 0.3.0).
- **License changed to GPLv3**: `license = "GPL-3.0-only"` with the
  verbatim GNU GPL v3 text as `LICENSE` (was MIT-without-file).
  Decision recorded in the decision log.

## Planned — 0.3.1 .. 0.3.25 (the 0.3.x capability train, v2)

Twenty-five independently shippable versions taking harna from context
tool to software-engineering instrument: modularity, scalability,
consistency enforced as data; built for millions of lines and hundreds of
thousands of files. v2 reshapes the v1 plan by merging where one decision
serves two items (severity into the registry, encapsulation into declared
surface, metrics into the score, cycles into engine placement) and by
pulling the persistence boundary up from the tail — the layer-3-cannot-
write-to-the-base scenario is core, not a capstone. Sequencing stays
foundations-first: the registry and symbol data precede their consumers.
Each version: one atomic commit (code + manifest + decision + CHANGELOG),
parity cycle closed, doctor/validate clean, tag, publish. Backend only —
the interface layer stays as shipped. All additive serde defaults; no
data migrations.

### Phase A — policy foundations (0.3.1–0.3.5)

- **0.3.1 — check registry as data + severity overrides**: check ids carry
  description + severity semantics as data; a bound rule may set its
  check's severity. Rule bindings, annotations and the doctor registry
  gain one canonical source.
- **0.3.2 — `whereis <symbol>`**: symbol-level locator over the digest
  inventory (pub + private): definitions and referencing modules. Plan
  matching upgrades from token substring to inventory overlap.
- **0.3.3 — policy as data**: env-scoped validate (env-tagged rules inject
  into validate runs) + pattern rules (`pattern` regex over paths/declared
  names as a rule-bound check — naming conventions become lint-as-data).
- **0.3.4 — encapsulation + declared surface**: engines declare `provides`
  in the manifest and importers consume only declared public symbols —
  one check from two sides; reaching non-declared/private items across
  containers is a violation.
- **0.3.5 — surface report**: public API per crate/container as a
  generated, committed, diffable view (sibling of ARCHITECTURE.md).

### Phase B — placement & structure (0.3.6–0.3.10)

- **0.3.6 — no-cycles + engine-own-container**: the import graph stays
  acyclic (cycles named as the ring they form); engine variants and
  extensions live inside the engine's own container path.
- **0.3.7 — size budgets**: bounded components per container and imports
  per component (`container-size`, `fan-out-cap`); thresholds as rule
  data.
- **0.3.8 — reuse checks**: near-duplicate blocks named (`no-copy-paste`),
  one symbol one home (`single-owner`).
- **0.3.9 — strict layering**: layer ranks from the depends_on DAG;
  `no-bypass` (at most one rank per import) + `depth-budget` on
  reach-through chains.
- **0.3.10 — boundary constructs**: gateways (only registered containers
  touch fs/network/the base; storage write-only through its gateway) +
  transiter thinness and declared `connects` edges. ✅ shipped
  2026-10-16 — `registries` (name→implementation dispatch) deliberately
  deferred: faking it as a check would have been a check without a
  reader; see the 0.3.10 entry's Honest notes.

### Phase C — measurement (0.3.11–0.3.14)

- **0.3.11 — coverage ratio + test mapping**: components-with-registered-
  tests ratio in status; registered tests map to runnable cargo test.
  ✅ shipped 2026-10-16.
- **0.3.12 — metrics + health score**: fan-in/fan-out and boundary
  pressure from the import graph; the composite grade (coverage, debts,
  digest freshness, boundary pressure, coupling outliers).
  ✅ shipped 2026-10-16 (harna dogfoods 70/C).
- **0.3.13 — metric snapshots + trends**: scores persisted per recording;
  regressions become visible.
  ✅ shipped 2026-10-16 (`.harna/scores/` ledger, `harna metrics --record/--trend`, drift on status).
- **0.3.14 — hotspots**: git churn x digest size; the accretion advisory
  becomes data-driven.
  ✅ shipped 2026-10-16 (`hotspot` check, registry 24; trend attribution collected from 0.3.13).

### Phase D — scale (0.3.15–0.3.18)

- **0.3.15 — digest sharding + compaction**: `.harna/digest/` shards per
  container with an index file; smaller per-module serialization. The
  monolithic digest.json is what dies at millions of lines.
  ✅ shipped 2026-10-16 (index + per-container shards, bilingual loader, homes owned by memory).
- **0.3.16 — bounded queries**: whereis/dependents/metrics/onboarding read
  shards + index, memory-capped, never loading the world.
  ✅ shipped 2026-10-17 (`for_each_shard` bounded stream; index path lists
  resolve dependents; two-level index declined — see the 0.3.16 entry).
- **0.3.17 — fast paths**: incremental validate (body-hash reuse) +
  parallel scan + one-scan-many-consumers (diff, metrics, hotspots, score,
  validate cache reuse one scan).
  ✅ shipped 2026-10-17 (`score::full` one map read; parallel chunked
  scan; diff reuses the map hash-verified; validate's name extraction
  reuses the inventory; two-level index answered: one level is enough).
- **0.3.18 — archive & compaction of state**: closed parity events, old
  metric snapshots and superseded decisions compress into
  `.harna/archive/`; `.harna/` stays lean forever.
  ✅ shipped 2026-10-17 (`harna archive` + `harna_archive`; score retention
  collected from 0.3.14; resolution extends into the archive — filed, not
  ghosted).

### Phase E — collaboration & adoption (0.3.19–0.3.23)

- **0.3.19 — graph export**: DOT/Mermaid of topology + import edges with
  layer ranks (from 0.3.9).
  ✅ shipped 2026-10-17 (`harna graph` + `harna_graph`; shared edge
  resolution with the metrics ledger — incl. the cross-crate package-name
  fix; layer clusters in manifest order, rank anchors in DOT).
- **0.3.20 — `harna adopt`**: draft a manifest from an existing folder
  tree (proposed layers/containers/components with sizes).
  ✅ shipped 2026-10-17 (`harna adopt` + `harna_adapt`; stdout-only
  proposal, package-named layers, shared cross-crate resolution for
  depends_on, sizes in `draft:`-marked summaries; refuses non-empty
  manifests).
- **0.3.21 — profiles**: `init --from <path>` — another repo's rules/
  manifest skeleton becomes a starting point.
  ✅ shipped 2026-10-17 (verbatim copy, refused-before-overwrite on
  unparseable sources, missing pieces tolerate; no MCP init — bootstrap
  is human-owned; history never copies).
- **0.3.22 — collaboration artifacts**: parity templates (declared fan-out
  shapes for recurring change types), review checklists from fan-out +
  scoped rules + registered tests, and notes (release/PR drafts) from
  decisions + parity between refs.
  ✅ shipped 2026-10-17 (`harna templates add/remove/list` + `harna_notes`
  add/list, `harna notes review/release` + `harna_notes`; `fanout-shape`
  check registered Info-default; template data in `.harna/templates.toml`
  owned by memory like every persisted document; notes stdout-only,
  filed parity archive read by the release draft).
- **0.3.23 — debt register**: deliberate deferrals (component, reason,
  review date) as tracked data, distinct from parity debts; surfaces in
  status and score.
  ✅ shipped 2026-10-17 (`.harna/deferred.toml` + `harna deferred
  add/remove/list` + MCP `harna_deferred_add/list`; review = date or
  version; status surfaces N open / M OVERDUE; overdue reviews cost one
  debt each; seeded with the three standing prose deferrals).

### Phase F — hardening & layers as data (0.3.24–0.3.25)

- **0.3.24 — hardening**: `migrate --dry` preview, backup/restore
  verification, path normalization and long-path robustness.
  ✅ shipped 2026-10-17 (`migrate --dry` + MCP dry param; `harna restore`
  + MCP `harna_restore` tool 31; every backup parse-verified before use;
  Store::open normalizes the root and prefixes the verbatim form past
  240 chars; relative `..` heads survive when nothing exists to resolve).
- **0.3.25 — layers as data** (capstone): layers declare `role` (base /
  engine / gateway / orchestration-async / orchestration-sync / transit /
  interface / context); async fn usage is inventoried and allowed only in
  orchestration roles (engines stay sync); `rules-coverage` flags any
  layer/container without scoped rules; role scaffolds and role-keyed
  checks complete the "users might want other layers" contract.
  ✅ shipped 2026-10-17 (`ROLES` vocabulary data + `Layer.role` with
  container override; extractor v3 inventories `async-fn`; checks
  `role-async` (error) + `rules-coverage` (info) — registry 27; scaffold
  `--role` stamps containers, transiter corridors born `role = "transit"`;
  gateway-ownership narration now role-aware — the deferred
  `gateway-ownership-narration` register entry collected and removed;
  harna's own six layers stamped with their real roles).

## Planned — 0.2.x line (patch series, no schema changes required)

A patch train of small, independently shippable increments — one theme per
number. Shipped slots are marked.

- **0.2.2 — machine output + strictness** ✅
- **0.2.3 — parity ergonomics** ✅
- **0.2.4 — placement guidance + advisory granularity + constants-are-data** ✅
- **0.2.5 — dependents query** ✅
- **0.2.6 — digest v2: inventory + hierarchy + private diff class** ✅
- **0.2.7 — ignore-respect (via git) + hash-verified incremental scan** ✅
- **0.2.9 — rule-driven checks v1 + rule lifecycle** (next): rules may bind to
  an executable check id (`check = "layer-boundary"`); violations annotate
  their bound rule; unknown bindings are warnings; rules gain
  `deprecate`/`activate`/`remove`. The constants-are-data enforcer arrives
  as a rule binding once the inventory-based detector exists.
- ~~0.2.8 — frontend surface~~ → **re-scoped to the language-plugin frontier
  (0.4.0 planned)**: TS/TSX, tauri/gpui/egui guidance and per-stack scanners
  become *plugins* feeding the same digest schema, not built-in branches;
  Rust stays the built-in scanner until the interface lands.
- **0.2.10 — `harna doctor`**: self-audit of the harna installation — schema
  currency (offers migrate), digest staleness, manifest paths, hook presence,
  binary/version match.
- **0.2.11 — release engineering**: MCP resources (digest/rules as read-only
  resources), CI recipes, cargo-deny regenerated by validate when stale.

## 0.1.0 — 2026-10-14

### Added

- **Onboarding digest** (`harna_onboarding`): architecture overview + rules +
  code map in ~1k tokens, replacing AGENTS.md re-reading; survives compaction.
- **Rules store** (`.harna/rules.toml`): rules with id, text, rationale, scope,
  origin, `env` (dev/prod/...); env-tagged rules only surface to sessions
  declaring that env.
- **Manifest** (`.harna/manifest.toml`): layers → containers → primitive /
  compound components, with paths validated against disk.
- **Decision log** (`.harna/decisions/`): ADR-style markdown, one CLI/MCP tool
  (`harna_log_decision`).
- **Digest** (`.harna/digest.json`): signature-only code map, committed to git.
- **Scaffold** (`harna_scaffold`): primitive/compound structure generated from
  templates and registered in the manifest in the same step.
- **Transiter**: dedicated connector components between engines/layers.
- **Validate** (`harna_validate`): hardcoded URLs, cross-layer imports,
  manifest-path drift; `--changed` scopes to uncommitted git changes; exits
  non-zero for hooks/CI.
- **Schema versioning**: `schema_version` stamped in manifest/rules; newer data
  read by an old tool is refused; `harna migrate` upgrades with backups and a
  decision entry.

### Changed

- `harna-core` reorganized by harna's own principles (schema / search /
  enforcement / generation / composition containers).

### Dogfooding

- harna manages itself: `.harna/`, AGENTS.md, `.githooks/pre-commit`
  running `harna validate --changed`.

## 0.2.1 — 2026-10-14

### Fixed

- **Layer-boundary check runs on the real import graph**: internal imports are
  resolved to their target crate + module stem and matched against manifest
  containers; the importer's allowed set is its own layers plus declared
  `depends_on`. Replaces the name-prefix heuristic.
- **`super::`/`self::` fan-out resolved**: one shared resolver
  (`digest::resolve_dep`) backs parity matching and the layer check
  (`super::x` from module stem `a/b` reaches `a/x`; `self::x` reaches
  `a/b/x`; crate-root `super::` is invalid and skipped).
- **Honest dogfood finding**: the importer of parity via a call site
  (`composition/onboarding.rs`) was invisible to the graph — inline
  `crate::`/workspace-crate path references now count as dep edges wherever
  they appear in shipped code, not only in `use` statements.

### Added

- **`body_hash` per digest module + `contents_changed` diff class**: body-only
  edits are now observable in the API-change log. By design they do not open
  parity targets (they cannot name a fan-out); their governance stays with
  `validate --changed`.

### Known limitations

- Parity rings beyond direct importers are manual (documented).

## 0.2.0 — 2026-10-14

### Added

- **Internal import graph in the digest**: internal deps from `use` statements
  *and* inline `crate::`/workspace-crate path references; test sections
  excluded everywhere, so the map describes only shipped surface.
- **Digest diffing** (`harna diff --from <ref>` / `harna_diff`).
- **Sequence lists**: explicit declarative boot/init order for layers and
  containers; scaffold appends; validate checks unknown/duplicate/unlisted.
- **Parity ledger** (`harna parity` / `harna_parity*`): structural changes
  open a parity event whose fan-out = changed modules + importers; targets
  resolve `ack`/`noop`/`auto`; revisions invalidate old resolutions;
  **`harna parity auto`** re-verifies open targets against the baseline
  without a new generation (deleted modules never auto-clear). Open debts
  gate `harna validate`.
- **Compile-time layering** (`harna deny` / `harna_deny`): renders
  `Layer.depends_on` into a cargo-deny `deny.toml` — same-layer peers and
  declared dependencies are the wrappers; `cargo deny check bans` turns
  wrong cross-crate imports into build errors. `validate` checks that
  `depends_on` names exist.
