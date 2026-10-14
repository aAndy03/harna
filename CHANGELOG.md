# Changelog

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

### Known limitations (v1)
- `super::`/`self::` import edges recorded but not resolved into fan-out.
- Parity fan-out triggers on published structural surface; private-body
  semantics are enforced by `validate --changed` (test hooks) rather than
  the ledger.

### Known limitations (v1)
- Parity imports: `super::`/`self::` edges are recorded but not resolved into
  the fan-out; deeper rings (folder/crate beyond importers) are manual.
- Violation severities: information violations are rendered but do not affect
  the exit code.
