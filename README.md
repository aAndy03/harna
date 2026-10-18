# harna

**Persistent agent context for codebases.** harna turns agent instructions into *data* that lives in the repository — so context survives compaction, new sessions, and even different machines or agent providers.

## The problem it solves

When you build apps with AI agents:

- the agent's context window is limited, and AGENTS.md can only hold so much;
- every compaction wipes conversational memory, so you re-explain the same rules again and again;
- nothing stops the agent from drifting: redundant code, hardcoded values, broken layering.

**harna's answer: instructions become data.** A fresh agent session calls one MCP tool and gets a ~1,000-token digest of everything it needs. Rules you state once are persisted in the repo and inherited by every future session automatically.

## How it works

harna adds a `.harna/` folder to any repository it manages:

```
.harna/
  manifest.toml    # architecture: layers -> containers -> components (primitive/compound)
  rules.toml       # coding rules, each with rationale, scope and origin
  decisions/       # ADR-style markdown decision log (why things are the way they are)
  digest.json      # compressed code map: signatures only, no file bodies
```

Everything is versioned in git like the code itself.

## The three crates

| Crate | Role |
|---|---|
| `harna-core` | Manifest model, rules store, code digest, onboarding digest |
| `harna-cli` | `harna init / digest / rules / add-rule / log-decision / manifest / scan` |
| `harna-mcp` | MCP stdio server exposing the same capabilities as agent tools |

### Layout: harna's own topology

`harna-core/src` is organized by harna's own principles (layers → containers → components) — see `.harna/manifest.toml` for the registered map and `AGENTS.md` for the working conventions:

```
schema/       container of primitives: manifest.rs, rules.rs, store.rs, migrate.rs
search/       container: digest.rs (compressed code map)
enforcement/  container: validate.rs (conformance checks)
generation/   container: scaffold.rs (structure from templates)
composition/  container: onboarding.rs (compound: composes schema + search)
```

Containers never import each other sideways; only `composition/` reaches down into others.

## MCP tools (what the agent calls)

- **`harna_onboarding`** — call at the start of every session; returns architecture overview, all active rules, and a codebase map. Replaces re-reading AGENTS.md.
- **`harna_rules(scope?)`** — only the rules that apply to the module being touched, with rationale so the agent *understands* them, not just obeys.
- **`harna_add_rule`** — persist a rule the moment the user states a preference. Survives compaction.
- **`harna_log_decision`** — record why something was built this way.
- **`harna_scan`** — rebuild the compressed code map (signatures only), so agents query the map instead of rescanning huge repos.
- **`harna_validate`** — conformance checking: hardcoded URLs, cross-layer imports, manifest paths that no longer exist. `changed_only: true` scopes it to files with uncommitted git changes.
- **`harna_diff`** — structural diff of the code map against a git ref: modules, pub signatures, and internal deps that moved. The substrate for parity fan-out.
- **`harna_parity` / `harna_parity_open` / `harna_parity_resolve` / `harna_parity_auto`** — the change-propagation ledger: open a parity event on structural changes (fan-out = changed modules + importers), show debts, resolve targets, re-verify unchanged surfaces. Open debts block validation.
- **`harna_deny`** — render the manifest's layer topology into `deny.toml`; `cargo deny check bans` turns layer violations into compile errors.
- **`harna_scaffold`** — generate a primitive/compound component from a template and register it in the manifest.
- **`harna_scaffold_transiter`** — generate a transiter: a dedicated component that connects two engines/layers.
- **`harna_migrate`** — upgrade `.harna/` data to the current schema after updating harna.

## Versioning: two axes, one command

**Tool version** (the binaries) is plain semver. **Data schema version** is a field stamped into `manifest.toml` and `rules.toml` — the protocol between tool and repo:

- Newer tool + older data: reads fine; writes require `harna migrate` first.
- Older tool + newer data: **refuses to run** with an upgrade message — silent corruption of your project's knowledge must never happen.
- After updating harna, run `harna migrate` (or the `harna_migrate` MCP tool): backs up old files to `.harna/backup/`, upgrades the schema, and logs the migration as a decision entry — reviewable as a git diff.

## Environments (dev / prod / staging)

