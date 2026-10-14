# 0.3.0 feature 4: `harna plan <idea>` — the full pre-code briefing

The PLAN step becomes a decision proposal instead of a placement hint: one
briefing (CLI `harna plan`, MCP `harna_place` — the tool keeps its name and
now returns the full briefing) that enumerates what the change will touch
before the first line exists:

- placement: similar modules from the digest with their (layer, container),
  plus the "candidates to extend" — the containers hosting them;
- decision context: decisions matching the idea, supersession-aware — a
  superseded decision is rendered VISIBLY DEAD with its successor, so dead
  guidance is named, not followed (feature 2 makes this data);
- rules to respect with their bound check ids (`layer-boundary`,
  `no-test-update`, ...) — documentation rides the enforcement it gates;
- dependencies to expect: the dependents query (ring 1 + knock-on + hosts)
  on the matched modules — planning half of the parity fan-out;
- test expectations: what those components register as tests, plus the
  obligations (`no-test-update` advisory in `validate --changed`,
  `tests-declared` gate),
- conventions: constants-are-data, engine-own-container, and the
  log-decision record reminder.

`harna place` stays the quick placement map; the token matching is shared
(`place::tokenize`) so both briefings match identically. New composition
component `plan` (registered in the briefing sequence) — three new
composition tests including no-matches and dead-decision paths.

Session 4 no-test-update record: `composition/place.rs` changed only by
extracting the tokenizer into `tokenize` for `plan` to share — its behavior
(placement map, rules, conventions) and test coverage are untouched; the
shared matcher gains its assertions through plan.rs's tests.
