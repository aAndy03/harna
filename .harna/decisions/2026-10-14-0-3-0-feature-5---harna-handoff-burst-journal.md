# 0.3.0 feature 5: `harna handoff` — the burst journal

The "where was I?" loop is closed with data instead of archaeology: before
compaction or running out of context, the agent records the current-burst
state with `harna handoff --summary .. --next ..` (MCP `harna_handoff` —
the 10th tool; `clear` when the burst is resumed). One data file,
`.harna/handoff.toml`, overwritten per burst; history is git blame as
everywhere in harna. The record captures the open-parity count for
cross-checking against the live ledger.

Session-start surfaces carry it without a separate query: `harna_onboarding`
renders summary + ordered next steps, `harna status` reminds while a burst
is open ("handoff on disk: N declared next step(s)"), and doctor checks its
parseability (a corrupt journal would strand the answer silently). The
AGENTS contract gained the handoff duty (template + this repo).

Why a new schema component rather than a decision entry: handoff is
operational state — written, overwritten, cleared in bursts — not durable
rationale. Decisions record *why*; handoff records *where we are right
now*. Composition (`status`, `onboarding`, `doctor`) reaches into the
schema component, per the container rules.
