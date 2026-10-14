# 0.3.0 feature front plan: session order

supersedes: 2026-10-14-plan-0-2-x-patch-train---record-0-3-0-as-future.md

0.3.0 ships as five ordered sessions, each one atomic code+data+decision commit: (1) tests-as-registered-components, (2) decision supersession with supersedes + chain rendering, (3) generated views (harna render -> ARCHITECTURE.md), (4) harna plan full briefing, (5) harna handoff burst journal. Version cranks once at the 0.3.0 ship (final feature + tag v0.3.0); baseline tag v0.3.0-baseline, last shipped feature commit f8903aa (0.2.12).

Session 2 no-test-update records: schema/rules.rs changed only in BUILTIN_CHECKS (`decision-chain` added — its binding coverage lives in the validate-decisions and rule-bindings tests); schema/migrate.rs changed only to pass the new `None` argument at its save_decision call site.
