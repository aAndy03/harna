# 0.2.2: machine output + strictness + status

status tool added to the briefing container (rename from 'onboarding' — it holds the briefing components now). validate/diff/parity show accept --json; the validate JSON report carries counts and a blocks verdict. Gating corrected: only Error violations block by default; --strict makes Info gate too (0.2.1's exit path gated on any violation regardless of severity — now explicit).
