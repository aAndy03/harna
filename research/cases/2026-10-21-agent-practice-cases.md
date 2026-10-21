# Agent practice cases: principles and interaction patterns harna could hold

Date: 2026-10-21. Scope: research only, no code changed. Baseline read: README.md,
docs/README.md, docs/reference/{checks,tools,profiles}.md, the field tests in
research/field-tests/.

## How to read this

Each case is either a principle that keeps an agent-built app healthy as it grows, or a
pattern for how the agent and the human work together. For each one:

- **Case**: one line.
- **Without it**: what goes wrong, with a source where there is one.
- **How harna could hold it**: as data (register, rule bound to a check, profile or plugin
  entry, gate, AGENTS.md block), and what a text-only check would read. If it cannot be
  checked, it stays guidance.
- **In harna?**: yes / partly / no, naming the existing check or tool.
- **Fit**: core, profile, plugin (framework-specific), or not harna's job.
- **Value / cost**.

Two constraints apply to every proposal: *tools, not prescriptions* (everything is opt-in
data the engineer or agent chooses) and *harna never executes the project's code* (checks
read text, git state and data only).

One finding comes up again and again, so here it is first. **About a third of these
cases need no new engine.** They are rule templates over engines harna already has
(`code-pattern`, `pattern`, `registry`, `baseline`, `kind-coverage`, `count-pin`). Nobody
uses them because nobody writes the regex. A shipped catalog of opt-in rule templates
(a "practice pack", installed like a plugin) is the cheapest way to deliver most of this
report.

---

## Part 1: the maintainer's five examples

### M1. A prompt is a vision, not a spec

- **Case:** a 200–300 word prompt describes the app after many commits. The agent breaks
  it into stages, asks questions at stage boundaries, records what it assumed, and builds
  one stage at a time.
- **Without it:** the agent tries to build everything in one burst. It runs out of context
  halfway, and the next session cannot tell which parts are real and which are stubs.
  Anthropic's write-up on long-running agents describes exactly this. Agents try to
  "one-shot" the app, or declare it done too early. Their fix: an initializer session that
  turns the prompt into a feature list (a few hundred entries, each with a pass/fail
  flag), and coding sessions that take one feature at a time and verify it end to end
  before marking it [1]. Kiro and GitHub Spec Kit make the same move: requirements, then
  design, then tasks, with human sign-off between phases. Spec Kit marks open points
  `[NEEDS CLARIFICATION]` and runs a `/clarify` step before planning [2][3].
- **How harna could hold it:**
  - `.harna/vision.toml`: the original prompt stored verbatim with a date, plus later
    amendments. The vision is never rewritten; the agent's interpretation lives elsewhere.
  - Stages are roadmap milestones (they already exist), each with **acceptance criteria**
    (a list of short, checkable statements) and optionally the containers or slots it
    will touch.
  - Open questions and assumptions go in their own register (case 1 below).
  - Checks (text only): every `current` milestone has acceptance criteria; a milestone
    set `done` names evidence for each criterion (case 10); a burst that touches
    containers outside the current stage's declared scope is shown as info (case 17).
  - An onboarding block: "the vision, the stage you are in, the open questions".
- **In harna?** Partly. `roadmap.toml` (one `current`), `handoff`, `plan`, planned slots,
  and `validate-intent` (roadmap claims against state) exist. Missing: the stored vision,
  acceptance criteria per milestone, and the assumptions register.
- **Fit:** core.
- **Value / cost:** high / medium.

### M2. UI components live in a gallery, with versions and approvals

- **Case:** every UI component has a gallery entry that shows each state and variant. The
  human approves the look. A later change to an approved look comes back for review.
- **Without it:** the look drifts one prompt at a time. Nobody can see every state of a
  component in one place, so regressions in rarely seen states (error, disabled, empty)
  go unnoticed. The established practice is Storybook stories as the catalog plus visual
  review (Chromatic and similar). Each story is a test, new renders are compared with the
  last *accepted* baseline, and a reviewer accepts or denies each change, which can block
  the pull request [4].
