# 0.3.48: rules carry their proof; external crates get gateways; cycles as feedback edges

Context: R1-R5 and H12 of the codex/zed/ruff study.

Decisions:
- R2 extends the gateway facility vocabulary (crate:NAME) instead of a new check: owning an external crate is the same idea as owning fs. Parsing lives on the Manifest (memory layer) so validate and harna deny (construction) both read it without a sideways import.
- R3 adds Rule.command rather than reusing pattern: a command is not a regex, and the field reads plainly in rules.toml. harna still executes nothing; its generated CI workflow runs the commands (non-goal no-code-execution kept).
- R4/R5 live in a new validate group artifacts/ because hygiene/ and coherence/ were at the 8-file folder budget.
- R5 judges a snapshot by its module prefix only: names are built at run time (ruff's format!), so a name-based check would be noise. Proven 0 false positives on codex and ruff after fixing three misreads.
- H12 is greedy, not minimal (minimum feedback arc set is NP-hard); each edge is named with the rings it closes.
- atomic::write retries a PermissionDenied rename on Windows (bounded, ~0.5s) - the rare suite flake was a real concurrent-writer bug.

No-test-update notes: gateway.rs/deny.rs/manifest.rs/crates.rs changes are exercised by gateway/crates/tests.rs; agents.rs proof line by the hooks proofs test path; atomic retry by the existing concurrent-writers test; mcp requests by the catalog-budget protocol test.
