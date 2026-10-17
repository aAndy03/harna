# 0.3.26-0.3.31 capability train agreed - terms, born-modular, dead-ends, roadmap, preflight gate, intent reconciliation, seams

Source: user review session, 2026-10-17. The 0.3.26-0.3.31 capability train is AGREED (all steps), driven by ten real-life usage cases reported by the user. Anchors the product stance: harna guides and guards the agent, never strips agentic capability; state stays in .harna/ as data.

Steps:
- 0.3.26 terms and protocol + BORN-MODULAR: .harna/terms.toml glossary (agent vocabulary -> harna vocabulary, doctor-checked against schema vocabulary), agents.md autonomous-ritual emission (plan -> scaffold -> implement -> scan -> parity open -> resolve -> validate --changed -> handoff -> atomic commit), born-size birth-budget check in budgets.rs (NEW files over the bound rule limit = ERROR at birth, no grandfathering; existing files stay advisory via hotspot + component-granularity). Rule born-modular persisted same session via add-rule.
- 0.3.27 decline and dead-ends: non-goals (product refusals, distinct from rules.toml code conventions) + lessons.toml (tried-and-failed with verdicts); place/plan emit a prior-attempts block; plan suppresses proposals violating active non-goals.
- 0.3.28 roadmap: .harna/roadmap.toml as DECLARED data, never generated; harna makes it load-bearing — onboarding+status show the current milestone, validate checks milestone/pointer coherence, deferred review triggers can key to milestones.
- 0.3.29 preflight: harna preflight release gate composing parity debts, overdue deferrals, digest age, declared min_score, refreshed release-note draft (notes reused as-is).
- 0.3.30 truthfulness: validate-intent reconciles the declared action/parity set against the real git diff and digest diff; files changed but undeclared and acks without substantiating diffs become findings.
- 0.3.31 seams: rename linkage kind in parity records (delete A + add B with near-identical digest maps), advisory single-ledger lock in .harna/, harna decompose (adopt-style split proposal, never writes) applied to harna itself per the hotspot advisories on store.rs/digest.rs/tools.rs/main.rs.

Constraints: every step stays inside the existing four-layer shape {schema, search, enforcement, generation, composition}; no new layers, no sideways imports; each new vocabulary ships advisory-first and only escalates to enforcement after observed adoption. Archived-decisions viewer and the language-plugin frontier stay at their 0.4.0 deferred review triggers — do not collect early.

Honest boundary declared for the 1M-user-product case: harna audits plans and gates releases; it never touches production systems, deploy pipelines or user data stores directly. A read-only surface.toml (what is actually live: environments, APIs, data owners) is the 0.4-tier candidate.
