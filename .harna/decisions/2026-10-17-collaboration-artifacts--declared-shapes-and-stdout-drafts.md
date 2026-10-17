# Collaboration artifacts: declared shapes and stdout drafts (0.3.22)

`harna templates add` declares a recurring change type's fan-out shape as
data — `.harna/templates.toml` (schema_versioned, owned by the memory layer
like every persisted document; the matching behavior stays with parity in
enforcement, `parity-data` same pattern as `digest-data`, store persists and
absence means empty, not error) — each template = scope (module-path prefix,
empty = everywhere) + `max_targets` ceiling + the rationale of *why* that
shape is what it is. The new `fanout-shape` check sits in the registry
(Info default, rule-bindable like every check): when an open parity event
carries more targets *inside the template's scope* than the declared ceiling,
it names the event, both numbers and the declared reason — the shaped fan is
the in-scope count (a template declares one change type's shape, not the
whole event's reach; `schema2/` under a `schema/` prefix is a sibling, not a
hit). Templates are orientation, not verdicts: no templates declared =
nothing to notice, an unreadable templates file invents no findings, and a
bound rule may promote the severity — a declared shape you don't watch is
your choice. `harna notes review` composes the review checklist from the
three sources a review actually opens — the parity event's fan-out (`id`) or
a ref-spanning digest diff (`--from`), the scoped rules watching those paths
with their rationale, and the registered tests of the changed components,
mapped through the coverage map to runnable invocations — and
`harna notes release --from <ref>` drafts the release/PR note from the
decisions recorded and the parity events opened since the ref, "since"
computed against the *committed* ledgers via git ls-tree (read through the
briefing container's process gateway; live log and filed archive both read so
a filed decision still tells its story). Both notes are stdout-only drafts
under the export doctrine — generated data to read, prune and speak, never a
committed format — and neither judges: a checklist says what to look at, a
draft says what happened, and the author stays the author. MCP gains
`harna_template_add` / `harna_template_list` / `harna_notes` (28 tools;
edit `templates.toml` directly for removal — the data file is the
interface), and the manifest description's tool count was bumped.
