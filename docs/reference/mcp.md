# MCP reference

Generated from the server's own `tools/list` and `resources/list` (`harna-mcp`) — do not edit.

49 tools and 10 resources. Wire the server with one config line (`harna-mcp`, run in the repository); `HARNA_TOOLS=core` lists the core set only.

## Tools

### `harna_acknowledge`

Advisory ledger: add (check, file, reason) accepts an Info finding; remove; list. Not for errors.

| Parameter | Type | Meaning |
|---|---|---|
| `action` (required) | string | "add" \| "remove" \| "list". |
| `check` | string | The check that fired, e.g. "no-copy-paste" (add/remove). |
| `file` | string | The repo-relative file it fired on (add/remove). |
| `reason` | string | Why the advisory is accepted rather than fixed (add). |

### `harna_adapt`

Alias of harna_adopt (kept for existing configs).

### `harna_add_rule`

Persist a rule the user stated; optionally bound to a check (harna checks).

| Parameter | Type | Meaning |
|---|---|---|
| `check` | string | Check id to bind (violations cite the rule). |
| `command` | string | CI proof command. |
| `env` | string | "dev" \| "prod"…; omit for all. |
| `id` (required) | string | Stable id. |
| `limit` | integer | Budget for a budget check. |
| `pattern` | string | Regex over file paths and declared names. |
| `rationale` (required) | string | Why the rule exists. |
| `scope` | array | Paths; empty = global. |
| `scope_signature` | string | Regex over declarations. |
| `severity` | string | "error" \| "info", overriding the bound check's default. |
| `text` (required) | string | The rule in imperative form. |
| `values` | array | List args (banned paths => replacement, sync files). |

### `harna_adopt`

Draft a manifest from the tree (by_dir, exclude); re-run refreshes an adopted repo.

| Parameter | Type | Meaning |
|---|---|---|
| `by_dir` | boolean |  |
| `exclude` | array | Globs to leave out. |

### `harna_archive`

Archive closed parity events, superseded decisions, old scores (keep newest N). Deletes nothing.

| Parameter | Type | Meaning |
|---|---|---|
| `keep` | integer | Score snapshots kept live (newest kept first; the rest archive). |

### `harna_baseline`

Ratchet ledger: 'record' freezes today's errors per check+file, 'tighten' lowers them, 'show'.

| Parameter | Type | Meaning |
|---|---|---|
| `action` (required) | string | record \| tighten \| show |
| `check` | string | record: only this check. |

### `harna_command`

The repo's commands: 'add' (name, run, proves, runs local|ci|both, scope), 'remove', 'list'.

| Parameter | Type | Meaning |
|---|---|---|
| `action` (required) | string | add \| remove \| list |
| `name` | string |  |
| `proves` | string | What a pass proves. |
| `run` | string | The exact invocation. |
| `runs` | string | local \| ci \| both |
| `scope` | array |  |

### `harna_decompose`

How to split an oversized file (read-only plan + manifest sketch).

| Parameter | Type | Meaning |
|---|---|---|
| `file` (required) | string | Repo-relative file to plan, e.g. "crates/x/src/main.rs". |

### `harna_deferred_add`

Deliberate deferral: component, reason, review (ISO date or version). Same id replaces.

| Parameter | Type | Meaning |
|---|---|---|
| `component` (required) | string | What it would touch: component, module or container path. |
| `id` (required) | string | Stable id, e.g. fixture-builder-dedup. |
| `reason` (required) | string | Why it is deferred. |
| `review` (required) | string | When to revisit: ISO date or version, e.g. "0.4.0". |

### `harna_deferred_list`

List deferrals; past-review entries are flagged OVERDUE.

### `harna_deny`

Write the layer topology into deny.toml so `cargo deny check bans` fails cross-layer crate deps.

### `harna_dependents`

Who imports a module (ring 1 with the edge, ring 2 knock-on) and their containers. Ask before changing it.

| Parameter | Type | Meaning |
|---|---|---|
| `module` (required) | string | Digest module path, crate-relative stem, or file name. |

### `harna_diff`

Structural diff of the code map vs a git ref: modules, pub signatures, internal deps added/removed.

| Parameter | Type | Meaning |
|---|---|---|
| `from` | string | Git ref whose .harna/digest.json is the baseline. Defaults to HEAD. |
| `max_files` | integer | Max files to scan (a safety cap). |

### `harna_doctor`

Installation health: schema, data, map freshness, git, commit gate, agent wiring. FAIL items first.

### `harna_graph`