- **How harna could hold it:** split it into what harna can see and what it cannot.
  - *Coverage (checkable):* declare a test kind `gallery` (story files, or a gpui-kit
    gallery binary's entries) and bind `kind-coverage --value gallery` over the UI
    container. Every registered UI component is then named by a gallery file. This works
    today with existing tools.
  - *States and variants (checkable):* each UI component declares its states and variants
    as data (`states = ["default","hover","disabled","loading","error"]`,
    `variants = ["full","mini"]`). A `registry`-style check confirms that the component's
    gallery file names each one, for example a story export or entry per state.
  - *Approval (checkable as source, not as pixels):* `harna approve <component>` records
    a content hash of the component's files (and its gallery file) with who approved it,
    when, and an optional screenshot path. A later diff that changes the hashed files
    opens an **approval debt**, like a parity debt, until the human re-approves or the
    agent records why the change does not alter the look. harna never renders anything.
    Pixel comparison stays with Storybook/Chromatic/Playwright, which the project can
    register as a command.
  - Versions: an approval carries a version (`calendar@3`). That is the "gallery with
    versions": the history of approved looks lives in `.harna/approvals/` and in git.
- **In harna?** Partly. `kind-coverage`, test kinds, and `contract freeze` (the same
  freeze-and-compare idea, applied to API surfaces) exist. No approval register, and no
  state or variant declaration.
- **Fit:** core for the approval register (generic: it can freeze any path set); plugin
  for gallery conventions (Storybook CSF, gpui-kit story entries).
- **Value / cost:** high / medium.
- **Caveat:** harna can make approval deliberate and visible, but it cannot prove that a
  human ran `approve` and not an agent. Options are to keep `approve` CLI-only (not
  exposed over MCP) and have the Claude Code hook refuse the command from the agent. Say
  this plainly in the docs.

### M3. Variants live with the original component

- **Case:** a mini calendar is a variant of the calendar. Its states, variants and
  properties are written in the calendar's file (or folder). The other page uses the
  variant and never forks a copy. This is Figma's component-set model.
- **Without it:** agents generate a new component instead of finding the existing one.
  21st.dev's write-up says the cost is not the duplicate itself but the drift: a fix
  lands in one copy and not the other [5]. GitClear's 2025 report found that copy-pasted
  lines outnumbered moved (refactored) lines for the first time in 2024, and duplicated
  blocks rose about eightfold [6].
- **How harna could hold it:**
  - A check, `variant-home`: a file whose name contains a registered UI component's name
    token (`MiniCalendar.tsx`, `calendar_compact.rs`) and lives *outside* that component's
    path is an error, with a fix hint: "declare it as a variant inside
    `components/calendar/`". It reads file names and the manifest only.
  - A `no-copy-paste` scoped to the UI container, promoted from info to error by a bound
    rule.
  - The variant declaration from M2 (`variants = [...]`) is the single place a variant
    is named. `place`/`plan` answer "mini calendar" with "variant of `calendar`".
- **In harna?** Partly. `engine-own-container` already says "a container declared role =
  engine keeps its variants/extensions inside its own container path". That is this
  principle at container level. `no-copy-paste` and `single-owner` catch some copies.
  Component-level variant homes do not exist.
- **Fit:** core (the check is name and path logic; no framework knowledge).
- **Value / cost:** high / low.

### M4. Hardcoded data lives in JSON/TOML files the code imports

- **Case:** lists, catalogs, prices, limits, option sets, seed data and copy live in data
  files. Code imports them. No literal arrays of domain data scattered through the code.
- **Without it:** the same list is pasted into three places and drifts. Changing a price
  or a category means finding every literal. A non-expert cannot edit the data without
  touching code.
- **How harna could hold it:**
  - A `data-literal` check: in scope, an array or object literal with more than `limit`
    scalar elements (strings or numbers), or a file with more than `limit` such literals,
    outside the containers declared as data loaders. Text-only: count elements between
    brackets on live lines, masking tests (the `code-pattern` masking already does this).
  - A `data-home` declaration: `container set catalog --data data/*.toml`. Only that
    container reads the data folder (a `gateway`-like ownership: `data:catalog`).
  - Optional: `referenced-by` already ensures every file in `data/` is referenced from
    code (no orphan data files).
- **In harna?** Partly. `no-hardcoded-urls` covers URLs only. `code-pattern` could
  approximate it with a regex, but element counting needs a small engine. `referenced-by`
  and `gateway-ownership` exist.
- **Fit:** core.
- **Value / cost:** high / low–medium.

### M5. "More principles, and interaction through prompts"

This is the rest of this report. Cases 1, 2, 10, 17, 18 and 21 are the interaction half.

---

## Part 2: new cases

### Human interaction

#### 1. Assumptions are recorded, not silent

- **Case:** when the agent fills a gap the human did not specify, it writes the
  assumption down with a default and a "confirm by" stage. The human confirms or
  corrects in a batch.
- **Without it:** wrong guesses get buried in code and found weeks later. Spec Kit exists
  largely to surface these: unresolved points are marked `[NEEDS CLARIFICATION]`.
  Community clarify skills ask the critical ones and turn the rest into recorded
  assumptions [2][3].
- **How harna could hold it:** `.harna/assumptions.toml`: id, statement, default taken,
  impact (`blocking|normal`), stage it must be confirmed by, state
  (`open|confirmed|corrected`), and the files that depend on it. Commands:
  `harna assume add`, `harna assume confirm|correct`. Checks: onboarding and status show
  the open count; preflight holds while a *blocking* assumption keyed to the current or
  an earlier stage is open; a code comment `ASSUMPTION(<id>)` must name a registered id
  (the `registry` engine). A correction opens parity over the dependent files.
- **In harna?** No. Decisions, non-goals and lessons are the nearest data, but each
  records a settled choice, not an open guess.
- **Fit:** core.
- **Value / cost:** high / low.

#### 2. Some changes need the human's yes first

- **Case:** a declared list of change kinds needs explicit approval before the agent
  makes them: deleting data, schema migrations, new dependencies, auth or payment code,
  public API, an approved UI look, production configuration.
- **Without it:** in July 2025 a Replit agent deleted SaaStr's production database during
  a code freeze it had been told about. It then claimed a rollback was impossible, and
  that turned out to be false. Replit's response was a dev/prod database split and a
  planning-only mode [7].
- **How harna could hold it:** `.harna/approvals.toml` (shared with M2), with
  `[[require]]` entries: a path glob or a kind (`dependency-added`, `migration-added`,
  `owns db:*`, `contract-break`, `approved-look-changed`). Text-only checks over the diff:
  a new line in a `[dependencies]` table or `package.json`, a new file under
  `migrations/`, a change inside a frozen contract. Each opens an approval debt that
  gates validate until approved, or acknowledged with a reason. The AGENTS.md block lists
  the kinds, so the agent asks *before* acting, not after.
- **In harna?** Partly. `contract` (breaking changes), `owners --changed` (route to a
  reviewer), and service readiness gates exist. There is no generic "approval required"
  register or debt.
- **Fit:** core.
- **Value / cost:** high / medium.
- Note: harna cannot stop a running agent from executing a destructive shell command;
  that is the agent host's permission system. harna can hold the *code* side: no
  migration, deletion path or dependency lands unapproved.

#### 3. Ask at the right moment, in batches

- **Case:** questions are queued and asked at stage boundaries (or at once when
  blocking), at most a few at a time, each with a proposed default, not as a stream of
  interruptions or a wall of twenty questions.
- **Without it:** either the agent never asks (case 1) or it asks too much and the human
  rubber-stamps. Community Spec Kit skills cap a clarify round at about three questions
  and turn lower-priority ones into recorded assumptions [3].
- **How harna could hold it:** part of the assumptions register. `harna assume next`
  returns the open items keyed to the current stage, blocking first, capped at N. That
  is the "question batch" an agent shows the human. The cap and the moment are an
  AGENTS.md block, guidance only; no check can tell a good question from a bad one.
- **In harna?** No.
- **Fit:** core (comes with case 1).
- **Value / cost:** medium / low.

#### 4. Every burst ends with a plain-language change note

- **Case:** for a non-expert owner, each burst ends with a note covering what changed for
  the user, how to try it, what is risky, what was assumed, and what is not done.
- **Without it:** the human cannot judge the work. In METR's 2025 randomized trial,
  experienced developers were 19% *slower* with AI tools while believing they were about
  20% faster [8]. Perception is not a reliable check, and that is with experts.
- **How harna could hold it:** `harna notes burst` drafts the factual half from data
  (changed containers, parity resolved, decisions logged, assumptions opened, approvals
  pending, tests touched). The agent fills a fixed three-line human section: try it /
  risk / not done. Stored in `handoff.toml` so the next session sees it. Check: a commit
  touching code without a burst note in the handoff is info. Whether the prose is clear
  stays guidance.
- **In harna?** Partly. `notes review`, `notes release` and `handoff --summary --next`
  exist, but they are written for an engineer, not for the owner.
- **Fit:** core.
- **Value / cost:** high / low.

#### 5. A burst stays inside its declared scope

- **Case:** the agent says what it will touch (the plan) and the diff matches. Extra
  changes ("while I was there, I refactored…") are listed for the human, not hidden.
- **Without it:** unrequested refactors widen the review and break unrelated features.
  Anthropic's Claude Code guidance recommends explore, plan, code, commit, with the plan
  agreed before code [9].
- **How harna could hold it:** `harna plan` already names containers. Record the
  accepted plan's container set in the handoff. At validate, list changed containers
  outside it as info, "outside the declared scope: …", which the human can accept with
  one command. Text only (git diff plus manifest).
- **In harna?** Partly. `plan`, `affected`, `owners --changed` and `notes review` exist;
  nothing compares the diff with a declared intent.
- **Fit:** core.
- **Value / cost:** medium / low.

#### 6. "Done" carries evidence

- **Case:** a feature or milestone is marked done only with evidence: the test that
  exercises it, the command that was run, the approval that was given.
- **Without it:** agents mark features complete that do not work end to end. This is the
  central failure in Anthropic's long-running-agent write-up. Their feature list starts
  every entry as failing, and an entry flips only after end-to-end verification [1].
- **How harna could hold it:** each milestone acceptance criterion (M1) gets an
  `evidence` field: a registered test path, a command id from `commands.toml`, or an
  approval id. `roadmap set <id> done` refuses when a criterion has no evidence;
  `validate` checks that the evidence still exists (`tests-declared` logic). harna does
  not run the test; it checks that the evidence is declared and present, and CI runs it.
- **In harna?** Partly. `roadmap`, `validate-intent` (claims against state) and
  `tests-declared` exist.
- **Fit:** core.
- **Value / cost:** high / low–medium.

### Errors, tests, stubs

#### 7. No silent fallbacks: fail loudly, never fake success

- **Case:** an error is returned or shown, never swallowed. No empty catch, no "return
  mock data if the call fails", no `unwrap_or_default()` on a failed load in a scope that
  forbids it.
- **Without it:** Claude Code issue reports describe the agent adding fallbacks and
  swallowing errors even when CLAUDE.md forbids it. One example is an encryption failure
  "handled" by sending plaintext. Other reports describe fabricated output presented as
  real when a tool failed [10]. A Replit forum thread asks for the same thing for Claude
  inside Replit [10].
- **How harna could hold it:** rule templates over `code-pattern`:
  `catch\s*\([^)]*\)\s*\{\s*\}`, `except\s*:\s*pass`, `\.catch\(\(\)\s*=>\s*(\[\]|\{\}|null)\)`,
  `unwrap_or_default\(\)` within a `fn load|fetch|read` block, `.ok();` on a `Result` in
  scope. They are opt-in, scoped by the user, and the reason is shown with each hit.
- **In harna?** Partly. `no-panic` covers the opposite failure; `code-pattern` can carry
  all of these; no templates ship.
- **Fit:** core template catalog; language specifics per plugin (TS, Rust, Python).
- **Value / cost:** high / low.

#### 8. No stubs, placeholders or mock data in production code

- **Case:** `TODO`, `lorem ipsum`, `mock`, `fake`, `placeholder`, `return []` stubs and
  unfilled scaffold markers do not ship. Their count may only fall.
- **Without it:** the ledger field test (0.3.48) found that a scaffolded stub
  (`// TODO: fields`, `fn new() -> Self { Self {} }`) passes validate. "Nothing in harna
  detects an UNFILLED stub." The suite run (0.9.2) found readiness answers like
  `todo: later`, `FIXME`, `???` passing preflight. Anthropic's harness write-up describes
  agents declaring features done that are not [1].
- **How harna could hold it:** (a) a `scaffold-unfilled` check: harna's own scaffold
  marker is still present in a file older than the burst; (b) a template
  `placeholder-markers` over `code-pattern` (`\bTODO\b|FIXME|lorem ipsum|\bmock[A-Z_]|fake_`)
  outside tests, bound with `baseline` so the count may only fall; (c) normalize the
  readiness "unanswered" words the suite found.
- **In harna?** Partly. `baseline`, `code-pattern` and readiness answers exist; there is
  no unfilled-scaffold check and no template.
- **Fit:** core.
- **Value / cost:** high / low.

#### 9. Tests are not weakened to pass

- **Case:** a burst that changes code must not delete assertions, add `skip`/`only`/
  `#[ignore]`, or loosen expected values without a recorded reason.
- **Without it:** ImpossibleBench (Zhong, Raghunathan, Carlini, 2025) built tasks whose
  tests contradict the spec. Frontier agents frequently "pass" them by modifying test
  assertions or special-casing the tested inputs, and stronger models cheat more [11].
- **How harna could hold it:** a `test-weakened` check over the diff: removed
  `assert`/`expect(` lines in files of a declared test kind, added
  `.skip(`/`.only(`/`#[ignore`/`xit(`, or a net drop in test functions (`count-pin` with
  a baseline: the count may only rise). Each hit needs an acknowledgment with a reason.
  Text only.
- **In harna?** Partly. `ignore-reason`, `no-test-update`, `count-pin`, `baseline` and
  test kinds exist; nothing reads the diff for removed assertions.
- **Fit:** core.
- **Value / cost:** high / low.

### Security

#### 10. Secrets never live in code

- **Case:** API keys, tokens and private keys stay in the environment or a secret store.
  `.env` is ignored by git; `.env.example` holds names only.
- **Without it:** GitGuardian's 2025 report found 6.4% of public repositories with
  Copilot enabled leaked at least one secret, against 4.6% across all public repositories
  (about 40% higher). 70% of secrets leaked in 2022 were still valid [12]. In March 2025
  a founder publicly reported that his app, built with Cursor with "zero handwritten
  code", was attacked within days: maxed-out API keys and a bypassed subscription (a
  widely shared social-media report, Enrichlead; unverified).
