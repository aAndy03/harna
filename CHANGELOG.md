# Changelog

All notable changes to harna. Data format (`.harna/`) changes are called out
separately — they are the protocol between tool and repo; the tool migrates
them with `harna migrate`.

## Planned — 0.3.0 (future)

- **Tests as registered components**: manifest gets test paths per component;
  validate reports "component changed with no test update" — built on the
  `contents_changed` signal from 0.2.1.
- **Decision supersession**: decisions gain an optional `supersedes` field with
  pointer chains, so superseded decisions stay referenced but visibly dead.
- **Generated views**: `harna render` emits ARCHITECTURE.md (and other views)
  from the manifest — always-current prose, generated, never stale.

## Planned — 0.2.x line (patch series, no schema changes required)

A patch train of small, independently shippable increments. Order is by
leverage; each must be shippable in one session. Slots may exceed 0.2.9 —
one theme per number.

- **0.2.2 — machine output + strictness**: `--json` on `validate`/`diff`/
  `parity` (CI/query integration without parsing prose); `--strict` treats
  Info violations as gating; `harna status` one-shot health line
  (modules, rules, parity debt, manifest state) for agents and shell prompts.
- **0.2.3 — parity ergonomics**: `harna parity auto --all` (sweep every event,
  not one); bulk resolve (`--file <list>` or stdin batch); `parity show --open`
  filters to debts only; resolution notes auto-record who/when more cleanly.
- **0.2.4 — granularity principle + cram check + self-refactor**: engine
  containers own their variants — "add the variant inside the engine's
  container (new file or modified file), register it, let importers consume
  the engine"; a grown-out component becomes its own container. Enforced by
  a cram check (size/complexity signal, Info). Dogfood: `validate.rs`
  becomes the `validate/` container (urls / boundary / git components).
- **0.2.5 — dependents query**: `harna dependents <module>` answers "who
  breaks if I change this?" *before* the change, straight from the import
  graph (the planning half of parity; also feeds agent context).
- **0.2.6 — digest v2: full inventory + hierarchy**: items carry visibility
  tags (pub/priv), `impl` method signatures captured, declared submodules
  (`mods`) recorded so renders can print the source tree; diff gains a
  private-only move class; parity fan-out stays public-driven. The digest file
  stays one diffable artifact; hierarchy is data, tree view on render.
- **0.2.7 — scan performance**: mtime-based incremental scan cache for huge
  repos (only re-read changed files), with `--full` escape hatch; scan
  benchmark recorded in the repo.
- **0.2.8 — frontend surface**: TypeScript/TSX signature + import extraction
  in the digest (the full-stack story: same map, same checks for the web
  layer; hardcoded-URL and orphan checks extended).
- **0.2.9 — rule-driven checks v1**: rules may name an executable check id
  (`check = "no-hardcoded-urls"`); validate runs rules-as-checks and reports
  rule violations by rule id; rules gain `deprecate`/`remove` lifecycle.
- **0.2.10 — `harna doctor`**: self-audit of a repo's harna installation —
  schema currency (offers migrate), digest staleness vs mtimes, manifest
  paths, hook presence, binary/version match; one word per check.
- **0.2.11 — release engineering**: `--json` parity everywhere, MCP resources
  (read-only digest/rules as resources, not only tools), CI recipe in README,
  cargo-deny regenerated automatically by `harna validate` when stale.

All notable changes to harna. Data format (`.harna/`) changes are called out
separately — they are the protocol between tool and repo; the tool migrates
them with `harna migrate`.

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