Architecture is environment-invariant; configuration is not. Rules are global by default; an env-specific rule carries `env = "dev"` and is only injected into sessions that declare that env:

```bash
harna add-rule --id mock-stripe --text "Stripe calls use the mock adapter" \
  --rationale "No real charges from dev sessions" --scope src --env dev
harna rules --env dev          # global rules + dev rules
harna rules                    # global rules only
```

MCP equivalent: pass `env` to `harna_rules` / `harna_add_rule`.

## Enforcement loop (the guardrail)

```bash
harna validate --changed       # only .rs files with uncommitted git changes
harna parity open <event-id>   # turn the current structural diff into parity targets
harna parity auto <event-id>   # re-verify unchanged targets; deleted ones stay owed
harna parity show              # see the debts
harna parity resolve ...       # ack / noop each target
harna deny                     # compile-time layering: deny.toml from the manifest
```

Wire `harna validate --changed` into pre-commit and CI (`harna_validate` with `changed_only: true` after edits). Structural changes additionally open parity debts: each propagated module must be acknowledged, recorded as nothing-needed, or auto-verified before validation passes. Regenerate `deny.toml` whenever the layer topology changes; run `cargo deny check bans` in CI so wrong cross-crate imports fail the build, not just the review. The agent is the only untrusted component — checking is deterministic.

## Installing

### From crates.io (v0.3.0 and up)

```bash
cargo install harna-cli   # the harna CLI (binary `harna`)
cargo install harna-mcp   # the MCP server
```

### From git (always available; tags are the releases)

```bash
cargo install --git https://github.com/YOUR/harna --tag v0.3.0 harna-cli   # harna CLI
cargo install --git https://github.com/YOUR/harna --tag v0.3.0 harna-mcp   # MCP server
```

- Both binaries land on PATH (`~/.cargo/bin`); every tool-call and config
  below then works from any directory and any project.
- **Private repositories** install the same way — `cargo install --git`
  authenticates through git (SSH key or HTTPS token), so access to the repo
  is all you need. Pinning by `--tag` (or `--branch` for a private line of
  development) works identically.
- Nothing in the data (`​.harna/`) contains absolute paths — cloning,
  branching, moving between machines is safe.

## Setup with an agent (OpenCode / Claude / etc.)

Add an MCP server entry running the `harna-mcp` binary over stdio. This repository ships a working example in `opencode.json`; for another
project, copy the same shape (binaries come from PATH after installing):

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "servers": {
      "harna": {
        "type": "local",
        "command": ["harna-mcp"],
        "cwd": "."
      }
    }
  }
}
```

**Hooks (recommended).** `harna hooks install` makes the gates run on
their own instead of relying on the agent to remember them:

- **git pre-commit** (`.githooks/pre-commit`, wired via `core.hooksPath`):
  `harna validate --changed` before every commit; it fails *closed* when
  no harna binary is found (bypass once: `HARNA_SKIP_HOOK=1`).
- **Claude Code** (`.claude/settings.json`, merged with yours): onboarding
  injected at session start, violations reported right after each edit,
  and a blocked change sent back once before the agent stops.
- **CI** (`.github/workflows/harna.yml`): validate + preflight on push/PR.

`--target git|claude|ci` picks a subset; `harna hooks status` audits.
`--craft` adds the Rust craft gates: `cargo fmt --check` and
`cargo clippy -D warnings` before every commit (bypass once:
`HARNA_SKIP_CRAFT=1`), and a CI job running fmt, clippy and the test
suite — the gate `preflight` names as NOT RUN.
harna never overwrites a hook or setting it did not write (unless
`--force`). Agents can do the same through the `harna_hooks` MCP tool.

**Tool profile (optional).** The server advertises all 41 tools by default
(~19 KB of catalog in the agent's context). Set `HARNA_TOOLS=core` in the
server's environment to advertise only the 15-tool session ritual (~8.5 KB),
or a comma list (`HARNA_TOOLS=validate,status,handoff`). Every tool's logic
is the same either way; a profile only changes what is listed.

Then keep `AGENTS.md` to a single line:

```
Source of truth is .harna/. Start every session by calling the harna_onboarding MCP tool.
```

## CLI quickstart

```bash
harna init                                  # create .harna/
harna add-rule --id no-hardcoded-urls \
  --text "Use Endpoint constants" \
  --rationale "Hardcoded URLs broke staging twice" \
  --scope src/api
