# Agent instructions

This repository is harna — it manages itself with harna.

- **Source of truth is `.harna/`** (manifest.toml, rules.toml, decisions/, digest.json).
  Start every session by calling the `harna_onboarding` MCP tool.
- **After an edit burst**, call `harna_validate` with `changed_only: true`
  (or run `harna validate --changed`). Fix violations before committing.
- **Structure changes**: update `.harna/manifest.toml` in the same commit as the code,
  and call `harna_log_decision` for any non-obvious choice.
- **After a structural change burst**: `harna parity open <id>` to turn the diff into
  parity targets, then `harna_parity_resolve`/resolve each (ack or noop). Open debts
  gate validation.
- **New rules**: when the user states a preference or a convention is agreed, persist it
  immediately with `harna_add_rule` — instructions become data, not chat.
- **Containers never import each other sideways** in harna-core; composition may reach down.
  Layer map: `crates/harna-core/src/{schema,search,enforcement,generation,composition}`.
- Commit `.harna/` together with the code change it describes (atomic PRs).
