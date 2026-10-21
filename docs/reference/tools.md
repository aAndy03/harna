# The toolbox

Generated from `crates/harna-core/src/composition/briefing/toolbox.toml` — do not edit.

36 tools. Every one is optional: harna shows them, the engineer or agent chooses (`harna tools`, MCP `harna://tools`; `place` and `plan` offer the ones that fit).

## Gateway: one owner for a facility

makes one container the only place that touches disk, network or processes (fs | fs:write | net | process).

- **Reach for it:** code that talks to the outside world; keep the rest of the system pure and testable
- **How:** `harna container set <container> --gateway fs:write`
- **Enforced by:** check `gateway-ownership`

## Crate gateway: one owner for an external crate

only the owner's crate may depend on the crate in Cargo.toml, only its modules may name it.

- **Reach for it:** a database driver, HTTP client or UI toolkit that should stay behind one boundary
- **How:** `harna container set <container> --gateway crate:<name>`
- **Enforced by:** check `gateway-ownership`

## Layers and depends_on: who may import whom

each layer imports only itself and the layers it declares; Cargo dependencies are held to the same graph.

- **Reach for it:** any boundary that must hold as the code grows (rules may not reach storage, UI may not reach the database)
- **How:** `harna layer set <layer> --depends-on a,b`
- **Enforced by:** check `layer-boundary`

## Declared surface: what a container offers

a container lists the names other containers may import; everything else stays private and free to change.

- **Reach for it:** a container others depend on — a domain kernel, a repository layer — whose internals must stay changeable
- **How:** `harna container surface <container> (a draft from its pub use / public items; --apply accepts it) | harna container set <container> --provides Money,Account`
- **Enforced by:** check `declared-surface`

## Corridor: one named, thin hand-off between layers

a transiter container connects two layers; the hand-off lives in one place instead of being spread through both.

- **Reach for it:** a container many other layers import directly — a command bus, an event channel, an API facade
- **How:** `harna transiter --from <layer> --to <layer>`
- **Enforced by:** check `transiter-thinness`

## Rule: a convention as data

records what to do and why for a scope; shown to every session working there; optionally bound to a check.

- **Reach for it:** the moment a convention is agreed (money is never a float, errors never leak storage details)
- **How:** `harna add-rule --id <id> --text ... --rationale ... --scope <path> [--check <id>] [--scope-signature 'Money'] (by meaning: every file declaring a match)`

## Banned API with its replacement

a path may not appear in scope; `path => replacement` says where to go; harna deny writes it into clippy.toml.

- **Reach for it:** a call that must go through your own abstraction (wall clock, raw filesystem, floats for money)
- **How:** `harna add-rule ... --check banned-api --value 'std::time::SystemTime::now => Clock::now'`
- **Enforced by:** check `banned-api`

## Crate conformity

every crate in scope carries required files (file:NAME) and Cargo keys (toml:KEY = VALUE).

- **Reach for it:** workspace-wide conventions: inherited lints, a companion BUILD file, a license header file
- **How:** `harna add-rule ... --check crate-conformity --value 'toml:lints.workspace = true'`
- **Enforced by:** check `crate-conformity`

## Registered tests

ties components to the tests that cover them; `harna tests --component X` prints the exact command.

- **Reach for it:** integration tests that cover many components; a change without its test update is named
- **How:** `harna container tests <container> --add <tests/file.rs>`
- **Enforced by:** check `no-test-update`

## Planned slots: a home for the future

declares a component before its code (the cache will live here, behind this gateway); validate reads it as intent; place points matching work at it; scaffold with its name fills it.

- **Reach for it:** the parts a quality says you will need (cache, job queue, audit log, sync) but do not build yet
- **How:** `harna slot add <container> warm-cache --path crates/app/src/cache/warm.rs --summary 'read-through cache behind the storage gateway'`

## Test kinds: what each kind of test may cross

declares where a kind of test lives (path globs) and which layers it may reach; its files and the crates' dev-dependencies are held to that.

- **Reach for it:** integration or end-to-end tests that cross layers on purpose — make the crossing a declared choice
- **How:** `harna test-kind add integration --path 'crates/*/tests/**' --reach store; require one: harna add-rule --check kind-coverage --value ui --scope crates/features`
- **Enforced by:** check `test-reach`

## Scaffold: structure first

creates components (several per call) registered in the manifest and wired into the module tree.

- **Reach for it:** before writing code: the files are born where they belong
- **How:** `harna scaffold --layer <l> --container <c> --name a --name b --kind primitive [--empty] [--new-crate lib|bin]`

