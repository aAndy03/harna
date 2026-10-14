# 0.3.0 feature 2: decision supersession (`supersedes` field, chain rendering)

Decisions may carry one structural field — `supersedes: <decision-file.md>`
written as a pointer line under the title. The superseded decision is never
deleted or edited: it stays referenced and becomes visibly dead —
`harna decisions` renders it with "superseded by <successor>". Nothing is
patched in place; a new decision re-owns the guidance, the old one keeps
pointing.

Surfaces: new `schema/decisions` component (markdown composition, parse,
list, chain render), `harna decisions [file]` CLI (list, or one chain:
successors, itself, ancestors), `--supersedes` on `harna log-decision` and
the `harna_log_decision` MCP tool. A `validate-decisions` component (check id
`decision-chain`, rule-bindable) keeps pointers real: unknown targets,
self-references and cycles are Errors — without it the chain would happily
render fiction.

Markdown-only and additive: no schema migration, no new data file, no
decision parse for tooling outside the pointer line. harna dogfoods it with
a true case: the 0.2.x patch-train plan decision (numbering and scope list
since contradicted by shipped history) is superseded by this front's
session-order plan decision.
