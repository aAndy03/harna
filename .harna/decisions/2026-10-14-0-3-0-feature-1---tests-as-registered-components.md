# 0.3.0 feature 1: tests as registered components (`tests` field, test-coverage validator)

Components register their tests in the manifest (`tests = [...]`, optional,
serde-defaulted — no schema migration; a component without declarations owes
nothing). Two checks join the validate container as a new component
`validate/test_coverage.rs` (check ids `tests-declared`, `no-test-update` in
BUILTIN_CHECKS so rules can bind to them):

- **declared coherence** (whole repo, Error/Info): a registered test path
  exists, and a registered Rust test actually contains test code — the
  manifest never points at fiction.
- **no-test-update** (`validate --changed` only, Info): a changed component
  whose registered tests were untouched in the burst is advised. Detection:
  separate test files by burst membership; inline tests by comparing the
  file's test region (first `#[test]`/`#[cfg(test)]`-style marker to EOF)
  between the working tree and `HEAD`.

Why the changed-scope enforcement loop instead of the parity ledger: the
digest deliberately carves test code out of the shipped surface (it must not
appear as signatures), so a digest diff structurally cannot see test updates;
`contents_changed` (body-only edits) is documented as governed by
`validate --changed`, and this check is the same governance applied to a
component's test obligation. Severity stays Info: comments/refactors must not
gate commits; the agent extends the tests or records why in the decision log.
Dogfooding registered every harna-core component with inline tests
(`statuses.toml`, `cli-main`, `mcp-*` stay unregistered — no test sections to
honor). Two dead-code warnings fixed in passing (unused SKIP_DIRS const,
no-op drop in scaffold).
