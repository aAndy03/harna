# MCP surface parity with the CLI (review tier 3)

The MCP server gains the CLI's gates and registers as tools: harna_status, harna_preflight, harna_validate_intent, harna_acknowledge, harna_decompose (tools/gates.rs) and harna_non_goal, harna_lesson, harna_roadmap, harna_terms (tools/registers.rs, action-dispatched). They live in submodules with their own rmcp routers merged in HarnaTools::new, so the oversized tools.rs does not grow. harna_validate now goes through acknowledge::gate, the same ledger partition and verdict the CLI uses, and every render states the verdict.

Deliberately NOT changed: harna_place keeps returning the full plan briefing (the 0.3.0 decision 'harna plan full pre-code briefing' — the tool keeps its name); its description already says CLI place is the quick map. harna_adapt (the original misspelling of adopt) stays as an alias of the new harna_adopt so existing agent configs keep working.

Pattern rules with an un-compilable regex now report as harna-data Error, not an Info under the borrowed manifest-depends id: an unenforceable rule is broken data (fail-closed, as in tier 1), and a borrowed bindable id let a rule binding or the ledger re-grade it away. The CLI/MCP add-rule edges already validate regexes, so only hand edits hit this. Rule origins and register records are local ISO dates on every surface (MCP wrote unix-days).
