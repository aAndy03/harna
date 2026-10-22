# Checks

Generated from the check registry (`crates/harna-core/src/enforcement/checks.toml`) — do not edit.

70 checks. A rule binds one with `harna add-rule --check <id>`; `limit`, `values` and `pattern` say what the binding may carry. *Advises until bound*: the check reports as info until a live rule binds it.

| Check | Default | Takes | What it holds |
|---|---|---|---|
| `no-hardcoded-urls` | error (advises until bound) | — | no hardcoded http(s) URLs in source; route through endpoint/constants modules (advisory until a rule binds it) |
| `layer-boundary` | error | — | imports stay inside the importer's own layers plus declared depends_on |
| `manifest-path` | error | — | component paths declared in the manifest resolve on disk |
| `manifest-sequence` | info | — | sequence lists name declared components, once each (unlisted order is advisory) |
| `manifest-depends` | error | — | layer depends_on names exist (coherence of the manifest graph) |
| `parity-open` | error | — | structural changes left unresolved parity targets to acknowledge, record or verify |
| `roadmap` | error | — | the declared roadmap stays sound data (unique ids, known states, one current) and deferred reviews keyed to milestones point at something declared |
| `rename-linkage` | error | — | a declared manifest path missing on disk with the same file name alive elsewhere is a stale linkage — update the manifest, the move may be deliberate |
| `fanout-shape` | info | — | an open parity event's fan-out exceeds a parity template's declared ceiling for its scope (Info: the declared shape was walked past; a bound rule may promote) |
| `component-granularity` | info | limit | guidance: an accreting component file is a candidate to become its own container; advisory past 600 lines or the bound rule's `limit` |
| `append-only` | error | values | files under the paths the bound rule's `values` name (globs: db/migrations/**) are only ever added: a committed one edited, deleted or renamed since the base (HEAD, or HARNA_BASE in CI) is the finding (1.1.3) |
| `approval-drift` | error | — | an approval in .harna/approvals.toml whose files no longer hash to what the owner approved: an approved look or contract changed since (1.1.4); the owner re-approves with `harna approve <id>` |
| `approval-required` | error | — | since the base (HEAD, or HARNA_BASE in CI), a change a [[require]] in .harna/approvals.toml names landed unapproved: a new dependency without `dep:<name>`, or a change under a required path no current approval covers (1.1.4) |
| `approvals-changed` | info | — | the approvals register itself changed since the base: approvals added, re-approved or withdrawn, requirements added or dropped — named so the reviewer confirms the owner made them (an agent can edit a file as well as run a command) (1.1.4) |
| `data-literal` | info | limit, values | a literal list of facts (strings, numbers, or records of them: plans, prices, countries) of the bound rule's `limit` or more items (default 6) written into code outside the data home its `values` name (globs: data/**) (1.1.6) |
| `route-drift` | info | values | a route the frontend calls that no backend route declares: both sides are the bound rule's values — server:<regex> and client:<regex> (first capture group: the path), optional server-files:/client-files: globs and prefix:/api; parameters (:id, {id}, [id], ${id}) compare equal (1.1.7) |
| `ui-states` | info | — | a component that declares states or variants has a gallery (its registered tests of the `gallery` kind, or a story or gallery file) naming each one: the error, empty and disabled looks regress where nobody looks (1.1.5) |
| `variant-home` | info | — | a file named after a declared variant of a component (MiniCalendar.tsx beside a calendar that declares `mini`) forks it: the variant belongs in the original component (1.1.5) |
| `test-weakened` | info | — | a change that removes more assertions than it adds, or adds a skip or focus (#[ignore], .skip(, .only(, xit(), is named so its reason is recorded (1.1.3) |
| `growth` | info | limit | early signal: a file past half its size budget (600 lines, or the bound rule's `limit`) that grew in most of its recent commits — split it while that is cheap (1.1.0) |
| `tests-declared` | error | — | registered test paths exist and hold real test code |
| `no-test-update` | info | — | a changed component whose registered tests were untouched in the burst (advisory) |
| `decision-chain` | error | — | supersedes pointers in the decision log resolve (no ghosts, no self-references, no cycles) |
| `pattern` | info | — | a rule's regex over file paths and declared names (naming conventions as lint-as-data; the rule supplies regex + severity) |
| `declared-surface` | error | — | cross-container imports consume only the target container's declared `provides`; private symbols never cross containers |
| `no-cycles` | error | — | the import graph stays acyclic; a cycle is reported as the ring it forms |
| `engine-own-container` | error | — | a container declared role = engine keeps its variants/extensions inside its own container path |
| `container-size` | info | limit | a container stays bounded in components; the budget is the `limit` on the bound rule |
| `fan-out-cap` | info | limit | a component stays bounded in outgoing imports; the budget is the `limit` on the bound rule |
| `no-copy-paste` | info | — | near-duplicate code blocks across files are named; consistency beats copies |
| `single-owner` | info | — | one symbol, one home: a declared name in several modules is a colliding home to resolve |
| `no-bypass` | error | — | an import reaches at most one rank down the layer DAG (skipping ranks bypasses the intermediate layer's data rules) |
| `depth-budget` | info | limit | how far an import reaches down the rank ladder; the budget is the `limit` on the bound rule |
| `gateway-ownership` | error | — | a facility (fs \| net \| process \| base) is touched only inside containers that declare it with `gateway` (a `:write` suffix means the write side only); `crate:NAME` owns an external crate (only the owner's crate depends on it, only its modules name it); declaring one engages the check |
| `no-sideways` | error | — | when a container declares `connects = [from, to]`, cross-layer imports between the connected layers travel through that corridor (transiter), not in a direct edge |
| `transiter-thinness` | info | limit | a declared corridor (transiter: a container with `connects`) stays thin: declared items + signatures per module under the bound rule's `limit` |
| `hotspot` | info | limit | a module past the churn × items threshold (statistical, or the bound rule's `limit` as a fixed score) keeps changing while carrying weight — an accretion candidate with evidence |
| `role-async` | error | — | async fns (inventoried as async-fn by the map) appear only in the orchestration-async and interface roles; engines, bases, gateways, transiters and sync orchestration stay sync |
| `rules-coverage` | info | — | every declared container is watched by at least one scoped, live rule (an unwatched container is an advisory island) |
| `born-size` | error | limit | a NEW file (born in the working tree: added/untracked) arrives over the bound rule's `limit` lines — split per-scope before the burst's commit; born checked, no grandfathering |
| `folder-size` | info | limit | a folder holds at most `limit` source files (its mod.rs and tests.rs excluded); the budget is the `limit` on the bound rule |
| `name-cluster` | info | limit | files in one folder sharing a name token (`*_data.rs`, `cmd_*.rs`) at least `limit` times (default 3) are a folder that was never created |
| `no-panic` | error | — | production code in a bound rule's scope does not panic: no .unwrap()/.expect(/panic!/todo!/unimplemented! outside tests (opt-in: runs only where a rule binds it) |
| `ownership` | error | — | what a container `owns` (a capability type, a marker like #[op2) is constructed or declared only inside it; holding a capability elsewhere is fine |
| `escape-hatch` | error | — | every call of a declared escape hatch (container `escapes`, e.g. CheckedPath::unsafe_new) is counted — baseline them and let the count only fall |
| `suppression-reason` | error (advises until bound) | — | every #[allow(...)]/#![allow(...)] carries reason = "…" (advisory until a rule binds it) |
| `banned-api` | error | values | paths in the bound rule's `values` (e.g. std::fs::read) do not appear in its scope; `path => replacement` names the sanctioned path; `harna deny --clippy` writes both into each crate's clippy.toml |
| `cargo-layering` | error | — | a crate's Cargo dependencies on workspace crates stay inside its layers plus their declared depends_on (the DAG holds before any import exists) |
| `sync-group` | error | values, pattern | the files in the bound rule's `values` yield the same ordered list from its `pattern` (capture group 1) — registration lists kept in sync as data |
| `folder-entries` | error | values | a folder (the rule's scope; none = the repo root) holds only the entries listed in `values` — a new entry is a decision |
| `file-header` | error | values, pattern | files in scope with the extensions in `values` (default rs) begin with a line matching the rule's `pattern` |
| `referenced-by` | error | values, pattern | every file in the rule's scope (filtered by its `pattern` over file names) is referenced by name from some file under its `values` folders (no orphan fixtures) |
| `ignore-reason` | error (advises until bound) | — | every #[ignore] test says why: #[ignore = "…"] (advisory until a rule binds it) |
| `count-pin` | error | limit, values | the count of a kind (values[0]: fn/struct/… or a marker like #[op2) in scope equals the pinned `limit` |
| `generated` | info | values | the rule's scope is generated from its `values` sources: outputs skip size budgets; an output changed without its sources reads as a hand edit; the rule's `command` (the generator's check mode) is the proof harna's CI workflow runs — none named is advisory |
| `doc-claims` | info | — | paths and registered-runner commands cited in AGENTS.md / CLAUDE.md / CONTRIBUTING.md / copilot-instructions exist |
| `unreachable-module` | info | — | a `.rs` file no crate target's module tree reaches (fixtures, Rust-shaped data) — read as data, skipped by every source check; declare it with `mod` if it is meant to compile |
| `crate-conformity` | error | values | every crate in the rule's scope carries what its `values` require: `file:NAME` (a companion file, e.g. BUILD.bazel) and `toml:KEY[ = VALUE]` (a Cargo.toml key, e.g. lints.workspace = true); named per crate |
| `unreferenced-snapshot` | error (advises until bound) | — | an insta `.snap` whose producing module (its `module__…` file-name prefix) the map no longer knows — not a file, not an inline mod: its test is gone; advisory until a rule binds it |
| `sibling-isolation` | error | — | in a layer declared `isolated = true`, containers are peers that never import each other; they meet through a lower layer or a declared corridor |
| `api-parity` | info | — | paths into a crate an installed plugin mirrors resolve against the plugin's api.toml (generated from the release's source): an invented module or item is named with the version; glob re-exports are trusted |
| `registry` | info | — | project-wide (full validate only): names a declare:<regex> registers (group 1, comma-separated) must be used on another live line (use:<regex> with NAME); keys a bind:<regex> registers (group 1, scope group 2) must not repeat in one scope |
| `code-pattern` | info | pattern | the rule's regex over live code lines (comments and test items masked); values within:<regex> limits it to the block a matching line opens, unless:<regex> excuses a line, lines:all reads comments and test code too - the generic engine plugin conventions bind (no plugin code runs) |
| `deprecation-window` | info | limit | items marked #[deprecated(since = ..)] (Rust) or @deprecated since x (TS doc) are removed once the bound rule's limit (minor versions; a new major closes all) has passed the project's version (Cargo.toml / package.json) |
| `contract` | error | — | a container's frozen contract (.harna/contracts/<c>.toml, harna contract freeze) against today's surface: a frozen name gone or re-signed is a breaking change (error until re-frozen at a new major); a new provided name is compatible (info) |
| `service-coherence` | error | — | the services .harna/services.toml declares (harna service add): unique names, containers the manifest declares and that run in one service alone, one owning service per resource, a deploy folder that exists; corridors join declared services, have a sender and a receiver, and each message shape is a type the map holds or a schema file; nothing declared, nothing runs |
| `service-talk` | error | — | services talk only through declared corridors: code one service runs (.harna/services.toml) never imports code another service runs; shared containers are linked by anyone (0.8.2) |
| `corridor-shape` | error | — | both ends of a corridor speak its declared message shape: each service on it names the shape type, or declares its own type of that name with the same fields (case and _ ignored: posted_at = postedAt); a service never naming it is info (0.8.2) |
| `kind-coverage` | info | — | every registered component in the rule's scope is named by a file of the test kind(s) the rule's values list (by module name, file stem or declared surface) - e.g. every view has a ui test |
| `test-reach` | error | — | files of a declared test kind (manifest [[test_kinds]]: path globs + reach) and crates' dev-dependencies cross only the layers their kind declares; no kinds declared, nothing runs |
