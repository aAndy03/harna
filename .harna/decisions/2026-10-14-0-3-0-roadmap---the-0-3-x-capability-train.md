# 0.3.0+: roadmap — the 0.3.x capability train (0.3.1 .. 0.3.27)

Twenty-seven independently shippable versions take harna from context tool
to software-engineering instrument. The full itemized plan lives in
CHANGELOG ("Planned — 0.3.1 .. 0.3.27"); this record captures the
non-obvious choices behind its shape.

Sequencing: foundations-first, not wishlist-order. The check registry as
data (0.3.1) and the symbol-level digest data (0.3.2) precede every
consumer of them (severity overrides, pattern rules, encapsulation,
surface, metrics, graph) — a consumer built on a registry that does not
exist becomes a second registry. Similarly, digest sharding (0.3.17)
precedes bounded queries (0.3.18) and any ambition of respecting
millions-of-lines scale.

Consolidations versus the raw wish list: compaction merged into sharding
(one data-format decision), one-scan-many-consumers merged into fast
paths (one performance decision), no-bypass and depth-budget paired
(strict layering is one principle with two measurements), gateways/
registries/transiter-thin clustered as "boundary constructs", and parity
templates/review/notes clustered as "collaboration artifacts" (all three
render from the same parity data). Granularity stays at one shipped theme
per version.

Backend-only constraint holds: no interface-layer growth (the 20-tool MCP
surface and CLI stay as shipped); everything lands in the schema/search/
enforcement/generation/composition engines.

Principles cited by the train and enforced with code: single
responsibility (engine scoping), encapsulation/information hiding
(private-surface imports), acyclic + explicit dependencies (no-cycles,
declared surface, connects edges), dependency inversion (gateways,
persistence boundary), strict layering (no-bypass, depth-budget),
functional core / imperative shell (orchestration roles), stable
dependencies (base as layer 0), and records-as-data for codebase change
over time (debt register, metric snapshots, rules-coverage).

The rule pattern stays the 0.3.0 pattern: seeded rule text + rationale in
rules.toml bound by `check = "<id>"` to builtin checks (registry from
0.3.1), violations carrying the rationale. `harna init` seeds the starter
principle set so agents meet the expectations before running anything.