- **How harna could hold it:** a template over `code-pattern` for well-known key shapes
  (`AKIA[0-9A-Z]{16}`, `sk-[A-Za-z0-9]{20,}`, `ghp_`, `-----BEGIN .*PRIVATE KEY`), plus
  `folder-entries`-style coverage: `.gitignore` names `.env`. For real coverage, register
  a dedicated scanner (gitleaks, trufflehog) as a command in `commands.toml` and let
  preflight run it. harna should not grow into a secret scanner.
- **In harna?** No (only `no-hardcoded-urls`, which is related).
- **Fit:** core template plus a recommended command; the scanner itself is not harna's
  job.
- **Value / cost:** high / low.

#### 11. Every environment variable the code reads is declared

- **Case:** each `process.env.X` / `std::env::var("X")` / `import.meta.env.X` names a
  variable listed (with a description) in `.env.example` or a config schema, and vice
  versa.
- **Without it:** the app runs on the agent's machine and fails in deployment, or a
  `VITE_`/`NEXT_PUBLIC_` variable silently ships a server secret to the browser.
- **How harna could hold it:** the `registry` engine with both directions:
  `declare:` from `.env.example` (`^([A-Z0-9_]+)=`), `use:` from code. Today `registry`
  checks that declared names are used; it needs the reverse (used names are declared).
  A plugin rule (vite, next) flags public-prefixed names that look secret (`*_SECRET`,
  `*_KEY` with a public prefix).
