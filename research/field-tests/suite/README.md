# The field-test suite

Before every minor release (and at every milestone proof), three fresh
agents test harna on three kinds of project, in parallel. None of them has
seen harna before; each gets one brief from this folder and nothing else.
What they find is the release's to-do list.

| Brief | Project type | What it proves |
|---|---|---|
| [rust-workspace.md](rust-workspace.md) | A new Rust workspace built by layers | Structure born correct: layers, contracts, services, the edit loop |
| [typescript-app.md](typescript-app.md) | A new TypeScript app (with a Rust side when it fits) | The same toolbox over TS: file edges, tsconfig, corridors with mirrored shapes |
| [adopt-existing.md](adopt-existing.md) | An existing repository adopted as it is | Adoption, ownership, affected CI, parity and migrations on real code |

## How to run it

1. Install the release candidate, both binaries:
   `cargo install --path crates/harna-cli` and
   `cargo install --path crates/harna-mcp`. `harna doctor` warns when
   they differ.
2. Start three subagents, one per brief, each in its own scratch folder.
   Tell each its log path: `research/field-tests/<date>-<brief>-<version>.md`.
3. Each agent works for about 45–60 minutes. It writes down every command
   with whether it helped, every break attempt as caught or missed with
   the exact output, bugs with exact repros, friction, and suggestions in
   priority order. Then it **deletes its project**: storage is finite. One
   large clone (`codex`) is kept for the adopt brief.
4. Fold the findings in before the release. A missed catch that should
   have been caught gets a regression story in
   `crates/harna-cli/tests/suite.rs`.

## The mechanical half

`crates/harna-cli/tests/suite.rs` replays the core of each brief's break
list through the real binary on generated projects, on every
`cargo test`. A release cannot lose a catch silently. The agents find what
no scripted story would think to try.

## Break lists

Every brief ends with a break list: what the agent must try to break, and
what harna must say. Each list grows by the catches the last field tests
asked for, and only grows.