Dependency graph of the code map, clustered by layer/container. format = 'mermaid' (default) or 'dot'.

| Parameter | Type | Meaning |
|---|---|---|
| `format` | string | Output format: "dot" \| "mermaid" (default mermaid). |

### `harna_handoff`

Before compaction: record summary + next_steps. clear=true when resumed; no args reads it.

| Parameter | Type | Meaning |
|---|---|---|
| `clear` | boolean | The burst is resumed: remove the recorded handoff. |
| `next_steps` | array | What the next session should pick up, in order. |
| `summary` | string | What this burst did (records; overwrites). Omit to read. |

### `harna_hooks`

Hooks: 'status'; 'install' writes git pre-commit, Claude Code hooks, CI (no overwrite without force).

| Parameter | Type | Meaning |
|---|---|---|
| `action` (required) | string | "status" \| "install". |
| `bin` | string | Command the Claude Code hooks run (default "harna"). |
| `craft` | boolean | Add Rust craft gates (fmt, clippy, tests) to git and CI. |
| `force` | boolean | Replace files not written by harna. |
| `targets` | array | Targets to install: git \| claude \| ci (default all). |

### `harna_lesson`

Dead ends: 'add' (id, claim, verdict, evidence) so plan/place warn before re-suggesting, 'remove', 'list'.

| Parameter | Type | Meaning |
|---|---|---|
| `action` (required) | string | "add" \| "remove" \| "list". |
| `claim` | string | What was tried (add). |
| `evidence` | array | Evidence pointers: commits, files, issues (add, optional). |
| `id` | string | Stable id (add/remove). |
| `verdict` | string | Why it failed (add). |

### `harna_log_decision`

Record a decision (why it was built this way). supersedes = the older decision file it replaces.

| Parameter | Type | Meaning |
|---|---|---|
| `body` (required) | string | The decision body: context, choice, consequences. |
| `supersedes` | string | File name in .harna/decisions/ of the decision this replaces. |
| `title` (required) | string | Short title of the decision. |

### `harna_manifest_edit`

Manifest edits: add (new_name=layer, path), register, unregister, rename, container (path, gateway, role, owns, provides), layer, slot, test-kind, surface, contract (new_name=version). "" clears.

| Parameter | Type | Meaning |
|---|---|---|
| `action` (required) | string |  |
| `depends_on` | string | Comma-separated layers (test-kind: its reach). |
| `description` | string |  |
| `escapes` | string |  |
| `gateway` | string | fs \| fs:write \| net \| process \| base \| crate:x \| npm:x |
| `isolated` | boolean |  |
| `name` (required) | string | Container or layer. |
| `new_name` | string |  |
| `owners` | string |  |
| `owns` | string |  |
| `path` | string | test-kind: comma-separated globs (none removes it). |
| `provides` | string |  |
| `role` | string | Role; for test-kind, its file marker (regex). |

### `harna_migrate`

Upgrade .harna/ data to this harna's schema (backups first, logged). dry=true previews.

| Parameter | Type | Meaning |
|---|---|---|
| `dry` | boolean | Preview only. |

### `harna_move`

Move a file or folder; manifest paths, tests, rule scopes, mod declarations and use-paths follow.

| Parameter | Type | Meaning |
|---|---|---|
| `from` (required) | string | Repo-relative source file or folder. |
| `to` (required) | string | Repo-relative destination. |

### `harna_non_goal`

Product refusals: 'add' (id, refusal, reason; plan/place suppress matching ideas), 'remove', 'list'.

| Parameter | Type | Meaning |
|---|---|---|
| `action` (required) | string | "add" \| "remove" \| "list". |
| `id` | string | Stable id, e.g. "no-remote-state" (add/remove). |
| `reason` | string | Why it was refused (add). |
| `refusal` | string | What is refused, imperative (add). |

### `harna_notes`

Draft notes: kind='review' (parity id or diff ref) or 'release' (since from).

| Parameter | Type | Meaning |
|---|---|---|
| `from` | string | Git ref, default HEAD. |
| `id` | string | Parity event id (review); omit to diff from `from`. |
| `kind` (required) | string | "review" \| "release". |

### `harna_onboarding`

Session start: architecture, rules, qualities, glossary, roadmap, handoff, code map. Call first.

### `harna_parity`

Show parity events and their targets; open_only=true for the open debts that gate validate.

| Parameter | Type | Meaning |
|---|---|---|
| `id` | string | Optional event id; omit to show all events. |
| `open_only` | boolean | Only events with unresolved targets (debt view). |

