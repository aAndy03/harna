# Command reference

Generated from the CLI's own parser (`crates/harna-cli/src/cli.rs`) — do not edit. Every command also answers `--help`.

## `harna init`

Initialize .harna/ in the repository

| Argument | Meaning |
|---|---|
| `--from` | Start from another repo's rules + manifest skeleton: path to its root (reads .harna/) or to a .harna/ directory itself. Copied verbatim as a starting point to prune |
| `--agents` | Opt AGENTS.md in: add harna's managed block (rendered from .harna/). Hand-written prose is never rewritten; without this flag init leaves AGENTS.md alone |
| `--profile` | Start from a profile: a shipped name (`harna profile list`), a folder path, or a folder under ~/.harna/profiles (0.4.2) |
| `--force` | With --profile/--from: replace a manifest/rules that already hold this project's data (otherwise init refuses) |

## `harna plugin`

Framework knowledge as versioned packs: list, show, add, remove

## `harna plugin list`

Installed plugins, then the ones shipped or under ~/.harna/plugins

## `harna plugin show`

What a plugin carries: mirrors, sources, rules, terms

| Argument | Meaning |
|---|---|
| `<SPEC>` | A plugin name or a path to a plugin folder |

## `harna plugin add`

Install (or upgrade) a plugin into .harna/plugins/<name>/

| Argument | Meaning |
|---|---|
| `<SPEC>` | A plugin name or a path to a plugin folder |

## `harna plugin remove`

Uninstall a plugin: its rules and terms stop applying

| Argument | Meaning |
|---|---|
| `<NAME>` |  |

## `harna plugin api`

Generate a plugin folder's api.toml from a release's source (for plugin maintainers; re-run on upgrade). Check api-parity reads it

| Argument | Meaning |
|---|---|
| `<DIR>` | The plugin folder to write api.toml into |
| `--crate` | The crate name as code spells it, e.g. gpui_kit |
| `--mount` | prefix=<crate src dir>, repeatable: "=<src>" for the crate's own root, "component=<src>" for a crate it re-exports as a module |

## `harna profile`

Starting shapes as data: list profiles, show what one declares

## `harna profile list`

Profiles shipped with harna and those under ~/.harna/profiles

## `harna profile show`

What a profile declares: layers, rules, qualities, next steps

| Argument | Meaning |
|---|---|
| `<SPEC>` | A profile name or a path to a profile folder |

## `harna affected`

What a change can break (0.7.5): containers changed, containers importing them, crates to test — for CI routing

| Argument | Meaning |
|---|---|
| `--base` | Compare against this ref (`<base>...HEAD`) plus the working tree |
| `--json` | Print JSON (a CI matrix) |

## `harna deprecations`

Deprecated items and their countdown to removal (0.7.4)