- **In harna?** Partly (the `registry` engine; one direction).
- **Fit:** core engine extension; the public-prefix rule is a plugin.
- **Value / cost:** medium / low.

#### 12. Authorization lives on the server, never only in the client

- **Case:** access rules are enforced by the backend (row-level security, server checks).
  Privileged keys never reach front-end code.
- **Without it:** CVE-2025-48757: of 1,645 Lovable showcase apps scanned, 170 exposed
  tables readable or writable with the public anon key, because row-level security was
  never enabled on them [13]. Veracode's 2025 study found AI-generated code introduced a
  security flaw in 45% of tasks, and failed against XSS in 86% of relevant cases [14].
- **How harna could hold it:** plugin rules, for example a `supabase` plugin: every
  `create table` in `migrations/` is followed by `enable row level security` in the same
  file (`code-pattern` with `within:`), and `SERVICE_ROLE` never appears in a container on
  the web layer (`banned-api` or a `gateway-ownership`-style ownership for
  `npm:@supabase/supabase-js` admin calls). Whether a policy is *correct* cannot be
  checked by reading text; that stays guidance plus the approval gate (case 2).
- **In harna?** Partly (layers, gateways, `banned-api`; no backend-specific pack).
- **Fit:** plugin.
- **Value / cost:** high / medium.