### `harna_parity_auto`

Auto-verify open parity targets whose surface is unchanged vs the baseline. id = one event; omit = all.

| Parameter | Type | Meaning |
|---|---|---|
| `id` | string | Event id to re-verify; omit to sweep every event. |

### `harna_parity_open`

Before committing a structural change: open a parity event (modules that must follow the diff).

| Parameter | Type | Meaning |
|---|---|---|
| `from` | string | Git ref whose committed digest is the baseline. Defaults to HEAD. |
| `id` (required) | string | Event id, e.g. "change-store-api"; reuse = revise. |
| `kind` | string | Event kind (rule-change, manual, digest-diff...). |

### `harna_parity_resolve`

Resolve one parity target: 'ack' (the change landed there) or 'noop' (nothing needed; the note says why).

| Parameter | Type | Meaning |
|---|---|---|
| `id` (required) | string | Event id. |
| `note` | string | Why / what was done. |
| `path` (required) | string | Module path of the target (as listed in the event). |
| `status` (required) | string | "ack" (change landed here) \| "noop" (nothing needed, say why). |

### `harna_place`

Before code: where an idea lives, decisions, rules, expected fan-out, test obligations, refusals, dead ends.

| Parameter | Type | Meaning |
|---|---|---|
| `idea` (required) | string | The idea in plain words, e.g. "timezone picker data". |

### `harna_plugin`

Framework plugins (gpui-kit 0.8.0, …): list, show, add (install/upgrade into .harna/plugins), remove.

| Parameter | Type | Meaning |
|---|---|---|
| `action` (required) | string | list \| show \| add \| remove |
| `name` | string | Plugin name or folder path. |

### `harna_preflight`

Release gate: validation + doctor + open parity + map freshness → PASS/FAIL. Does not run the test suite.

| Parameter | Type | Meaning |
|---|---|---|
| `max_files` | integer | Max files to check (a safety cap). |

### `harna_quality`

Long-term qualities (scale, latency, offline, privacy): add, remove, list.

| Parameter | Type | Meaning |
|---|---|---|
| `action` (required) | string | "add" \| "remove" \| "list". |
| `category` | string |  |
| `held_by` | array |  |
| `id` | string |  |
| `measure` | string |  |
| `scope` | array |  |
| `statement` | string |  |
| `why` | string |  |

### `harna_render`

Regenerate ARCHITECTURE.md and SURFACE.md. check = CI staleness gate; surface = SURFACE.md only.

| Parameter | Type | Meaning |
|---|---|---|
| `check` | boolean | Verify the committed views are current instead of writing. |
| `surface` | boolean | Only SURFACE.md. |

### `harna_restore`

Restore the newest verified backup of 'manifest.toml' or 'rules.toml' over the live file.

| Parameter | Type | Meaning |
|---|---|---|
| `name` (required) | string | Data-file name: 'manifest.toml' \| 'rules.toml' (no path separators). |

### `harna_roadmap`

Milestones: 'add' (id, title, state), 'set' (declared|current|done|dropped), 'remove', 'list'.

| Parameter | Type | Meaning |
|---|---|---|
| `action` (required) | string | "add" \| "set" \| "remove" \| "list". |
| `id` | string | Milestone id, e.g. "0.3.33" (add/set/remove). |
| `notes` | string | Optional longer goal (add). |
| `state` | string | declared \| current \| done \| dropped (add defaults to declared; set requires it). |
| `title` | string | What it delivers, one line (add). |

### `harna_rule_lifecycle`

Rule lifecycle: 'deprecate' (visibly dead, give a reason), 'activate' (undo), 'remove' (git keeps history).

| Parameter | Type | Meaning |
|---|---|---|
| `action` (required) | string | "deprecate" (visibly dead; add the reason) \| "activate" (restore) \| "remove". |
| `id` (required) | string | Rule id. |
| `reason` | string | Why the rule is being retired (appended to its rationale). |

### `harna_rules`

Rules (with rationale and origin), optionally for one scope path, e.g. 'src/engine'.

| Parameter | Type | Meaning |
|---|---|---|
| `env` | string | Session environment ("dev", "prod"…); hides other envs' rules. |
| `scope` | string | Optional scope path to filter rules by, e.g. "src/engine". |

### `harna_scaffold`

New component (primitive | compound | data): file, manifest entry, module wiring; new_crate creates its crate.

