# The `.harna/` data format

`.harna/` is the protocol between harna and a repository. The code reads
it, and git versions it next to the code it describes. This page lists every
file harna reads or writes, what it holds, and how it is versioned.

## Versioning rules (0.9.0 freeze candidate)

- **Stamped by need.** A TOML file carries `schema_version`, the lowest
  version that can hold what it contains. A file that needs nothing newer
  than the baseline (1) is stamped 1. For parity events and contracts, the
  baseline is written as no stamp at all. Current schema: 4.
- **Older data reads as is.** A field added later has a default, so a file
  written by any harna since 0.3.0 still reads. `harna migrate` restamps
  `manifest.toml` and `rules.toml` and backs up each file it changes in
  `.harna/backup/` (each backup is verified readable first).
  `harna migrate --dry` previews the run.
  - The test `crates/harna-core/tests/formats.rs` reads and migrates
    harna's own `.harna/` from 0.3.0, 0.3.50, 0.5.9 and 0.7.7.
- **Newer data is refused.** Every reader refuses a `schema_version` above
  the one it understands instead of misreading it: upgrade harna.
- **Regenerable files are not migrated.** The code map (`digest/`) is
  rebuilt by `harna scan`. It carries an `extractor_version`, and
  preflight asks for a rescan when that version is older.
- **Unknown entries are named.** `harna doctor` warns about a file or folder
  in `.harna/` that harna does not read, such as a typo like
  `qualites.toml`.

## Files

| Entry | Since | Holds | Written by |
|---|---|---|---|
| `manifest.toml` | 0.1 | Layers (`depends_on`, `isolated`, owners), their containers (path, role, gateway, owns, owners, components with `provides`, tests and summaries), planned slots, `[[test_kinds]]`. | `harna init`, `layer`/`container`/`slot`/`test-kind` edits, `register`, `scaffold`, `adopt`, `move` |
| `rules.toml` | 0.1 | Rules: id, text, rationale, scope or `scope_signature`, an optional bound `check` with its `limit`/`values`/`pattern`/`severity`/`command`, `env`, `deprecated`. | `harna add-rule`, `deprecate-rule`, profiles |
| `decisions/` | 0.1 | One markdown file per decision: `# title`, then prose. A `Supersedes:` line chains it to an older one. Not versioned: prose. | `harna log-decision`, `migrate` |
| `parity/` | 0.2 | One TOML file per parity event: id, kind, source, generation, `baseline_ref`, and targets (path, reason, status `open`/`ack`/`noop`/`auto`, note). | `harna parity open/resolve/bulk/auto` |
| `digest/` | 0.3.15 | The code map: `index.json` (modules, body hashes, `extractor_version`) and `shards/<layer>-<container>.json`. Regenerable. | `harna scan` |
| `digest.json` | 0.1 | The map's single-file form from before 0.3.15. Still read when `digest/` is absent. | older harna |
| `scores/` | 0.3.13 | Score snapshots, one TOML per recording. | `harna score --record` |
| `archive/` | 0.3.18 | Filed state, one folder per kind: closed parity events, pruned scores, superseded decisions. | `harna archive` |
| `templates.toml` | 0.3.22 | Parity templates: a change type's fan-out ceiling over a scope. | `harna templates add` |
| `deferred.toml` | 0.3.23 | Deliberate deferrals with a review trigger (a date or a milestone). | `harna deferred add` |
| `terms.toml` | 0.3.26 | The glossary: a word, what it means in harna, its aliases. | `harna terms add` |
| `non-goals.toml` | 0.3.27 | Deliberate product refusals, with their reason. | `harna non-goal add` |
| `lessons.toml` | 0.3.27 | Tried and failed: claim, verdict, evidence. | `harna lesson add` |
| `roadmap.toml` | 0.3.28 | Milestones: id, title, state (`declared`/`current`/`done`/`dropped`), notes. | `harna roadmap` |
| `acknowledged.toml` | 0.3.31 | The advisory ledger: accepted Info findings, each with its reason. | `harna ack` |
| `baselines.toml` | 0.3.44 | Ratchets: allowed error counts that may only fall. | `harna baseline` |
| `commands.toml` | 0.3.46 | The repo's commands: what to run, what a pass proves, local/ci/both. | `harna command add` |
| `handoff.toml` | 0.3.30 | The open burst: summary and next steps for the next session. | `harna handoff` |
| `qualities.toml` | 0.4.0 | Long-term qualities: statement, measure, category, scope, why, `held_by` rules. | `harna quality add` |
| `plugins/` | 0.5.0 | Vendored packs, one folder each: `plugin.toml` (name, version, the upstream it mirrors), its `rules.toml`, `terms.toml`, `api.toml`, capabilities and bridges. Applied as `<plugin>/<id>`. | `harna plugin add/remove` |
| `contracts/` | 0.7.2 | One TOML file per container: the version and each provided name with its signature then. | `harna contract freeze` |
| `services.toml` | 0.8.0 | Services (containers each runs alone, resources each owns, owners, deploy folder, readiness answers), corridors (kind, from, to, messages with shapes) and the required readiness facets. | `harna service …` |
| `backup/` | 0.3.24 | `<file>.v<n>` copies kept by `harna migrate`. `harna migrate --restore <file>` puts one back. | `harna migrate` |

Next to `.harna/`, harna also writes these views and wiring files. Each is
regenerated from the data above, and a hand-written file is never touched:
`ARCHITECTURE.md`, `SURFACE.md`, the marked blocks in `AGENTS.md` and
`CLAUDE.md` (also per container folder), `.github/CODEOWNERS`, `deny.toml`
and `clippy.toml`, and the hooks.
