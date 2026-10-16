# Changelog

All notable changes to harna. Data format (`.harna/`) changes are called out
separately — they are the protocol between tool and repo; the tool migrates
them with `harna migrate`.

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
  registries (name->implementation dispatch stays in registries) +
  transiter thinness and declared `connects` edges.

### Phase C — measurement (0.3.11–0.3.14)

- **0.3.11 — coverage ratio + test mapping**: components-with-registered-
  tests ratio in status; registered tests map to runnable cargo test.
- **0.3.12 — metrics + health score**: fan-in/fan-out and boundary
  pressure from the import graph; the composite grade (coverage, debts,
  digest freshness, boundary pressure, coupling outliers).
- **0.3.13 — metric snapshots + trends**: scores persisted per recording;
  regressions become visible.
- **0.3.14 — hotspots**: git churn x digest size; the accretion advisory
  becomes data-driven.

### Phase D — scale (0.3.15–0.3.18)

- **0.3.15 — digest sharding + compaction**: `.harna/digest/` shards per
  container with an index file; smaller per-module serialization. The
  monolithic digest.json is what dies at millions of lines.
- **0.3.16 — bounded queries**: whereis/dependents/metrics/onboarding read
  shards + index, memory-capped, never loading the world.
- **0.3.17 — fast paths**: incremental validate (body-hash reuse) +
  parallel scan + one-scan-many-consumers (diff, metrics, hotspots, score,
  validate cache reuse one scan).
- **0.3.18 — archive & compaction of state**: closed parity events, old
  metric snapshots and superseded decisions compress into
  `.harna/archive/`; `.harna/` stays lean forever.

### Phase E — collaboration & adoption (0.3.19–0.3.23)

- **0.3.19 — graph export**: DOT/Mermaid of topology + import edges with
  layer ranks (from 0.3.9).
- **0.3.20 — `harna adopt`**: draft a manifest from an existing folder
  tree (proposed layers/containers/components with sizes).
- **0.3.21 — profiles**: `init --from <path>` — another repo's rules/
  manifest skeleton becomes a starting point.
- **0.3.22 — collaboration artifacts**: parity templates (declared fan-out
  shapes for recurring change types), review checklists from fan-out +
  scoped rules + registered tests, and notes (release/PR drafts) from
  decisions + parity between refs.
- **0.3.23 — debt register**: deliberate deferrals (component, reason,
  review date) as tracked data, distinct from parity debts; surfaces in
  status and score.

### Phase F — hardening & layers as data (0.3.24–0.3.25)

- **0.3.24 — hardening**: `migrate --dry` preview, backup/restore
  verification, path normalization and long-path robustness.
- **0.3.25 — layers as data** (capstone): layers declare `role` (base /
  engine / gateway / orchestration-async / orchestration-sync / transit /
  interface / context); async fn usage is inventoried and allowed only in
  orchestration roles (engines stay sync); `rules-coverage` flags any
  layer/container without scoped rules; role scaffolds and role-keyed
  checks complete the "users might want other layers" contract.

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
