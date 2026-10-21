# The `.harna/` data format

`.harna/` is the protocol between harna and a repository. The code reads
it, and git versions it next to the code it describes. This page lists every
file harna reads or writes, what it holds, and how it is versioned.

**The format is frozen at 1.0, at schema 6.** From here on:

- every file written by harna 1.0 reads with every later 1.x release;
- a change to the format is a new schema version, shipped with the
  migration `harna migrate` performs, a line in this page, and a fixture
  of the release before it (`crates/harna-core/tests/formats.rs` fails
  otherwise);
- a field is never removed or given a new meaning within 1.x; new fields
  are optional, with defaults.

Plugin packs follow [their own interface](PLUGINS.md), also stable at 1.0.

## Versioning rules

- **Stamped by need.** A TOML file carries `schema_version`, the lowest
  version that can hold what it contains. Current schema: **7**.
  - **1** is the baseline.
  - **2** (0.4.9): isolated layers, slots, test kinds, rules scoped by
    meaning.
  - **3** (0.5.9): a quality's `held_by`, a test kind's `contains`, a
    plugin capability in `owns`.
  - **4** (0.7.0): owners on layers and containers.
  - **5** (0.9.0): corridors and readiness in `services.toml`.
  - **6** (1.0): a corridor message's `planned` mark.
  - **7** (1.1.5): a component's `states` and `variants` (the UI
    catalog). A manifest that declares none stays at its earlier stamp.
- **Where the stamp is written.** The single-file registers always carry
  their stamp, written at least 1. Parity events and contracts are one
  file per item and leave the stamp out at the baseline.
- **Older data reads as is.** A field added later has a default, so a file
  written by any harna since 0.3.0 still reads.
- **`harna migrate`** restamps every versioned file to what it needs.
  - It backs up each file it changes in `.harna/backup/`, and verifies each
    backup reads before touching the file.
  - It logs the run as a decision.
  - It changes stamps, never content. The test asserts this for every
    fixture.
- **Previewing and gating.** `harna migrate --dry` previews the run.
  `harna migrate --check` writes nothing and fails when a file needs
  migrating, does not read, or holds a key harna does not know; it is
  meant for CI. `harna restore <file>` puts the newest backup back.
- **Newer data is refused.** Every reader refuses a `schema_version` above
  the one it understands instead of misreading it: upgrade harna.
  - This covers every register, `handoff.toml`, parity events, contracts,
    and plugin rules and terms.
  - `harna doctor` runs the same reads, so a register that a lenient
    briefing showed as empty is named there.
- **Unknown keys are named.** A key the model does not know (a typo like
  `rationle`, or a field from a newer harna) would be dropped by the next
  save. `harna doctor` and `migrate --check` name it first.
- **Unknown entries are named.** `harna doctor` warns about a file or folder
  in `.harna/` that harna does not read, such as `qualites.toml`. A file
  added by a newer release is unknown to an older one: a harna before
  1.1.2 names `assumptions.toml` and `vision.md` as unknown. Keep them,
  and upgrade harna instead of deleting them.
- **Words from a fixed set are checked.** `harna doctor` fails on a value
  outside its set, because harna would read it as the default:
  - a rule's `status`: `active` or `deprecated`;
  - a command's `runs`: `local`, `ci` or `both`;
  - a parity target's `status`;
  - an assumption's `state`: `open`, `confirmed` or `corrected` (an
    unknown state is read as open, so a typo never settles a question).
- **Regenerable files are not migrated.** The code map (`digest/`) is
  rebuilt by `harna scan`. It carries an `extractor_version`, and
  preflight asks for a rescan when that version is older.
- **harna-written files are canonical.** A save writes keys in model order
  and drops empty values. Only the leading comment header of
  `manifest.toml` and `rules.toml` survives a save.
- **Tested.** `crates/harna-core/tests/formats.rs` reads, migrates and
  re-reads harna's own `.harna/` from 0.3.0, 0.3.50, 0.5.9 and 0.7.7, a
  0.8.4 repository with services, a contract, a plugin, a score and a
  handoff, and a 1.0 repository with every register.
  It also feeds data from the future and a typo'd key, and checks
  that both are named.

## Files

