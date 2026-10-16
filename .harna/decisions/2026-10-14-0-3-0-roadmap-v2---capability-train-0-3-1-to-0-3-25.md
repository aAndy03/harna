# 0.3.0+: roadmap v2 — the 0.3.x capability train, 25 versions

supersedes: 2026-10-14-0-3-0-roadmap---the-0-3-x-capability-train.md

Plan for 0.3.1 .. 0.3.25 (the complete itemized version lives in CHANGELOG,
"Planned — 0.3.1 .. 0.3.25 (v2)"). v1 (27 versions) is superseded: same
content, sharper shape. Four merges where one decision serves two items:
rule severity becomes registry data (0.3.1); env-scoped validate and
pattern rules pair as "policy as data" (0.3.3); encapsulation and
declared surface are one check from two sides (0.3.4); metrics and the
health score merge (0.3.12), cycles merge into engine placement (0.3.6).
One pull-up: the persistence boundary + gateways (+ registries,
transiter-thin) move from the tail to 0.3.10 — the
layer-3-cannot-write-to-the-base scenario is core, and its prerequisites
(encapsulation, placement checks) are done by then. Strict layering
(0.3.9) lands immediately before it, so `no-bypass` and the gateway check
compose. Scale still starts after measurement; hardening (migrate --dry,
backup/restore, path robustness) gets its own slot; layers-as-data stays
the capstone (0.3.25).

Process unchanged: one atomic commit per version (code + manifest +
decision + CHANGELOG), parity cycle closed, doctor/validate clean, tag,
publish. Backend only; additive serde defaults throughout. Rule pattern
stays the 0.3.0 pattern — seeded rules bound by `check = "<id>"` against
the builtin registry.