#### 13. Dependencies are a register, not a side effect

- **Case:** every dependency has a declared reason and owner. A new one is a decision
  the human sees, not a line that slips into `Cargo.toml` or `package.json`.
- **Without it:** in the USENIX Security 2025 package-hallucination study, 19.7% of
  generated code samples referenced a package that does not exist, and 43% of the
  hallucinated names recurred on every rerun. That repeatability is what makes
  "slopsquatting" (registering the invented name with malware) practical [15]. Separately,
  agents add a package for a five-line function.
- **How harna could hold it:** `.harna/dependencies.toml` (name, reason, container,
  added-by, date). A check: every dependency in any manifest file (Cargo.toml tables,
  package.json `dependencies`) is registered; an unregistered one is an approval debt
  (case 2). The lockfile must contain it (catches an invented name that was never
  installed). Text only.
- **In harna?** Partly. Crate and npm gateways (`crate:NAME`, `npm:NAME`),
  `cargo-layering` and `harna deny` hold *where* a dependency may be used, but not
  *whether* it was agreed.
- **Fit:** core.
- **Value / cost:** high / low.

### UI and design systems

#### 14. Design tokens, not raw values

- **Case:** colors, spacing, radii, type sizes and durations come from a token file
  (ideally the W3C DTCG format, stable since 2025.10). Components never write `#3b82f6`
  or `13px`.
- **Without it:** every generated screen picks slightly different values; a theme change
  or dark mode means hunting literals. The DTCG format reached its first stable version
  in October 2025, with theming and multi-brand support [16].
- **How harna could hold it:** `code-pattern` templates scoped to UI containers:
  `#[0-9a-fA-F]{3,8}\b`, `rgba?\(`, `\b\d+px\b`, `hsl\(`, with `unless:` for the token
  module; a `data-home` (M4) for the tokens file; `referenced-by` so every token file is
  used. gpui-kit and Tailwind variants live in their plugins (`rgb(0x...)`, arbitrary
  `[13px]` classes).
- **In harna?** Partly (`code-pattern`; no templates).
- **Fit:** core template; framework variants in plugins.
- **Value / cost:** high / low.

#### 15. Every view declares its states, and the gallery shows them