## Move without breaking links

relocates a file or folder and relinks manifest, rule scopes, mod declarations and use paths.

- **Reach for it:** restructuring; never move Rust files by hand
- **How:** `harna move <from> <to>`

## Parity: changes reach their importers

turns a structural diff into the list of modules that must be reviewed; open debts gate validation.

- **Reach for it:** after any change to a public signature, trait or module
- **How:** `harna scan; harna parity open <id>; harna parity resolve ...`

## Acknowledge, baseline, defer: bending a rule on the record

an exception with a reason, a count that may only fall, or a review keyed to a date or milestone.

- **Reach for it:** a violation that is intentional or too large to fix now — never silence it
- **How:** `harna acknowledge ... | harna baseline ... | harna deferred add ...`

## Qualities: what the software must hold over its life

records long-term intent (scale, latency, offline, data-loss tolerance, privacy) with its measure and scope; onboarding, plan and place show it to every session.

- **Reach for it:** before designing anything that must survive growth: say how many users, how fast, how safe, offline or online
- **How:** `harna quality add scale-users --statement 'serves many households' --measure '1M users' --category scale [--held-by <rule-id>]`

## Decision log

records a non-obvious choice with its context; superseding keeps the chain.

- **Reach for it:** any choice a future session would otherwise re-litigate
- **How:** `harna log-decision --title ... --body ...`

## Non-goals and lessons

deliberate refusals and tried-and-failed directions; plan/place surface them before anyone re-proposes.

- **Reach for it:** the user says no to something, or an approach failed with evidence
- **How:** `harna non-goal add ... | harna lesson add ...`

## Generated outputs and their proof

declares generated files and the command that proves them current; harna's CI workflow runs it.

- **Reach for it:** anything produced by a generator (schemas, bindings, workflows)
- **How:** `harna add-rule ... --check generated --scope <outputs> --value <sources> --command '<gen> --check'`
- **Enforced by:** check `generated`

## Roadmap and handoff

declared milestones (where the project is going) and the burst journal (where this session stopped).

- **Reach for it:** planning what is next; before a session ends or compacts
- **How:** `harna roadmap add|set ... | harna handoff --summary ... --next ...`

## Plugins: a framework's conventions as a versioned pack

vendors a framework's rules and vocabulary (gpui-kit 0.8.0, ...) into .harna/plugins/<name>/; they apply beside your rules as <plugin>/<id>, and the doctor warns when Cargo.lock outruns the version the plugin mirrors.

- **Reach for it:** building on a framework whose conventions should hold in every session, kept in step with its release
- **How:** `harna plugin list; harna plugin add gpui-kit; harna plugin show gpui-kit`

## Practices: habits that keep agent-built code healthy

eight opt-in advisory rules over the generic checks: errors are not swallowed, no silent fallbacks, no placeholders or debug leftovers, no secret shapes, design tokens instead of raw colors or pixel sizes, no copied files (Calendar2, auth_new).

- **Reach for it:** a codebase agents write day after day, where these habits creep in one file at a time
- **How:** `harna plugin add practices; keep, rescope (add-rule --replace) or deprecate each practices/<id>; harna try-rule practices/<id> --text .. to see one work`

## Profiles: a starting shape for a kind of project

writes a manifest, rules, qualities and vocabulary to prune, and installs the plugins the shape builds on.

- **Reach for it:** a new project of a known kind (a gpui-kit desktop app), before the first crate exists
- **How:** `harna profile list; harna init --profile rust-gpui-desktop`

## Code pattern: a convention over code lines, as data

a rule's regex over live code (comments and tests masked); within:<regex> limits it to the block a line opens (inside fn render), unless:<regex> excuses a statement (.detach()).

- **Reach for it:** a convention a type system cannot hold: state created in render, a dropped handle, an index used as identity
- **How:** `harna add-rule --id r --text ... --rationale ... --check code-pattern --pattern 'cx\.new\(' --value 'within:fn render\('`
- **Enforced by:** check `code-pattern`

## Registry: declared names used, bindings unique

declare:<regex> names (group 1) must be used elsewhere (use:<regex> with NAME); bind:<regex> keys (group 1, scope group 2) must not repeat in one scope — full validate.

- **Reach for it:** actions, commands, routes, key bindings: anything declared in one place and handled in another
- **How:** `harna add-rule --check registry --value 'declare:actions!\([^,]+,\s*\[([^\]]*)\]' ...`
- **Enforced by:** check `registry`

