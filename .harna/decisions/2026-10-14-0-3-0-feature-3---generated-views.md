# 0.3.0 feature 3: generated views (`harna render` -> ARCHITECTURE.md)

Views are regenerated prose, not hand-written docs: `harna render` emits
`ARCHITECTURE.md` at the repo root — a full layer/container/component tour in
manifest order, including depend_on declarations, container sequences and
registered tests (a test path equal to the component's own renders as
"inline"). The file is committed and diffable: structural changes review as
view diffs. `harna render --check` is the staleness gate for CI — same recipe
shape as the deny regeneration — and fails non-zero when the committed view
diverges from what the current manifest produces. Nothing hand-edits the
view; the manifest stays the single source of truth.

While wiring it, render exposed silent data loss: `Component` and `Container`
lacked the `summary`/`description` fields entirely — the manifest TOML's
one-liners were ignored by serde and would have been dropped on any manifest
rewrite. They are schema data now (additive serde defaults, no migration),
and scaffold fills them from its request. The rendering itself lives in
`generation/render.rs` (component `render`, registered in the structure
container's sequence; byte-deterministic output, unit-tested).

Passing fix: parity tests cleaned their temp dirs before use — Windows pid
reuse could surface a previous run's parity events and race the generation
assertions.