| Entry | Since | Holds | Written by |
|---|---|---|---|
| `manifest.toml` | 0.1 | Layers (`depends_on`, `isolated`, owners), their containers (path, role, `gateway`, owns, owners, components with `provides`, tests and summaries), planned slots, `[[test_kinds]]`. | `harna init`, `layer`/`container`/`slot`/`test-kind` edits, `register`, `scaffold`, `adopt`, `move` |
| `rules.toml` | 0.1 | `[[rules]]`: id, text, rationale, scope or `scope_signature`, an optional bound `check` with its `limit`/`values`/`pattern`/`severity`/`command`, `env`, `status`, `origin`. `tags` and `examples` are reserved: read and kept, not used yet. | `harna add-rule`, `deprecate-rule`, profiles |
| `decisions/` | 0.1 | One markdown file per decision, named `YYYY-MM-DD-<slug>[_N].md` (UTC date): `# title`, then prose. A line starting `supersedes: <file>` (any case) chains it to an older one. Not versioned: prose. | `harna log-decision`, `migrate` |
| `parity/` | 0.2 | One TOML file per parity event: id, kind, source, generation, `baseline_ref`, `created`/`updated` (UTC dates), and targets. A target has a path and reason, a status (`open`, `ack`, `noop`, `auto`, `origin`), and optionally a note, `by`, `when` (RFC 3339 UTC since 0.9.0; older values stay), `breaking` and `history`. | `harna parity open/resolve/bulk/auto` |
| `digest/` | 0.3.15 | The code map: `index.json` (modules, body hashes, `extractor_version`, `generated_at` RFC 3339) and `shards/<layer>-<container>.json`. Regenerable. | `harna scan` |
| `digest.json` | 0.1 | The map's single-file form from before 0.3.15. Still read when `digest/` is absent. | older harna |
| `scores/` | 0.3.13 | Score snapshots, one TOML per recording. Every part defaults, so a part added later reads in old snapshots. Values computed by different releases are not comparable. | `harna metrics --record` |
| `archive/` | 0.3.18 | Filed state, one folder per kind: closed parity events, pruned scores, superseded decisions. A name clash keeps both files (`_N`). | `harna archive` |
| `templates.toml` | 0.3.22 | `[[templates]]`: a change type's fan-out ceiling (`max_targets`) over one `scope` path, with a rationale. | `harna templates add` |
| `deferred.toml` | 0.3.23 | `[[entries]]`: deliberate deferrals (id, component, reason) with a `review` trigger, read as follows: <br>• `date:2027-01-15`, or a bare ISO date. <br>• `version:1.2.0` (the **project's** version, from the root `Cargo.toml` or `package.json`). <br>• `milestone:<id>`. <br>A bare version-shaped value is that milestone when the roadmap declares it, and a version otherwise; any other bare word is a milestone id. | `harna deferred add` |
| `terms.toml` | 0.3.26 | `[[terms]]`: a word, what it means in harna, its aliases. | `harna terms add` |
| `non-goals.toml` | 0.3.27 | `[[goals]]`: deliberate product refusals (id, refusal, reason). | `harna non-goal add` |
| `lessons.toml` | 0.3.27 | `[[lessons]]`: tried and failed (id, claim, verdict, evidence). | `harna lesson add` |
| `assumptions.toml` | 1.1.2 | `[[assumptions]]`: what the agent assumed (id, `statement`), the `question` that settles it, the `default` the code follows meanwhile, the `impact`, the milestone it is due `by`, `blocking`, `state` (`open`/`confirmed`/`corrected`, absent is open), the `answer`, `recorded` (date). A new file: older harna releases do not read it. | `harna assume add/confirm/correct` |
| `approvals.toml` | 1.1.4 | `[[approvals]]`: what the owner approved — `id` (a component, a change, or `dep:<name>` for a dependency), the `paths` it freezes (paths or globs), the content `hash` of the files they matched (`fnv1a64:<16 hex>`: FNV-1a 64 over each tracked file in path order — its path, a NUL byte, its body with CRLF read as LF, a newline), `version` (bumped on each re-approval: `calendar@3`), `by`, `date`, `note`, `screenshot`. `[[require]]`: a change that needs an approval first — `kind` (`dependency-added` or `path`), `paths`, `reason`. A new file: older harna releases do not read it. | `harna approve`, `harna approvals require` |
| `vision.md` | 1.1.2 | The owner's vision, as written: where the project should be after a long time. Never generated, never rewritten by harna. | `harna vision set` |
| `roadmap.toml` | 0.3.28 | `[[milestones]]`: id, title, state (`declared`/`current`/`done`/`dropped`), notes. | `harna roadmap` |
| `handoff.toml` | 0.3.30 | The open burst: summary, next steps, `created` (RFC 3339), `open_parity`. | `harna handoff` |
| `acknowledged.toml` | 0.3.31 | `[[entries]]`: accepted Info findings (check, file, reason). | `harna ack` |
| `baselines.toml` | 0.3.44 | `[[entries]]`: ratchets, allowed error counts per (check, file) that may only fall. | `harna baseline` |
| `commands.toml` | 0.3.46 | `[[commands]]`: name, the exact `run`, what a pass `proves`, `runs` (`local`/`ci`/`both`), scope. | `harna command add` |
| `qualities.toml` | 0.4.0 | `[[qualities]]`: id, statement, measure, category, scope, why, `recorded` (date), `held_by` rules. | `harna quality add` |
| `plugins/` | 0.5.0 | Vendored packs, one folder each: `plugin.toml` (name, version, the upstream it mirrors), its `rules.toml` and `terms.toml` (both version-checked), `api.toml`, capabilities and bridges. Applied as `<plugin>/<id>`. | `harna plugin add/remove` |
| `contracts/` | 0.7.2 | One TOML file per container: `container`, `version` (strict semver), `frozen` (date), and `items`, each provided name with its whole signature then. | `harna contract freeze` |
| `services.toml` | 0.8.0 | `[[services]]`: name, description, `containers` (run by this service alone), `owns` (resources as `kind:name`, compared case-insensitively), owners, `runs` (deploy folder), and a `ready` table (facet → answer). <br>`[[corridors]]`: name, kind, `from`, `to`, and `messages` (name, shape `<file>::<Type>` or a schema file, and `planned = true` when it was declared before its code exists). <br>`require`: the readiness facets. | `harna service …` |
| `backup/` | 0.3.24 | `<file>.v<n>` copies kept by `harna migrate` (git-ignored). `harna restore <file>` puts the newest back. | `harna migrate` |

## Spellings frozen as they are

The 0.9.0 audit listed some inconsistencies. Renaming them would cost
every repository a migration, so they stay as they are, and are written
down here so they surprise nobody:

- **Collection names.** `non-goals.toml` keeps its records under
  `[[goals]]`. The acknowledged, baselines and deferred ledgers use
  `[[entries]]`. Every other file uses its plural noun.
- **Keys whose meaning depends on the file.**
  - `owns` is a capability in the manifest and a resource in
    `services.toml`.
  - `runs` is where a command runs (`local`/`ci`/`both`) in
    `commands.toml`, and a deploy folder in `services.toml`.
  - "corridor" is a layer hand-off in the manifest (a transiter) and a
    channel between services in `services.toml` (glossary:
    `service-corridor`).
- **The reason for a record.** It is `rationale` in rules and templates,
  `reason` in ledgers, non-goals and parity targets, and `why` in
  qualities.
- **The identifying key.** A record has an `id`; a named thing (a layer,
  container, command, service or plugin) has a `name`; a glossary entry
  has a `word`. Ack and baseline entries are keyed by `(check, file)`.
- **Gateways are a comma-separated string.** `gateway = "fs:write,
  process"` is the one list kept as a string. A template's `scope` is one
  path.
- **Dates.** Days are `YYYY-MM-DD`; instants are RFC 3339 UTC. A rule's
  `origin` keeps its date in prose.
- **Owners.** A container's own owners win, then the owners of the service
  that runs it, then the layer's.

## Generated next to `.harna/`

Next to `.harna/`, harna also writes these views and wiring files. Each is
regenerated from the data above, and a hand-written file is never touched:

- `ARCHITECTURE.md` and `SURFACE.md`;
- the marked blocks in `AGENTS.md` and `CLAUDE.md`, also per container
  folder;
- `.github/CODEOWNERS`;
- `deny.toml` and `clippy.toml`;
- the hooks.
