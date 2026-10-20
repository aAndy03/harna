# harna

**A software-engineering toolbox for building long-lived Rust codebases with AI agents.** harna keeps a project's architecture, conventions and decisions as *data* in the repository (`.harna/`), shows every agent session the context it needs, and checks the code against what was declared.

Like a design tool's palette, harna shows tools and never dictates the design: every tool is optional, and only what you declare is enforced. What your app needs (offline use, a local password, license-based auth, sync) is built by the agents and people using harna, never hardcoded into it.

## Why

When you build software with AI agents:

- context windows are limited and every compaction forgets what was agreed;
- nothing stops drift: duplicated code, hardcoded values, layers that leak into each other;
- a change in one place silently breaks the code that depends on it.

harna's answer: **instructions become data.** A rule stated once is stored with its reason and shown to every future session that works in its scope. Structure is declared in a manifest and checked on every edit and commit. Structural changes open a list of everything they affect (parity) until each item is reviewed.

## How it works

```
.harna/
  manifest.toml     architecture: layers -> containers -> components, with gateways, depends_on, roles
  rules.toml        conventions: text + rationale + scope (+ optional check binding)
  decisions/        why things are the way they are (ADR-style)
  digest/           the code map: modules, public signatures, imports (sharded, regenerable)
  parity/           change-propagation events and their open debts
  roadmap.toml      declared milestones          terms.toml      shared vocabulary
  non-goals.toml    deliberate refusals          lessons.toml    tried-and-failed directions
  commands.toml     the repo's own commands      handoff.toml    where the last session stopped
  acknowledged.toml / deferred.toml / baselines   rule exceptions, always with a reason
```

Everything is versioned in git with the code it describes.

## The toolbox

`harna tools` (or the MCP resource `harna://tools`) lists every tool: what it does, when to reach for it, and how. `harna place` and `harna plan` also offer the tools that fit the code you are about to touch. They are suggestions, never requirements. The main tools:

| Tool | What it gives you |
|---|---|
| **Layers + `depends_on`** | Who may import whom. Cargo dependencies are held to the same graph, and `harna deny` makes it a build error. |
| **Gateways** | One owner for disk, network or processes (`fs`, `fs:write`, `net`, `process`), or for an external crate (`crate:rusqlite`). |
| **Declared surface (`provides`)** | The names other containers may import. Everything else stays private and free to change. |
| **Corridors (transiters)** | One named, thin hand-off between layers: a command bus, an event channel, an API facade. |
| **Rules** | Conventions with their reason, scoped to a path. Optionally bound to one of 49 checks (`harna checks`), e.g. banned APIs with `=> replacement`, crate conformity, naming patterns, size budgets, or a proof command CI runs. |
| **Scaffold / move** | Structure born where it belongs (several components per call), and moves that keep every link: manifest, `mod` declarations, `use` paths. |
| **Parity** | A structural change becomes the list of modules to review. Open debts block validation. |
| **Ledgers** | Bend a rule on the record: acknowledge (with a reason), baseline (a count that may only fall), defer (review at a date or milestone). |
| **Memory** | Decisions, non-goals, lessons, roadmap, handoff, glossary: what a future session must not re-litigate. |

## Install

```bash
cargo install harna-cli   # the `harna` CLI
cargo install harna-mcp   # the MCP server (stdio)
```

## Start a project

```bash
git init
harna init --agents          # .harna/ + AGENTS.md with the agent contract (an existing AGENTS.md only gains a managed block)
harna hooks install          # pre-commit gate + Claude Code hooks (+ CI for PRs and release tags); --craft adds fmt/clippy/tests
harna digest                 # what every session starts with
```

Then, for every burst of work: `place`/`plan` → `scaffold` → write code → `scan` → `parity open` → resolve → `validate --changed` → tests → `preflight` → commit with `.harna/`.

```bash
harna plan "monthly spending report"                      # where it belongs, rules in scope, tools that fit, fan-out
harna scaffold --layer app --container reporting --name monthly --name totals --kind primitive
harna layer set app --depends-on domain                   # who may import whom
harna container set sqlite --gateway crate:rusqlite       # only storage may use the driver
harna container set domain --provides Money,Account       # the domain's public surface
harna container tests sqlite --add crates/storage/tests/repos.rs
harna add-rule --id money-exact --text "Money is never a float" \
  --rationale "rounding corrupts balances" --scope crates/domain \
  --check banned-api --value "f64 => Money"
harna validate --changed
harna preflight                                           # the release gate
```

## Agents (MCP)

Add the `harna-mcp` binary as a stdio MCP server. 46 tools mirror the CLI (`harna_onboarding`, `harna_place`, `harna_plan`, `harna_validate`, `harna_parity_*`, `harna_scaffold`, …). Resources expose the data (`harna://manifest.toml`, `harna://rules.toml`, `harna://tools`, …). `HARNA_TOOLS=core` advertises only the session-ritual tools.

With hooks installed, Claude Code gets onboarding at session start, violations right after each edit, and a blocked change sent back before it stops.

## Enforcement, without a wall

A rule without a check is guidance: it is shown, never failed. A bound rule reports violations that quote the rule and its reason. Severity is graded (info advises, error blocks), and every exception is recorded with a reason. Checks run after each agent edit, at commit (pre-commit hook), and in CI only for pull requests and release tags.

## Versioning

The tool is semver. The data format (`.harna/`) carries its own schema version: a newer harna migrates older data (`harna migrate`, with backup and a logged decision), and an older harna refuses newer data rather than corrupting it.

## Roadmap

Declared as data in [`.harna/roadmap.toml`](.harna/roadmap.toml) (`harna roadmap list`):

- **0.3.50** toolbox discoverability.
- **0.4.0** long-term intent as data (qualities) and architecture profiles. The first profile is a Rust desktop app with gpui-kit.
- **0.4.1** the plugin mechanism. gpui-kit is the first plugin, kept in parity with its published API.
- **0.5.0** a TypeScript plugin.
- **0.6.0** ownership and versioned contracts between layers.
- **0.7.0** service boundaries and operational readiness.

## Repository layout

harna manages itself with harna. `crates/harna-core/src` is layered `schema` (memory) → `search` (code map) → `enforcement` (checks, parity) / `generation` (scaffold, move, views) → `composition` (briefings, gates). The CLI and MCP crates sit on top. See `.harna/manifest.toml` and the generated `ARCHITECTURE.md`.