| Argument | Meaning |
|---|---|
| `--window` | The window in minor versions (default: the bound rule's limit, else 2) |

## `harna service`

Services as data (0.8.0): which exist, the containers each runs, the resources each alone writes

## `harna service add`

Declare a service: the containers only it runs, the resources only it writes

| Argument | Meaning |
|---|---|
| `<NAME>` |  |
| `--description` |  |
| `--containers` | Containers it runs (comma-separated) |
| `--owns` | Resources it alone writes, `kind:name` (db:ledger, queue:sync) |
| `--owners` |  |
| `--runs` | Folder of its deploy unit (crate or package) |

## `harna service set`

Change a service; a given list replaces the old one ("" clears it)

| Argument | Meaning |
|---|---|
| `<NAME>` |  |
| `--description` |  |
| `--containers` |  |
| `--owns` |  |
| `--owners` |  |
| `--runs` |  |

## `harna service remove`


| Argument | Meaning |
|---|---|
| `<NAME>` |  |

## `harna service show`

Every service, or one in full

| Argument | Meaning |
|---|---|
| `<NAME>` |  |

## `harna service require`

The readiness facets every service must answer before a release (comma-separated, your words: health,backups,migrations); "" clears

| Argument | Meaning |
|---|---|
| `<FACETS>` |  |

## `harna service ready`

Answer a facet for a service: where it lives, or `n/a: why`; "" removes the answer

| Argument | Meaning |
|---|---|
| `<SERVICE>` |  |
| `<FACET>` |  |
| `<ANSWER>` |  |

## `harna service corridor`

How services talk: a queue, an event bus, an API

## `harna service corridor add`

Declare (or replace) a corridor: who sends, who receives, what it carries

| Argument | Meaning |
|---|---|
| `<NAME>` |  |
| `--kind` | event \| queue \| api \| rpc \| … (your word) |
| `--from` | Sending services (comma-separated) |
| `--to` | Receiving services (comma-separated) |
| `--message` | A message and its shape: Name=<file>::<Type> or Name=<schema file>; repeatable |
| `--planned` | The messages are planned: declared before their code exists (validate reminds, preflight holds the release until it does) |

## `harna service corridor remove`


| Argument | Meaning |
|---|---|
| `<NAME>` |  |

## `harna services`

Every declared service (same as `harna service show`)

## `harna contract`

Versioned contracts (0.7.2): freeze a container's surface, show one

## `harna contract freeze`

Freeze the container's surface (its provides, with signatures) at a version; a breaking change is accepted by freezing a new major

| Argument | Meaning |
|---|---|
| `<CONTAINER>` |  |

## `harna contract show`

What a contract holds: version, names, signatures

| Argument | Meaning |
|---|---|
| `<CONTAINER>` |  |

## `harna owners`

Who answers for paths (0.7.0): owners per path, grouped per owner; --changed routes the working tree's change to its reviewers

| Argument | Meaning |
|---|---|
| `<PATHS>` | Repo-relative paths |
| `--changed` | Route the working tree's changed files instead |
| `--base` | With --changed: also the commits since this ref (`main`) |

## `harna rules`

List rules, optionally filtered by scope

| Argument | Meaning |
|---|---|
| `--scope` | Only rules applying to this scope path |
| `--env` | Only rules valid for this environment ("dev", "prod", ...). Env-tagged rules are hidden unless their env matches |
| `--agent` | Print the token-efficient agent rendering |

## `harna add-rule`

Add a rule and persist it to rules.toml

| Argument | Meaning |
|---|---|
| `--id` | Short stable id, e.g. no-hardcoded-urls |
| `--text` | The rule in imperative form |
| `--rationale` | Why the rule exists |
| `--scope` | Scope path(s) this rule applies to (repeatable; empty = global) |
| `--env` | Restrict this rule to one environment ("dev", "prod", ...) |
| `--check` | Bind the rule to an executable check id (violations cite the rule) |
| `--severity` | Override the bound check's severity ("error" \| "info"); requires --check |
| `--pattern` | Naming convention as lint-as-data: a regex matched over file paths and declared names (check `pattern`, the default); with --check code-pattern, over code lines |
| `--limit` | Budget for a budget check (`harna checks` lists which take one): components, imports, lines, files |
| `--value` | Argument for a list-taking check (repeatable): banned paths for banned-api, the files of a sync-group, … |
| `--command` | The command that proves the rule, run by harna's CI workflow (generated: the regenerate-and-diff check) |
| `--scope-signature` | Scope by meaning: a regex over declarations; the rule applies to every file declaring a match (\bMoney\b), within --scope if set |
| `--replace` | Replace the rule of this id where it stands (otherwise an existing id is refused) |

## `harna deprecate-rule`

Deprecate a rule: visibly dead in the file, removed later

| Argument | Meaning |
|---|---|
| `<ID>` |  |
| `--reason` | Why it is being retired (appended to the rationale) |

## `harna activate-rule`

Re-activate a deprecated rule

| Argument | Meaning |
|---|---|
| `<ID>` |  |

## `harna remove-rule`

Remove a rule outright (git history keeps the record)

| Argument | Meaning |
|---|---|
| `<ID>` |  |

## `harna try-rule`

Try one rule on a snippet or a file before trusting it: what validate would report there (code-pattern, pattern, banned-api)

| Argument | Meaning |
|---|---|
| `<ID>` |  |
| `--text` | Text to judge, as if it lived at --as (default src/lib.rs) |
| `--file` | A file to judge, at its own path |
| `--as` | The path the --text is judged at (scopes apply) |

## `harna log-decision`

Record a decision (ADR-style markdown under .harna/decisions/)

| Argument | Meaning |
|---|---|
| `<TITLE_ARG>` | The decision in one line (or --title) |
| `<BODY_ARG>` | Why, and what it rules out (or --body) |
| `--title` |  |
| `--body` |  |
| `--supersedes` | The decision file this one supersedes (name in .harna/decisions/). The old decision stays referenced but visibly dead |

## `harna decisions`

Decision log: every decision, superseded ones visibly dead

| Argument | Meaning |
|---|---|
| `<CHAIN>` | Optional: render the chain around this decision (filename or unique prefix) — successors first, then its supersedes ancestors |
| `--archived` | Include the decisions `harna archive` moved to .harna/archive/ (0.7.6) |

## `harna migrate`

Upgrade .harna/ data files to this tool's schema version (backups kept, verified readable). Restore rolls the newest backup back over the live file

| Argument | Meaning |
|---|---|
| `--dry` | Preview what would move — nothing is written |
| `--check` | For CI (0.9.0): fail when a file needs migrating or does not read with this harna; writes nothing |

## `harna restore`

Restore the newest backup of a data file (.harna/backup/<name>.vN). The backup is verified readable before it is swapped in

| Argument | Meaning |
|---|---|
| `<NAME>` | Plain data-file name, e.g. manifest.toml |

## `harna archive`

Archive closed parity events, superseded decisions and old score snapshots into .harna/archive/ — the live surface stays lean

| Argument | Meaning |
|---|---|
| `--keep` | Score snapshots kept live (newest kept first; the rest archive) |

## `harna tools`

harna's toolbox: one line per tool; an id for how to use one. Every tool is optional; `place`/`plan` offer the ones that fit

| Argument | Meaning |
|---|---|
| `<ID>` | A tool id for its detail, or `all` for the whole catalog |

## `harna checks`

List the checks a rule can bind (`--check`): one line each; an id (or `all`) for what it enforces

| Argument | Meaning |
|---|---|
| `<ID>` | A check id for its detail, or `all` |

## `harna manifest`

Print a component summary from the manifest

## `harna adopt`

Draft a manifest from the tree, or refresh an existing one (printed; you prune)

| Argument | Meaning |
|---|---|
| `--by-dir` | Group layers by top-level folder (ext/, runtime/, web/), crates as containers — for big mixed repos (0.6.4) |
| `--exclude` | Leave out paths (glob or folder, repeatable): fixtures, testdata, generated trees |

## `harna templates`

Parity fan-out templates: declared shapes for recurring change types

## `harna templates add`

Add or replace a template (same id overwrites)

| Argument | Meaning |
|---|---|
| `<ID>` | Stable id, e.g. schema-shard-change |
| `--scope` | Module-path prefix this change type touches (empty = everywhere) |
| `--max-targets` | The fan-out ceiling an event over this scope shouldn't walk past |
| `--rationale` | Why this shape is declared |

## `harna templates remove`

Remove a template by id

| Argument | Meaning |
|---|---|
| `<ID>` |  |

## `harna templates list`

List the declared templates

## `harna notes`

Render collaboration notes (review checklist / release draft)

## `harna notes review`

A review checklist: an event's fan-out (or the diff from a ref), the scoped rules and the registered tests over the changed paths

| Argument | Meaning |
|---|---|
| `<ID>` | Parity event id; omit to review against `--from`'s diff |
| `--from` | Git ref to diff against (default HEAD) when no event is given |

## `harna notes release`

A release/PR note draft: decisions + parity events since a ref

| Argument | Meaning |
|---|---|
| `--from` | Git ref the draft covers up to (default HEAD) |

## `harna deferred`

The debt register: deliberate deferrals with a review trigger

## `harna deferred add`

Add or replace an entry (same id overwrites)

| Argument | Meaning |
|---|---|
| `<ID>` | Stable id, e.g. fixture-builder-dedup |
| `--component` | What collecting this deferral would touch (component name, module or container path) |
| `--reason` | Why this is deliberately deferred, not an accident |
| `--review` | When to revisit: a date (2027-01-15), a version the project must reach (version:1.2.0), or a roadmap milestone (milestone:beta, or a declared id such as 0.9.0) |

## `harna deferred remove`

Remove an entry (it was collected, folded into a version, or the ask it guarded against never came)

| Argument | Meaning |
|---|---|
| `<ID>` |  |

## `harna deferred list`

List the open deferrals, overdue ones flagged

## `harna terms`

The glossary: agent vocabulary -> harna vocabulary (0.3.26)

## `harna terms add`

Add or replace a term (same word overwrites)

| Argument | Meaning |
|---|---|
| `<WORD>` | harna's canonical word, e.g. engine |
| `--means` | The meaning as harna defines it |
| `--alias` | Agent vocabulary mapping to this word (repeatable) |

## `harna terms remove`

Remove a term by word

| Argument | Meaning |
|---|---|
| `<WORD>` |  |

## `harna terms list`

List the glossary

## `harna non-goal`

The decline register: deliberate product refusals (0.3.27)

## `harna non-goal add`

Add or replace a refusal (same id overwrites)

| Argument | Meaning |
|---|---|
| `<ID>` | Stable id, e.g. no-remote-state |
| `--refusal` | What is refused, in imperative form |
| `--reason` | Why the refusal was made — the supersession briefing's evidence |

## `harna non-goal remove`

Remove a refusal by id (record why in the decision log)

| Argument | Meaning |
|---|---|
| `<ID>` |  |

## `harna non-goal list`

List the refusals in force

## `harna quality`

Long-term intent (0.4.0): what the software must hold over its life — scale, latency, durability, privacy, connectivity — as data

## `harna quality add`

Add or replace a quality (same id refines it)

| Argument | Meaning |
|---|---|
| `<ID>` | Stable id, e.g. scale-users |
| `--statement` | What must hold, in plain words |
| `--measure` | How it is measured or bounded: "1M users", "p95 < 200ms" |
| `--category` | A grouping word you choose: scale, latency, availability, durability, privacy, security, connectivity, … |
| `--scope` | Where it applies (repeatable); none = the whole system |
| `--why` | Why it matters |
| `--held-by` | A rule that holds it (repeatable): the quality's enforcement |

## `harna quality remove`

Remove a quality by id

| Argument | Meaning |
|---|---|
| `<ID>` |  |

## `harna quality list`

List the declared qualities

## `harna assume`

What the agent assumed and must ask the owner (1.1.2): add, confirm, correct, list, next

## `harna assume add`

Record an assumption the code now follows (same id replaces)

| Argument | Meaning |
|---|---|
| `<ID>` | Stable id, e.g. accounts-are-personal |
| `--statement` | What was assumed, as a statement |
| `--question` | The question that settles it, put to the owner |
| `--default` | What the code does meanwhile, if not the statement itself |
| `--impact` | What the answer would change |
| `--by` | The roadmap milestone it must be settled by |
| `--blocking` | Hold the release until it is answered |
| `--replace` | Replace one the owner already settled |

## `harna assume confirm`

The owner confirmed it: the code keeps following it

| Argument | Meaning |
|---|---|
| `<ID>` |  |
| `--answer` | The owner's words, if any |

## `harna assume correct`

The owner corrected it: say what is true instead

| Argument | Meaning |
|---|---|
| `<ID>` |  |
| `--answer` |  |

## `harna assume list`

Every assumption, open first

## `harna assume next`

The next questions for the owner: blocking first, then by milestone

| Argument | Meaning |
|---|---|
| `--n` | How many to ask at once |

## `harna approve`

The owner approves (1.1.4): freeze what was approved by content hash (a component's files, a change), or let a dependency land (`dep:<name>`). A later change to approved files is a debt until approved again. CLI only: the MCP tools never approve

| Argument | Meaning |
|---|---|
| `<ID>` | What is approved: a component (calendar), a change, or dep:<name> |
| `--paths` | The files it freezes (paths or globs); a re-approval keeps the earlier ones when omitted |
| `--note` |  |
| `--by` | Who approves (default: git user.name) |
| `--screenshot` | A screenshot of what was approved |

## `harna approvals`

What the owner approved, and which changes need the owner's yes first (1.1.4): list (default), require, unrequire, revoke

## `harna approvals list`

Every approval with its state (current, changed, gone), then the changes that need the owner's yes

## `harna approvals require`

Name a change that needs an approval before it lands

| Argument | Meaning |
|---|---|
| `<KIND>` | dependency-added \| path |
| `--paths` | For `path`: the globs it covers. For `dependency-added`: the manifests it reads (all when omitted) |
| `--reason` | Why the owner wants to be asked |

## `harna approvals unrequire`

Drop a requirement

| Argument | Meaning |
|---|---|
| `<KIND>` |  |
| `--paths` |  |

## `harna approvals revoke`

Withdraw an approval

| Argument | Meaning |
|---|---|
| `<ID>` |  |

## `harna vision`

The owner's vision, kept as written in .harna/vision.md (1.1.2)

## `harna vision set`

Keep the vision: from a file (the prompt as the owner wrote it) or text

| Argument | Meaning |
|---|---|
| `--file` | A file holding the vision, e.g. VISION.md |
| `--text` | The vision as text |

## `harna vision show`

Print the vision

## `harna lesson`

The dead-ends register: tried-and-failed records with verdicts (0.3.27)

## `harna lesson add`

Add or replace a tried-and-failed record (same id overwrites)

| Argument | Meaning |
|---|---|
| `<ID>` | Stable id, e.g. ps-roundtrip-source-files |
| `--claim` | The claim — what was attempted, in the direction of the failure |
| `--verdict` | The verdict — why it did not work out |
| `--evidence` | Evidence pointers: decisions, commits, events (repeatable) |

## `harna lesson remove`

Remove a recorded dead end by id

| Argument | Meaning |
|---|---|
| `<ID>` |  |

## `harna lesson list`

List the recorded dead ends

## `harna layer`

Edit a layer in the manifest: depends_on, role (0.3.42)

## `harna layer set`

Set a layer's fields; an empty value clears (`--depends-on ""`)

| Argument | Meaning |
|---|---|
| `<NAME>` | Layer name |
| `--depends-on` | Layers this layer may import, comma-separated (replaces the list) |
| `--role` | Role from the role catalog (base, engine, gateway, …) |
| `--description` |  |
| `--isolated` | Sibling isolation: its containers are peers that may not import each other (true \| false) |
| `--owners` | Who answers for the layer, comma-separated (`@team`, a mail) |

## `harna layer remove`

Take a layer and its containers out of the manifest (files stay)

| Argument | Meaning |
|---|---|
| `<NAME>` |  |

## `harna test-kind`

Test kinds (0.4.7): where each kind of test lives and which layers it may reach; declaring any holds tests and dev-dependencies to it

## `harna test-kind add`

Add or replace a test kind (same name replaces)

| Argument | Meaning |
|---|---|
| `<NAME>` | The kind's name, e.g. integration |
| `--path` | Path glob holding it (repeatable): crates/*/tests/**, **/*_tests.rs |
| `--reach` | Layer it may reach beyond its home's own (repeatable); * = any |
| `--description` |  |
| `--contains` | A marker its files contain (regex), e.g. '#\[gpui_kit::test' (a regex: escape the bracket): files in the globs without it are not of this kind |

## `harna test-kind remove`

Remove a test kind

| Argument | Meaning |
|---|---|
| `<NAME>` |  |

## `harna test-kind list`

List the declared test kinds

## `harna slot`

Planned components (0.4.1): declare a slot before its code; scaffold with its name fills it

## `harna slot add`

Declare a slot: a component whose code does not exist yet

| Argument | Meaning |
|---|---|
| `<CONTAINER>` | Container the slot belongs to |
| `<NAME>` | Component name, e.g. warm-cache |
| `--path` | Where its code will live, e.g. crates/app/src/cache/warm.rs |
| `--summary` | What it will do, and behind which boundary |

## `harna slot fill`

Mark a slot built once its code exists

| Argument | Meaning |
|---|---|
| `<NAME>` |  |

## `harna slot list`

List the open slots

## `harna container`

Edit a container in the manifest: rename, path, gateway, role (0.3.42)

## `harna container rename`

Rename a container and relink its layer's sequence (paths move with `harna move`)

| Argument | Meaning |
|---|---|
| `<OLD>` |  |
| `<NEW>` |  |

## `harna container remove`

Take a container out of the manifest (its files stay on disk)

| Argument | Meaning |
|---|---|
| `<NAME>` |  |

## `harna container surface`

Draft the container's declared surface (provides) from its code: the root module's re-exports, else its public items (0.4.5)

| Argument | Meaning |
|---|---|
| `<NAME>` | Container name |
| `--apply` | Write the draft as the container's provides |

## `harna container register`

Register an existing file as a component (what `harna adopt`'s refresh proposes)

| Argument | Meaning |
|---|---|
| `<CONTAINER>` | Container name |
| `<PATH>` | Repo-relative file |
| `--name` | Component name (default: the file stem, kebab case) |
| `--summary` | One line: what the component is for |

## `harna container add`

Declare a container over a folder, any language (no files written)

| Argument | Meaning |
|---|---|
| `<NAME>` | Container name |
| `--layer` | Layer it sits in (created when new) |
| `--path` | The folder it governs, e.g. web/src/features/ledger |
| `--description` |  |

## `harna container unregister`

Take a component out of the manifest (its files stay)

| Argument | Meaning |
|---|---|
| `<COMPONENT>` | Component name |

## `harna container tests`

Add or remove the tests registered for a container's components — every component unless --component narrows it (0.3.49)

| Argument | Meaning |
|---|---|
| `<CONTAINER>` | Container name |
| `--component` | Only these components (repeatable); default: all of them |
| `--add` | Test file to register (repeatable), e.g. crates/x/tests/api.rs |
| `--remove` | Test file to unregister (repeatable) |

## `harna container set`

Set a container's fields; an empty value clears (`--gateway ""`)

| Argument | Meaning |
|---|---|
| `<NAME>` | Container name |
| `--path` | Source directory the container owns, e.g. src/backend/index |
| `--gateway` | Owned facilities: fs \| fs:write \| net \| process \| base, or crate:NAME for an external crate (crate:rusqlite); comma-separated |
| `--role` | Role from the role catalog; overrides the layer's role |
| `--description` |  |
| `--owns` | What only this container constructs/declares: capability types (CheckedPath) or markers (#[op2), comma-separated |
| `--escapes` | Counted escape hatches (CheckedPath::unsafe_new), comma-separated |
| `--provides` | Declared surface: the names other containers may import, comma-separated; everything else stays private (declared-surface) |
| `--owners` | Who answers for the container, comma-separated (`@team`) |

## `harna baseline`

The ratchet ledger: today's errors frozen per (check, file), allowed to fall, never to grow (0.3.44)

## `harna baseline record`

Freeze today's error counts per (check, file); growth then fails

| Argument | Meaning |
|---|---|
| `--check` | Only this check (e.g. no-panic); default: every erroring check |

## `harna baseline tighten`

Lower each baseline to the errors left (never raises one)

## `harna baseline show`

List the ledger

## `harna command`

The repo's own commands: what to run, what it proves, where (0.3.46)

## `harna command add`

Add or replace a command (same name overwrites)

| Argument | Meaning |
|---|---|
| `<NAME>` | Stable id, e.g. test-spec |
| `--run` | The exact invocation, e.g. "./x test-spec" |
| `--proves` | What a pass proves, e.g. "spec tests under tests/specs" |
| `--runs` | local \| ci \| both (default both) |
| `--scope` | Paths it covers (repeatable); none = the whole repo |

## `harna command remove`

Remove a command by name

| Argument | Meaning |
|---|---|
| `<NAME>` |  |

## `harna command list`

List the registry

## `harna roadmap`

The roadmap: declared milestones, never generated (0.3.28)

## `harna roadmap add`

Add or replace a milestone (same id overwrites)

| Argument | Meaning |
|---|---|
| `<ID>` | Stable id, e.g. 0.3.28 or phase-h |
| `--title` | What the milestone delivers, in one line |
| `--state` | One of declared\|current\|done\|dropped (default declared) |
| `--notes` | Optional longer goal statement |

## `harna roadmap set`

Move a milestone's state (the whole point of the roadmap: NOW moves)

| Argument | Meaning |
|---|---|
| `<ID>` | Milestone id |
| `<STATE>` | One of declared\|current\|done\|dropped |

## `harna roadmap remove`

Remove a milestone by id (say why in the decision log if it's a drop)

| Argument | Meaning |
|---|---|
| `<ID>` |  |

## `harna roadmap list`

List the declared path

## `harna graph`

Print the generated dependency graph (DOT or Mermaid)

| Argument | Meaning |
|---|---|
| `--format` | Output format: dot \| mermaid |

## `harna render`

Emit generated views (ARCHITECTURE.md + SURFACE.md) from the manifest and the committed map — committed and diffable; never hand-edit what render owns. Also fills the harna block of agent instruction files

| Argument | Meaning |
|---|---|
| `--check` | Verify the committed views are current instead of writing (CI) |
| `--surface` | Render only the public-surface view (SURFACE.md, 0.3.5). The default regenerates every generated view the map can produce |
| `--scope` | Create (opt in) a container folder's own AGENTS.md: its layer, owners, surface and the rules covering it (0.7.6). Repeatable |

## `harna validate`

Check the codebase for conformance violations (drift, hardcoded values, layer violations)

| Argument | Meaning |
|---|---|
| `--changed` | Only check `.rs` files with uncommitted git changes (pre-commit / agent edit loop) |
| `--strict` | Info violations also block (default: only Error blocks) |
| `--json` | Emit a JSON report instead of prose |
| `--env` | Environment the session declares ("dev", "prod", ...): env-tagged rules for other environments stay out of enforcement |
| `--max-files` | Max files to check (safety cap) |
| `--check` | List every finding of one check (others are summarized by count) |

## `harna onboarding`

The session briefing: a summary, or one section in full

| Argument | Meaning |
|---|---|
| `<SECTION>` | rules \| glossary \| map \| registers \| conventions \| all (omit for the summary) |

## `harna status`

One-shot health line: map size, rules, parity debts, digest age

| Argument | Meaning |
|---|---|
| `--json` |  |

## `harna dependents`

Who imports this module? ("Who breaks if I change this?")

| Argument | Meaning |
|---|---|
| `<MODULE>` | Digest module path, crate-relative stem, or file name |
| `--json` |  |

## `harna whereis`

Where does this symbol live? Definitions + referencing modules

| Argument | Meaning |
|---|---|
| `<SYMBOL>` | Symbol name, optionally module-qualified (e.g. module::Symbol) |
| `--json` |  |

## `harna tests`

List registered tests mapped to runnable `cargo test` commands (0.3.11)

| Argument | Meaning |
|---|---|
| `--component` | Only this component (by manifest component name) |
| `--json` | Emit machine-readable JSON |

## `harna metrics`

Measure the import graph (fan-in/fan-out, boundary pressure, coupling outliers) and print the composite health grade (0.3.12)

| Argument | Meaning |
|---|---|
| `--json` | Emit machine-readable JSON |
| `--record` | Persist the current grade as a timestamped snapshot (0.3.13) |
| `--trend` | Show the recorded trend with deltas (regressions flagged) |

## `harna tree`

Print the codebase's own hierarchy from the committed digest

## `harna doctor`

Installation self-audit: schema, data files, git, hook, agent wiring

| Argument | Meaning |
|---|---|
| `--json` |  |

## `harna finish`

End a work session: refresh the map and views, check the change, list what is left (parity, handoff, roadmap) with the command for each

## `harna preflight`

Release gate: validation errors + doctor failures + open parity debts + map freshness compose into one PASS/FAIL (0.3.29). Exits 2 on FAIL. The test suite is NOT run here (harna never executes project code) — run it yourself, then release

| Argument | Meaning |
|---|---|
| `--json` | Emit machine-readable JSON |
| `--max-files` | Max files to check (safety cap) |

## `harna validate-intent`

Truthfulness reconciliation: the mechanically verifiable claims (package version, changelog head, roadmap NOW/DONEs, parity silence) cross-checked against the state (0.3.30). DRIFTs name both sides; exits 2 on drift

| Argument | Meaning |
|---|---|
| `--json` | Emit machine-readable JSON |

## `harna acknowledge`

The advisory ledger: lock away accepted Info findings with a reason, re-arm them, or list what is locked (0.3.31). Locked advisories render as one-line receipts and stay out of strict-mode gating; errors can never be acknowledged

## `harna acknowledge add`

Lock away an accepted advisory: a specific Info finding (check + file) seen, reasoned about, kept with a why. Errors are never acknowledgeable — gates are not suggestions

| Argument | Meaning |
|---|---|
| `<CHECK>` | The check that fired, e.g. no-copy-paste |
| `--file` | The file it fired on, repo-relative |
| `--reason` | Why the advisory is accepted rather than fixed |

## `harna acknowledge remove`

Re-arm an acknowledged advisory by check + file

| Argument | Meaning |
|---|---|
| `<CHECK>` | The check that was acknowledged |
| `--file` | The file it was acknowledged on, repo-relative |

## `harna acknowledge list`

List the locked-away advisories with their reasons

## `harna hooks`

Install harna's own enforcement — git pre-commit, Claude Code hooks, CI — so the gates run without anyone remembering (0.3.36)

## `harna hooks install`

Write and wire the hooks: git pre-commit (+ core.hooksPath), Claude Code hooks merged into .claude/settings.json, a CI workflow. Never overwrites a file you wrote (unless --force)

| Argument | Meaning |
|---|---|
| `--target` | Only these targets: git \| claude \| ci (repeatable; default all) |
| `--bin` | Command the Claude Code hooks run (default: `harna` on PATH) |
| `--force` | Replace files that were not written by harna |
| `--craft` | Add the Rust craft gates: fmt --check + clippy -D warnings in the pre-commit hook, fmt/clippy/tests as a CI job |
| `--no-craft` | Drop the craft gates (without either flag, an install keeps what the installed files have) |

## `harna hooks status`

Which hooks are installed and current

| Argument | Meaning |
|---|---|
| `--bin` |  |

## `harna hook`

Answer an agent-runtime hook: session-start | post-edit | stop (what the installed Claude Code hooks call; exit code is the verdict)

| Argument | Meaning |
|---|---|
| `<EVENT>` |  |

## `harna decompose`

Split planner: map an oversized file's top-level items and print HOW to split it (dispatch-split or prefix-cluster) plus the manifest sketch (0.3.31). Application stays manual — the cut lines are visible, the judgment is yours

| Argument | Meaning |
|---|---|
| `<FILE>` | File to plan, repo-relative, e.g. crates/x/src/main.rs |

## `harna place`

Placement briefing for an idea, before code exists (PLAN step)

| Argument | Meaning |
|---|---|
| `<IDEA>` | The idea in plain words, e.g. "timezone picker data" |

## `harna plan`

Full pre-code briefing: placement + decision context (supersession- aware) + rule bindings + dependency fan-out preview + test obligations

| Argument | Meaning |
|---|---|
| `<IDEA>` | The idea in plain words, e.g. "timezone picker data" |

## `harna handoff`

The burst journal: record the current-burst state before compaction. No arguments shows the current handoff; the next session resumes from onboarding/status (which carry it)

| Argument | Meaning |
|---|---|
| `--summary` | What this burst was doing (overwrites any previous handoff) |
| `--next` | Declared next step, in order; repeatable with --next |
| `--clear` | The burst is resumed: remove the recorded handoff |

## `harna scaffold`

Scaffold a new primitive/compound component and register it in the manifest

| Argument | Meaning |
|---|---|
| `--layer` | Layer name (created if missing) |
| `--container` | Container name within the layer (created if missing) |
| `--name` | Component name in kebab-case, e.g. user-repo. Repeatable: one call scaffolds several components into the same container |
| `--kind` | primitive = leaf; compound = folder with mod.rs + starter child |
| `--path` | The component's path (ending in its name), or the folder it goes in. Defaults to the container's path (or src/<layer>/<container>) + /<name> |
| `--description` | One-line description embedded in the generated files |
| `--role` | Role for the container (base/engine/gateway/orchestration-async/ orchestration-sync/transit/interface/context) — stamped on it |
| `--empty` | Write the doc header only, no starter stub (for code you write yourself) |
| `--new-crate` | Create the declared container's crate first: lib or bin (Cargo.toml, workspace member, src/lib.rs or src/main.rs) |

## `harna move`

Move a file or folder and keep it linked (0.3.39): manifest paths, tests and rule scopes relinked, `mod` declarations moved, `use` paths rewritten across the workspace

| Argument | Meaning |
|---|---|
| `<FROM>` | Repo-relative source, e.g. crates/x/src/schema/terms_data.rs |
| `<TO>` | Repo-relative destination, e.g. crates/x/src/schema/registers/terms.rs |

## `harna transiter`

Scaffold a corridor (transiter): the one named, thin hand-off between two layers — a command bus, an event channel, an API facade

| Argument | Meaning |
|---|---|
| `--from` | Layer the connection starts from, e.g. "api" |
| `--to` | Layer the connection reaches, e.g. "billing" |
| `--name` | Optional custom name; defaults to transiter-<from>-to-<to> |

## `harna scan`

Rebuild .harna/digest/ shards by scanning the repo

| Argument | Meaning |
|---|---|
| `--full` | Disable mtime reuse and re-parse every respected file |
| `--max-files` | Max files to scan (safety cap) |

## `harna deny`

Generate deny.toml (compile-time layering) from the manifest

## `harna diff`

Compare the working tree's code map against a committed digest

| Argument | Meaning |
|---|---|
| `--from` | Git ref whose committed digest is the baseline (shards or legacy monolith) |
| `--max-files` | Max files to scan (safety cap) |
| `--json` | Emit machine-readable JSON |

## `harna parity`

Parity ledger: open, show, and resolve change-propagation debts

## `harna parity open`

Open (or re-open = revise) a parity event from structural changes

| Argument | Meaning |
|---|---|
| `<ID>` | Event id, e.g. change-store-api |
| `--kind` | Event kind (rule-change, manual, digest-diff...) |
| `--from` | Git ref whose committed digest is the baseline |
| `--max-files` |  |

## `harna parity show`

Show parity events with open-debt counts

| Argument | Meaning |
|---|---|
| `--id` |  |
| `--open` | Only events with unresolved targets (debt view) |
| `--json` | Emit machine-readable JSON |

## `harna parity resolve`

Resolve one target of one event

| Argument | Meaning |
|---|---|
| `<ID>` |  |
| `<PATH>` | Module path of the target (as listed by `parity show`) |
| `--status` | ack (work done) \| noop (nothing needed) \| auto (verified) |
| `--note` |  |
| `--by` |  |

## `harna parity auto`

Re-verify open targets against the event's baseline (no new generation)

| Argument | Meaning |
|---|---|
| `<ID>` | Event id; omit to sweep every event |
| `--max-files` |  |

## `harna parity bulk`

Resolve several targets of one event in one go

| Argument | Meaning |
|---|---|
| `<ID>` | Event id |
| `--status` | Status applied to every listed target: ack \| noop \| auto |
| `--note` | Note applied to every listed target |
| `--paths` | File with module paths, one per line; omit to read paths from stdin |
| `--all-open` | Resolve every currently-open target of the event (ignores --paths) |