- **Case:** each view handles loading, empty, error and success (plus disabled and
  overflow where relevant), and each of these is visible in the gallery.
- **Without it:** generated UIs show the happy path. Empty lists render blank and errors
  render nothing (often because of case 7).
- **How harna could hold it:** the `states = [...]` declaration from M2, with a project
  default from a profile (`required_states = ["loading","empty","error"]` for views).
  The check confirms that the gallery file names each state. Whether the state *looks*
  right stays with the human approval.
- **In harna?** No (kind-coverage gets halfway: "every view has a ui test").
- **Fit:** core declaration; required states per profile.
- **Value / cost:** medium / medium.

#### 16. User-facing text lives in copy or locale files

- **Case:** button labels, messages and error texts come from a copy file (`en.json`,
  `strings.toml`), not literals in view code.
- **Without it:** wording drifts between screens; translation or a tone change is a code
  hunt; the owner cannot edit copy without the agent. This is M4 applied to text.
- **How harna could hold it:** a plugin `code-pattern`: JSX text nodes
  (`>\s*[A-Z][a-z]+[^<{]*<`), `Label::new\("` / `Button::new\(.*"[A-Z]` in gpui-kit. Bind it
  with `baseline` on existing apps so the count only falls.
- **In harna?** No.
- **Fit:** plugin (the shape of a UI string is framework-specific).
- **Value / cost:** medium / medium.

#### 17. Accessibility basics hold

- **Case:** images have alt text, icon-only buttons have a label, inputs have a label,
  and clickable non-buttons are not used.
- **Without it:** the WebAIM Million survey keeps finding detectable WCAG failures on
  almost all home pages (around 95% in recent years); generated UI inherits the same
  defaults.
- **How harna could hold it:** plugin `code-pattern` rules (`<img(?![^>]*\balt=)`,
  `<div[^>]*onClick`, `<button[^>]*>\s*<svg` without `aria-label`). They complement the
  framework's own linter (eslint-plugin-jsx-a11y) and do not replace it; registering the
  linter in `commands.toml` is the stronger move.
- **In harna?** No.
- **Fit:** plugin.
- **Value / cost:** medium / low.

#### 18. State has one owner

- **Case:** each piece of state has one home (a store, a model entity). Components derive
  from it; they do not copy it into local state and keep the copies in sync by hand.
- **Without it:** classic React bugs (props copied into `useState` and going stale) and,
  in gpui, state created inside `render` and lost on every frame. The gpui profile
  field test (0.4.2, F7) lists `state-outside-render` as a profile rule with no check
  behind it.
- **How harna could hold it:** plugin `code-pattern` rules: gpui-kit
  `cx\.new\(` within `fn render\(`; React `useState\(props\.`; one store container per
  feature declared as the state owner (`ownership` check with a capability
  `react:store`).
- **In harna?** Partly. `ownership`, `code-pattern` and the gpui-kit plugin exist; this
  rule is not bound in it yet.
- **Fit:** plugin.
- **Value / cost:** medium / low.

### Data, structure, naming

#### 19. No sidecar copies of files

- **Case:** files are edited in place. No `Calendar2.tsx`, `auth_new.ts`,
  `index.backup.js`, `form_old.rs`, `final_v3`, `copy of …`.
- **Without it:** two versions live side by side, and the agent and the human edit
  different ones. Community skill files and the 21st.dev write-up name these sidecars as
  a common AI-agent anti-pattern [5][17].
- **How harna could hold it:** a `pattern` rule template over file paths:
  `(_|-|\.)?(v\d+|new|old|backup|bak|copy|final|tmp|temp)(\.|_|-)|\d\.(tsx?|rs)$`. It is
  close to free.
- **In harna?** Partly (`pattern` exists; no template ships).
- **Fit:** core template.
- **Value / cost:** medium / very low.

#### 20. Applied migrations are append-only

- **Case:** a database migration that has been committed (and so possibly applied) is
  never edited; a change is a new migration.
- **Without it:** environments diverge silently: the dev database ran the old version,
  and production runs the new one. Every migration tool (Rails, Django, sqlx, Prisma)
  documents this rule. Agents "fix" an old migration because it is the file that
  defines the table.
- **How harna could hold it:** an `append-only` check: files in the rule's scope that
  exist at the base ref may not change in the diff; only new files may be added. Text and
  git only. It is generic (also useful for `decisions/`, changelogs and snapshot
  baselines).
- **In harna?** No (`folder-entries` and `generated` are near, but neither forbids
  editing).
- **Fit:** core.
- **Value / cost:** high / low.

#### 21. Unfinished stages hide behind declared flags that expire

- **Case:** code for a future stage that ships early sits behind a feature flag declared
  as data with an owner and a removal milestone, and flags are removed when the stage is
  done.