## Capabilities: a plugin's tokens one container owns

a plugin names token sets ([capabilities]); a container owns one (gpui-kit:menus, gpui-kit:raw-input) and every other container calling those is an ownership error.

- **Reach for it:** one engine for menus, raw input, a bridge: the framework names stay in the plugin, the boundary in your manifest
- **How:** `harna plugin show gpui-kit; harna container set <engine> --owns gpui-kit:menus`
- **Enforced by:** check `ownership`

## API parity: a framework's surface as data

a plugin's api.toml (generated from the release source: harna plugin api) lets every path into the crate resolve; an invented module or item is named with the version.

- **Reach for it:** agents writing against a fast-moving framework API they half remember
- **How:** `harna plugin add gpui-kit (ships api.toml); bind a rule: --check api-parity`
- **Enforced by:** check `api-parity`

## TypeScript containers: the same words over TS/JS

TS/JS files join the map (exports, imports as file edges, packages as npm:<name>, tsconfig paths and workspace packages resolved); a container over a TS folder keeps layers, isolation, declared surface and package gateways (npm:<pkg>).

- **Reach for it:** a web front end, a Tauri webview, a Node service beside the Rust
- **How:** `harna container add web --layer ui --path web/src; harna container set api --gateway npm:axios`
- **Enforced by:** check `layer-boundary`

## Bridges: Rust <-> TypeScript calls as edges

a plugin's [[bridges]] (a Rust marker, optionally naming the fn with (?P<name>...), and a TS call with NAME) link each call to the Rust file that exposes it; dependents, parity and layers see across the language line.

- **Reach for it:** Tauri commands, deno ops, WASM exports: a rename on one side that the other must follow
- **How:** `harna plugin add tauri; harna dependents src-tauri/src/commands/x.rs`

## Adopt: a draft manifest from an existing tree

proposes layers, containers and depends_on from the code (one layer per crate, or per top-level folder with --by-dir); fixtures left out with --exclude; a cycle the real edges make is named.

- **Reach for it:** bringing harna to a repo that already exists
- **How:** `harna adopt --by-dir --exclude tests/specs --exclude '**/testdata/**'`

## Owners: who answers for a part

people or teams per layer and container; the innermost owner answers for a path; a change is routed to its reviewers, and unowned paths are named.

- **Reach for it:** more than one person or team on the code, or an agent that must ask the right human
- **How:** `harna container set ledger --owners @ledger-team; harna owners --changed --base main`

## Contracts: a surface frozen at a version

freezes a container's provided names with their whole signatures; validate calls a gone or re-signed name breaking (until a new major) and a new name compatible; a re-freeze needs the version the change earns.

- **Reach for it:** a container other teams or crates build against, where a quiet signature change breaks them
- **How:** `harna contract freeze money --version 1.0.0; harna contract show money`
- **Enforced by:** check `contract`

## Deprecation windows: removal counted in versions

reads #[deprecated(since = ..)] and @deprecated since x, counts minor versions against the project version, names what must go.

- **Reach for it:** an API you are retiring, so it does not linger forever
- **How:** `harna deprecations; harna add-rule --check deprecation-window --limit 2 ...`
- **Enforced by:** check `deprecation-window`

## Affected: run what a change can break

changed files against a base, their containers, the containers importing them (re-exports too), the crates to test and the crates depending on them; says why it widens to everything; JSON for a CI matrix.

- **Reach for it:** a CI that runs everything on every change, or a reviewer asking what else this touches
- **How:** `harna affected --base main; harna affected --base main --json`

## Scoped instructions: an AGENTS.md per container

a container folder's own AGENTS.md block: its layer, what it may import, owners, surface, gateway and the rules covering it; kept current by render.

- **Reach for it:** a large repo where an agent works in one folder and the root file is too long to be read
- **How:** `harna render --scope ledger; harna render`

## Services: what runs on its own, and what it owns

names each service, the containers only it runs, the resources only it writes (db:x, queue:y) and the corridors it talks through with their message shapes; validate holds single ownership, no direct imports between services, and matching shapes on both ends; readiness facets (health, backups, …) each service answers before preflight passes.

- **Reach for it:** a product that deploys as more than one process, or is about to split into some
- **How:** `harna service add ledger-api --containers ledger --owns db:ledger --runs crates/ledger-api; harna service corridor add postings --kind event --from ledger-api --to sync --message Posted=crates/events/src/lib.rs::Posted; harna services`
- **Enforced by:** check `service-coherence`
