# Agent instructions

This repository is harna — it manages itself with harna.

- **Source of truth is `.harna/`** (manifest.toml, rules.toml, decisions/, digest.json).
  Start every session by calling the `harna_onboarding` MCP tool.
- **After an edit burst**, call `harna_validate` with `changed_only: true`
  (or run `harna validate --changed`). Fix violations before committing.
- **Structure changes**: update `.harna/manifest.toml` in the same commit as the code,
  and call `harna_log_decision` for any non-obvious choice.
- **After a structural change burst**: `harna parity open <id>` to turn the diff into
  parity targets, then `harna_parity_resolve`/resolve each (ack or noop). Open debts
  gate validation.
- **New rules**: when the user states a preference or a convention is agreed, persist it
  immediately with `harna_add_rule` — instructions become data, not chat.
- **Before compaction / running out of context**: record the burst with
  `harna handoff` (summary + declared next steps) — `harna_onboarding`
  carries it into the next session; `harna status` reminds while it is open.
- **Containers never import each other sideways** in harna-core; composition may reach down.
  Layer map: `crates/harna-core/src/{schema,search,enforcement,generation,composition}`.
- Commit `.harna/` together with the code change it describes (atomic PRs).

## Born modular (0.3.26, rule `born-modular`, check `born-size`)

Per-scope files from the first line: scaffold the components first, then fill
them. Never write one oversized file intending to split it later — a new file
born over its budget (`limit = 600`) is an ERROR at `validate`, while files
with history stay covered by the advisory `component-granularity`/`hotspot`
checks. The vocabulary mapping sits in `.harna/terms.toml` (glossary:
agent vocabulary -> harna vocabulary; carried in every onboarding, audited
by `harna doctor`, exposed as the `harna://terms.toml` MCP resource).
Decline and failure are data too: `.harna/non-goals.toml` (deliberate
refusals — plan suppresses fully-covering proposals) and
`.harna/lessons.toml` (tried-and-failed with verdicts and evidence —
place/plan surface them as prior attempts); audited by `harna doctor`,
exposed as MCP resources, `harna non-goal add` / `harna lesson add`.

Autonomous ritual (trusted agent, auto-mode — each step is an audit point):
plan -> scaffold -> implement -> scan -> parity open -> resolve (real acks +
noops) -> validate --changed -> handoff -> atomic commit.