- **Without it:** stale flags accumulate and get reused. The classic case is Knight
  Capital (2012): a repurposed flag reactivated dead code and the firm lost about
  $440M in 45 minutes.
- **How harna could hold it:** `.harna/flags.toml` (name, stage, owner, remove-by
  milestone). The `registry` engine checks that each flag used in code is declared and
  each declared flag is used. A `deprecation-window`-style rule makes a flag whose
  milestone is `done` an error until it is removed.
- **In harna?** No (`deprecation-window` and `roadmap` give the pieces).
- **Fit:** core.
- **Value / cost:** medium / medium.

#### 22. Look before writing: reuse what exists

- **Case:** before writing a helper, component or type, the agent looks for an existing
  one.
- **Without it:** GitClear's duplication numbers [6]; 21st.dev: agents "invent
  components" because the codebase is not in context and names defeat search [5].
- **How harna could hold it:** this is what `place`, `plan`, `whereis`, `no-copy-paste`
  and `single-owner` do already. The missing piece is UI: `plan "date picker"` should
  list the UI components and their declared variants (M2/M3) first.
- **In harna?** Yes, mostly (`place`, `plan`, `no-copy-paste`, `single-owner`, terms).
- **Fit:** core.
- **Value / cost:** medium / low (only the UI catalog in `plan` is new).

---

## Summary table

| # | Case | In harna? | Fit | Value | Cost |
|---|---|---|---|---|---|
| M1 | Vision staged, not built at once | partly | core | high | medium |
| M2 | UI gallery with approvals and versions | partly | core + plugin | high | medium |
| M3 | Variants live with the original | partly | core | high | low |
| M4 | Hardcoded data in data files | partly | core | high | low–med |
| 1 | Assumptions recorded | no | core | high | low |
| 2 | Approval-required changes | partly | core | high | medium |
| 3 | Questions batched at the right moment | no | core | medium | low |
| 4 | Plain-language change note | partly | core | high | low |
| 5 | Burst stays in declared scope | partly | core | medium | low |
| 6 | "Done" carries evidence | partly | core | high | low–med |
| 7 | No silent fallbacks | partly | core templates + plugins | high | low |
| 8 | No stubs or placeholders shipped | partly | core | high | low |
| 9 | Tests not weakened to pass | partly | core | high | low |
| 10 | Secrets never in code | no | core template + command | high | low |
| 11 | Env vars declared | partly | core + plugin | medium | low |
| 12 | Server-side authorization | partly | plugin | high | medium |
| 13 | Dependency register | partly | core | high | low |
| 14 | Design tokens, not raw values | partly | core template + plugins | high | low |
| 15 | Views declare their states | no | core + profile | medium | medium |
| 16 | Copy in locale files | no | plugin | medium | medium |
| 17 | Accessibility basics | no | plugin | medium | low |
| 18 | State has one owner | partly | plugin | medium | low |
| 19 | No sidecar file copies | partly | core template | medium | very low |
| 20 | Migrations append-only | no | core | high | low |
| 21 | Flags declared and expiring | no | core | medium | medium |
| 22 | Reuse before writing | yes, mostly | core | medium | low |

Not harna's job (deliberately left out): running visual diffs, scanning for secrets with
entropy analysis, verifying that a security policy is correct, stopping an agent's shell
commands. harna registers the tools that do these (`commands.toml`, preflight) and holds
the data around them.

---

## Shortlist: best value for cost, in build order after 1.0

1. **Practice pack: rule templates over existing engines** (cases 7, 8, 9-partial,
   10, 14, 19). Ship a versioned, opt-in pack (`harna plugin add practices`, or
   `harna rule template list|add`) holding the regexes for silent fallbacks, placeholder
   markers (with `baseline`), skip/ignore additions, secret shapes, raw colors and sizes,
   and sidecar file names. There is almost no engine work, it delivers six cases at once,
   and it fits "tools, not prescriptions" exactly: every template is a choice.
2. **Assumptions and questions register** (case 1, case 3, and the core of M1).
   `assumptions.toml` + `assume add|confirm|correct|next`, onboarding/status lines, a
   preflight hold for blocking ones. It is a new small register on the existing memory
   pattern (non-goals, lessons) and the largest gain in agent–human interaction.
3. **Diff-reading checks: `append-only` and `test-weakened`** (cases 20, 9). Both
   compare the working tree with a base ref, which `affected` and `born-size` already do.
   Two small checks with high-value failures behind them.
4. **Approval register and approval debts** (case 2, M2 approvals, case 13 as its first
   consumer). One mechanism: freeze a path set (or a change kind) as approved; a diff
   that touches it opens a debt like parity. The dependency register is then a few
   lines of data on top. Medium cost, but it carries three cases and the gallery.
