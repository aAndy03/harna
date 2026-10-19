# Thanks

harna learns from projects whose authors made their work public. Each one below was read
(never modified) to find what harna is missing; what we learned is in the linked report.

| Project | License | Studied | What it taught harna |
|---------|---------|---------|----------------------|
| [denoland/deno](https://github.com/denoland/deno) | MIT | 2026-10-19, commit `a18ce33` | Capability-typed permission gateways, ratcheting baselines, per-crate banned-API lists, sync-checked registration, docs drift. [Report](research/2026-10-19-deno.md) |
| [openai/codex](https://github.com/openai/codex) | Apache-2.0 | 2026-10-19, commit `54f9901` | Workspaces below a Bazel root, who-may-depend-on-a-crate gates, regenerate-and-diff schema tests, crates mirrored by BUILD files; and harna's scale limits (validate 168s → 14s). [Report](research/2026-10-19-codex-zed-ruff.md) |
| [zed-industries/zed](https://github.com/zed-industries/zed) | GPL-3.0 / AGPL-3.0 / Apache-2.0 (per crate) | 2026-10-19, commit `042e040` | Crate roots named after the crate, crate names that shadow std modules, banned APIs with replacements, context-scoped lints, generated CI with drift guards, per-crate agent rules. [Report](research/2026-10-19-codex-zed-ruff.md) |
| [astral-sh/ruff](https://github.com/astral-sh/ruff) | MIT | 2026-10-19, commit `1939846` | Local names shadowing crate names, trait-injected IO keeping core crates pure, snapshot and spec-markdown test data, generators whose check mode nothing runs. [Report](research/2026-10-19-codex-zed-ruff.md) |
