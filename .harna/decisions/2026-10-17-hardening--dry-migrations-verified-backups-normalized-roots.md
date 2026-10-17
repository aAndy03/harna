# Hardening: dry migrations, verified backups, normalized roots (0.3.24)

`harna migrate --dry` (CLI flag; MCP `harna_migrate(dry=true)`) is the
preview without writes: each schema-bearing file that would move reports
`WOULD migrate schema vN -> vM`, naming BOTH consequences the real run
will produce — the backup kept in `.harna/backup/` AND the decision entry
that will be recorded — and ends with `nothing written`; a dry run
pinned by test touches no decision file and creates no backup shelf.
Backups became **verified** (0.3.24's honest list says backup/restore
verification): after the copy, the backup is read back fully, checked
non-empty and parsed as TOML BEFORE the transform touches the live file —
a run whose backup fails the read-back aborts before any write; the same
`verify_backup` gate guards restore. `harna restore <name>` (+ MCP
`harna_restore`, tool 31 — count 31 in the manifest) rolls the newest
`.harna/backup/<name>.vN` back over the live file, refusing first on a
name with path separators or `..` (the shelf it reads is fixed, no path
guessing), and refusing on a backup that does not parse — pinned with the
corrupt-after-migration fixtures (`not [ valid toml`, then blank): the
live file survives a refused restore, which is the whole point of
restore. The swap stages into a temp sibling and renames, so a
half-copied restore never becomes the truth. Root paths normalized in
`Store::open` (0.3.24's path robustness): `canonicalize` when the root
exists, lexical dot-cleanup when it does not (the tool must open repos
pre-init); canonicalize's Windows verbatim form is first REDUCED back to
the plain readable form so display/tests/messages keep comparing
straightforward strings, then re-applied only past `LONG_PATH_PREFIX_AT`
(240 chars) so deep application paths ride `\\?\` through the store's fs
calls instead of failing at classic MAX_PATH; relative `..` heads
survive the fallback (dropping them would silently change the path's
meaning — pinned). No automatic restore-on-anything: restore is an
explicit command over the shelf you already have, and git remains the
primary undo (data tracked, diff the review). Dogfood: migrate/restore
exercise their own fixtures, 217/217 tests (+3), Phase F opened.