| Parameter | Type | Meaning |
|---|---|---|
| `container` (required) | string | Container (created if missing). |
| `description` | string | One line. |
| `empty` | boolean |  |
| `kind` (required) | string | "primitive" (file) \| "compound" (folder) \| "data" (toml + loader). |
| `layer` (required) | string | Layer (created if missing). |
| `name` (required) | string | kebab-case. |
| `new_crate` | string | lib \| bin |
| `path` | string | Default <container path>/<name>. |
| `role` | string | Container role. |

### `harna_scaffold_transiter`

Create the transiter that connects two layers (engines never call each other directly).

| Parameter | Type | Meaning |
|---|---|---|
| `from` (required) | string | From layer. |
| `name` | string | Custom name. |
| `to` (required) | string | To layer. |

### `harna_scan`

Rebuild the code map (.harna/digest/), incremental unless full=true. Run after structural changes.

| Parameter | Type | Meaning |
|---|---|---|
| `full` | boolean | Re-parse every respected file instead of reusing unchanged modules. |
| `max_files` | integer | Max files to scan (a safety cap). |

### `harna_service`

Services: add/set/remove/list; corridor/uncorridor (from, to, messages Name=file::Type); require/ready (facet, answer).

| Parameter | Type | Meaning |
|---|---|---|
| `action` (required) | string |  |
| `answer` | string |  |
| `containers` | array |  |
| `description` | string |  |
| `facet` | string |  |
| `from` | array |  |
| `kind` | string |  |
| `messages` | array |  |
| `name` | string |  |
| `owners` | array |  |
| `owns` | array |  |
| `runs` | string |  |
| `to` | array |  |

### `harna_status`

One-line health: map, manifest, tests, score, rules, parity, deferrals, roadmap NOW, handoff.

### `harna_template_add`

Parity template: a change type's fan-out ceiling (max_targets) over a scope. Same id replaces.

| Parameter | Type | Meaning |
|---|---|---|
| `id` (required) | string | Stable id, e.g. schema-shard-change. |
| `max_targets` (required) | integer | The fan-out ceiling an event over this scope shouldn't walk past. |
| `rationale` | string | Why this shape is declared. |
| `scope` | string | Module-path prefix this change type touches (empty = everywhere). |

### `harna_template_list`

List parity templates: id, scope, ceiling, rationale.

### `harna_terms`

Glossary: 'add' (word, means, aliases; warns on collisions), 'remove', 'list'.

| Parameter | Type | Meaning |
|---|---|---|
| `action` (required) | string | "add" \| "remove" \| "list". |
| `aliases` | array | Agent vocabulary that maps to it (add, optional). |
| `means` | string | Its meaning, as harna defines it (add). |
| `word` | string | harna's canonical word (add/remove). |

### `harna_validate`

Conformance check, verdict PASS/BLOCKED. changed_only after edits; strict makes infos block; env scopes rules.

| Parameter | Type | Meaning |
|---|---|---|
| `changed_only` | boolean | Only files with uncommitted changes (use after edits). |
| `env` | string | Session environment; other envs' rules are not enforced. |
| `max_files` | integer | Max files to check (a safety cap). |
| `strict` | boolean | Info findings block too (default: only errors block). |

### `harna_validate_intent`

Claims vs state: package version, changelog head, roadmap, parity silence. Each DRIFT names both sides.

### `harna_whereis`

Where a symbol (name or module::Symbol) is defined, and which modules reach it.

| Parameter | Type | Meaning |
|---|---|---|
| `symbol` (required) | string | Symbol name, optionally module-qualified ("module::Symbol"); a trailing "()" is tolerated. |

## Resources

| URI | What it holds |
|---|---|
| `harna://digest.json` | compressed code map: signatures, inventory, deps (read-only) |
| `harna://lessons.toml` | recorded dead ends: tried-and-failed with verdicts and evidence (read-only) |
| `harna://manifest.toml` | architecture: layers -> containers -> components (read-only) |
| `harna://non-goals.toml` | deliberate product refusals the agent must not propose around (read-only) |
| `harna://qualities.toml` | long-term intent: what the software must hold (scale, latency, offline, data loss, privacy) (read-only) |
| `harna://roadmap.toml` | declared milestones with states — the project's path, never generated (read-only) |
| `harna://rules.toml` | coding rules with rationale, scope, origin (read-only) |
| `harna://services.toml` | services: the containers each runs alone and the resources each alone writes (read-only) |
| `harna://terms.toml` | glossary: agent vocabulary -> harna vocabulary, with meanings (read-only) |
| `harna://tools` | harna's toolbox: what each tool does, when to reach for it, how (all optional) |
