# 0.3.49: a field test drives the release before 0.4.0

Context: the first born-under-harna project (ledger-harna-test, built by an agent following the ritual) found that parity missed trait-method signature changes, deny wrote clippy entries clippy cannot resolve, and init tripped its own doc-claims check; plus workflow friction (no tests CLI, one-at-a-time scaffold, super:: paths after move).

Decisions:
- Trait methods become surface as an additive signature form (trait Name: fn ...) instead of reformatting existing pub fn signatures: an extractor bump that changed old signature strings would raise a breaking parity event for every user on upgrade.
- deny writes only paths clippy can resolve and withdraws its own stale entries (identified by the '(harna rule ...)' reason it writes); the user's entries are never touched.
- doc-claims exempts .harna/ paths rather than init writing empty registers, which would trip roadmap coherence instead.
- container tests applies to a whole container by default: the real case was one integration test file covering a crate.
- move rewrites super:: to the old parent's crate:: path (absolute, always correct) rather than computing a new relative path.

No-test-update notes: cmd-rules (checks listing), preflight NOTE label, diff baseline_note, mcp flow/memory text, map extractor doc, live brace_delta visibility - covered by the field-test replay and the existing suites.
