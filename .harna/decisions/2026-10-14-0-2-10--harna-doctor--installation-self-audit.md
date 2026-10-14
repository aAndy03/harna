# 0.2.10: harna doctor (installation self-audit)

doctor composes the readiness view: .harna presence, schema currency (migrate hints; FAIL when data is newer than the tool), parseability of manifest/rules, decisions, digest staleness measured against file mtimes (warn with changed-module count), git presence, commit gate wiring, agent wiring (MCP config mentions). FAIL exits non-zero for scripts; every check carries its next action. Doctor registered as compound component of briefing.
