# harna manages itself with harna + git

.harna/ (manifest, rules, decisions, digest) is committed and evolves with the code. pre-commit runs 'harna validate --changed' (exit 2 on violations gates the commit). Rationale: enforcement must be deterministic, not an agent promise, and must run on harna first.
