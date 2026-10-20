# 0.3.50: the toolbox is data; provides kept; stubs carry no fake coverage

Context: 0.3.50 makes harna's tools discoverable (rule tools-not-prescriptions). Proven by a fresh agent (household-harna-test) that adopted the crate gateway, --provides, container tests and layer set from harna tools without being told.

Decisions:
- The catalog is data (briefing/toolbox.toml). Suggestions fire on structural signals only (single-user external crate, cross-layer importers, missing surface, no rules, unregistered tests), so no framework or product name lives in code. A test keeps catalog checks and signals coherent with the code.
- provides is NOT deprecated, contrary to the earlier audit: it is the declared-surface contract, enforced since 0.3.x but never exposed. It gained container set --provides and a catalog entry. It is the seed of 0.6.0's versioned contracts.
- Transiters keep their mechanics and become corridors in wording; the user's layer-to-layer idea has a job at the interface<->backend boundary (0.4.1 command bus).
- sequence is opt-in: an order nobody declared is ceremony.
- Scaffold stubs no longer register a placeholder test as coverage: registered tests should mean real tests (the stale fake caused 8 removals in the field test).
- hotspot requires 5 commits under the statistical threshold: churn is repeated change. An explicit rule limit is honored as written.
- New containers follow the layer's crate; a crate-less layer in a workspace asks for --path rather than inventing a root src/ tree.
- Larger field findings are deferred to their milestones rather than patched: symbol-level parity (0.6.0), surface drafted from pub use, intent-aware placement and dev-dependency layering (0.4.0).