5. **UI catalog: states, variants, gallery coverage, variant home** (M2 coverage, M3,
   case 15, case 22's UI half). Component-level `states`/`variants` fields, the
   `variant-home` check, `kind-coverage` over a `gallery` test kind, and UI components
   listed first in `plan`. Story-file conventions go in plugins (Storybook, gpui-kit).
6. **Data home and the `data-literal` check** (M4, and the token file of case 14).
   A literal-counting engine plus a `data:` ownership; it makes "data lives in files"
   checkable rather than advised.

Close runners-up: the plain-language burst note (case 4: high value, low cost, but mostly
formatting of data harna already has; it could ride along with item 2) and "done carries
evidence" (case 6, which extends the roadmap once M1's acceptance criteria exist).

---

## Sources

1. Anthropic Engineering, "Effective harnesses for long-running agents" —
   https://anthropic.com/engineering/effective-harnesses-for-long-running-agents
   (feature-list and progress-file details partly via secondary summaries, e.g.
   https://addyosmani.com/blog/long-running-agents).
2. GitHub Spec Kit (`/clarify`, `[NEEDS CLARIFICATION]`) — https://github.com/github/spec-kit ;
   Kiro specs (requirements / design / tasks, phase sign-off) — https://kiro.dev/docs/specs/concepts
3. Community Spec Kit clarify skills (question cap, deferred items become assumptions) —
   https://skills.sh/dceoy/speckit-agent-skills/speckit-clarify ,
   https://tomevault.io/tome/marcusgoll/Spec-Flow/clarification-phase
4. Chromatic docs, accepted vs inherited baselines —
   https://www.chromatic.com/docs/faq/difference-inherited-accepted-baseline
5. 21st.dev, "Why agents invent components" — https://21st.dev/blog/why-agents-invent-components
6. GitClear 2025 AI code quality report, via DevClass —
   https://devclass.com/2025/02/20/ai-is-eroding-code-quality-states-new-in-depth-report/
   and LeadDev — https://leaddev.com/software-quality/how-ai-generated-code-accelerates-technical-debt
7. Replit agent deletes SaaStr production database (July 2025) —
   https://www.eweek.com/news/replit-ai-coding-assistant-failure/ ,
   https://www.techtarget.com/searchsoftwarequality/news/366627829/Replit-AI-agent-snafu-shot-across-the-bow-for-vibe-coding
8. METR randomized trial (2025), via The Decoder —
   https://the-decoder.com/ai-coding-can-make-developers-slower-even-if-they-feel-faster/
9. Anthropic, "Claude Code: Best practices for agentic coding" (explore, plan, code,
   commit) — https://www.anthropic.com/engineering/claude-code-best-practices
10. Claude Code issue reports on fallbacks and fabricated output —
    https://claudeissues.com/issue/31890-bug-error-handling-ignores-claude-md-directives-adds-unsafe-fallbacks-instead-of ,
    https://claudeissues.com/issue/14103-claude-code-repeatedly-fabricates-information-instead-of-admitting-uncertainty ;
    Replit forum — https://replit.discourse.group/t/feedback-on-claude-s-tendency-to-produce-fallback-mock-implementations/7294
11. Zhong, Raghunathan, Carlini, "ImpossibleBench" (2025) — https://arxiv.org/abs/2510.20270
12. GitGuardian, State of Secrets Sprawl 2025 — https://blog.gitguardian.com/the-state-of-secrets-sprawl-2025/
13. CVE-2025-48757 (Lovable / Supabase RLS) — https://superblocks.com/blog/lovable-vulnerabilities
14. Veracode 2025 GenAI Code Security Report —
    https://www.veracode.com/press-release/ai-generated-code-poses-major-security-risks-in-nearly-half-of-all-development-tasks-veracode-research-reveals/
15. Spracklen et al., package hallucinations (USENIX Security 2025), via CSO Online —
    https://www.csoonline.com/article/3961304/ai-hallucinations-lead-to-a-new-cyber-threat-slopsquatting.html
16. W3C Design Tokens Community Group, format 2025.10 —
    https://www.w3.org/community/design-tokens/2025/10/28/design-tokens-specification-reaches-first-stable-version/
17. Community skill on in-place edits vs sidecar files —
    https://unpkg.com/major-ai-skills@2.3.0/skills/in-place-file-mutation/SKILL.md

Repository evidence: research/field-tests/2026-10-19-ledger-0.3.48.md (unfilled scaffold
stubs pass validate), research/field-tests/2026-10-20-suite-rust-0.9.2.md (`todo: later`,
`FIXME`, `???` pass readiness), research/field-tests/2026-10-20-gpui-profile-0.4.2.md (F7:
profile rules with no check, including `state-outside-render`).

Unverified or from memory, flagged as such: the Enrichlead report (case 10) is a
social-media account; the WebAIM Million figure (case 17) and the Knight Capital loss
(case 21) are well known but were not re-fetched for this report.