harna log-decision --title "..." --body "..."
harna scan                                  # build digest.json
harna digest                                # print the onboarding text
harna rules --scope src/api --agent

# plan before code (the full briefing lives in the MCP `harna_place` tool)
harna plan "timezone picker data"           # placement + decisions + rules + fan-out + tests
harna place "timezone picker data"          # quick placement map only

# step 3 of the loop: structure is born correct
harna scaffold --layer api --container users \
  --name user-repo --kind primitive \
  --description "persists and loads users"
harna scaffold --layer engine --container billing \
  --name billing-engine --kind compound
harna transiter --from api --to billing     # engines never call each other directly

# enforcement
harna validate                              # conformance: drift, hardcoded URLs, layer crossings
```

## What validate catches (the conformance loop)

| Check | Violation it reports | Suggested fix |
|---|---|---|
| `no-hardcoded-urls` | `http(s)://` literals in source (endpoint/constants modules exempt) | move URL to the Endpoint constants module |
| `layer-boundary` | `use crate::<other_layer>::` inside a layer's directory | route through a transiter engine |
| `manifest-path` | component/container path in manifest that no longer exists | scaffold it or fix the manifest |

Delete a folder the manifest knows about, or paste a URL into a handler, and `harna validate` (or the `harna_validate` MCP tool) names the file, the line, and the fix.

## Git workflow

- Commit `.harna/` **with the code it describes** — atomic PRs mean `git blame` on a rule shows the conversation that created it.
- `rules.toml` merges append-mostly (keep both sides); `digest.json` is regenerable — on merge conflict, take either side and run `harna scan`.
- Hooks: pre-commit → `harna validate --changed`; CI → `harna validate`; post-merge → `harna scan`.

## Developer workflow reminder

## Design principles (registered in the manifest model)

1. **Layering** — layers contain containers; containers contain components.
2. **Components** — `primitive` (leaf) or `compound` (hierarchical composition of children).
3. **Decisions at the edges** — rationale and origin are first-class fields, never lost.
4. **Modularity** — every artifact is specialized and importable/queryable.
5. **Token efficiency** — every rendering targets the smallest useful output (overview truncation, scope filters, signatures-only digest).

## Roadmap

- Rule-driven checks — `validate` currently runs built-in checks; next: rules in `rules.toml` with a check-id tag become enforced checks.
- Component-level rules (attach a rule to a single component), dependency graph of the digest (`deps`), richer scanners (TS, trait impls, `use` graph).
- Custom scaffold templates per project (template folder under `.harna/templates/`).

## CI recipes (GitHub Actions)

Drop this alongside your regular test job. Build harna once with the repo's
own toolchain, then let the installed checks gate:

```yaml
name: harna
on: [push, pull_request]

jobs:
  harna:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust (stable)
        uses: dtolnay/rust-toolchain@stable

      - name: Build harna
        run: cargo build --release

      - name: harna doctor (installation gate)
        run: ./target/release/harna doctor

      - name: harna validate (conformance; exit 2 gates)
        run: ./target/release/harna validate

      - name: Regenerate deny.toml and fail if it went stale
        run: |
          ./target/release/harna deny
          git diff --exit-code deny.toml

      - name: Generated views are current (fail if ARCHITECTURE.md went stale)
        run: ./target/release/harna render --check

      - name: cargo deny (layering = build errors)
        run: |
          cargo install cargo-deny --locked
          cargo deny check bans

      - name: harna tests
        run: cargo test --workspace
```

Notes:
- `harna validate` exits 2 on violations (Error-severity); `--strict`
  additionally gates Info violations.
- Parity debts gate through the same command (`parity-open` is an Error).
- `harna deny && git diff --exit-code deny.toml` makes regenerate-when-stale
  explicit: forgetting to re-render after layer changes fails the job. The
  equivalent in-repo signal is the `deny-sync` Info violation from
  `harna validate`.
- In pre-commit, keep the fast path: `harna validate --changed`.
