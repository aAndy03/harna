# Three more repos: what codex, zed and ruff show harna is missing

**Subjects** (shallow clones from 2026-10-19):

| Repo | Commit | Stars | Rust lines | Crates | Shape |
|------|--------|-------|------------|--------|-------|
| [openai/codex](https://github.com/openai/codex) | `54f9901` | 130k | 2.09M | 157 | Cargo workspace in `codex-rs/`, Bazel at the repo root |
| [zed-industries/zed](https://github.com/zed-industries/zed) | `042e040` | 92k | 1.78M | 258 | crate roots named `src/<crate>.rs` |
| [astral-sh/ruff](https://github.com/astral-sh/ruff) | `1939846` | 50.2k | 840k (+129k Python/TS) | 55 | linter and type checker, Python packaging |

**Method** (same as the [deno study](2026-10-19-deno.md)):
1. harna 0.3.46 mapped each repo automatically (`init`, `scan`, `adopt`, `validate`, `metrics`, `HARNA_TIMINGS`), then again with its own draft applied.
2. The same two Haiku readers as for deno read organization and orchestration, comparing against deno.
3. Every claim used here was re-checked in the tree.

Each gap is generalized. Priority: **P1** = harna misreads or mishandles repos of this shape. **P2** = the next capability. **P3** = polish.

---

## H. What harna's own run exposed

### Fixed during the study (unreleased, ships as 0.3.47)

Each fix has a regression test named after the repo that exposed it.

| # | Gap | Evidence | Fix |
|---|-----|----------|-----|
| H1 | Cargo workspace below the repo root | codex: the root is Bazel; adopt found **0** cross-crate edges | Every tracked `[workspace]` manifest is a root when the repo root has none; `workspace = true` resolves against the member's own workspace |
| H2 | Scan cost grew with crate count | zed: per line, rebuilding and searching ~800 crate-name needles took **15s** | One pass over `::` with a name map, with word boundaries (`my_crate::` ≠ `crate::`). zed 15s → **1.4s**, ruff 7.3s → 1.6s |
| H3 | Validate cost grew with module count | codex validate **168s**. `hosted_module` scanned every module for every dependency, and `deny-sync` reloaded the crate index per container (35s) | `HostIndex` (O(stem depth)), index loaded once per plan, copy-paste maps keyed by file index. codex **168s → 14s**, zed 52s → 12.6s, deno 74s → 16s |
| H4 | `use` statements read line by line | zed has crates named `path`, `fs`, `channel`, so `use std::{\n path::PathBuf }` was read as an import of zed's `path` crate. Meanwhile `use client::{…, proto}` recorded **no** dependency | Whole statements are parsed: multi-line groups keep their prefix, brace groups expand per item, and a crate-root group is a dependency (extractor v8) |
| H5 | Local names did not shadow crate names | ruff: `use crate::rules::ruff;` then `ruff::rules::…` was read as the `ruff` crate, inventing cross-crate cycles Cargo forbids | Names a file brings into scope (`use`, declared mods) shadow crate names |
| H6 | Only `lib.rs`/`main.rs` were crate roots | zed: 225 crates use `src/<crate>.rs` via `[lib] path`. Root items didn't resolve, and root→child facades counted as cycles | `Module::is_crate_root()` covers lib, main, and the file named after the crate. zed cycles 696 → 281 |
| H7 | One package name, two crates | zed's `tooling/lints/test_fixture/gpui` is also `gpui`. Two layers were both named `gpui`, the second lost all its `depends_on`, and `use gpui` picked the fixture | The crate the workspace's dependency table points at wins. Adopt makes layer names unique; validate names duplicate layer names. Target-specific `[target.'cfg(…)'.dependencies]` count too. zed layer-boundary on adopt's own draft: 2,285 → **89** |
| H8 | Boilerplate read as copy-paste | codex: blocks shared by hundreds of files made pairing quadratic | A block shared by more than 10 files is a convention, not a copy |

### Open

| # | Gap | Evidence | Extend |
|---|-----|----------|--------|
| H9 | **P1 · Emitted code read as imports.** | zed's 89 remaining layer-boundary errors are almost all in `gpui_macros`, a proc-macro crate whose `quote! { gpui::… }` bodies are code it *emits*. | Mask `quote!`/`quote_spanned!`/`macro_rules!` bodies from dependency extraction, as test items are masked. |
| H10 | **P1 · Fixture `.rs` files read as source.** | zed `crates/agent/src/tools/evals/fixtures/*/before.rs` (no `mod` reaches them), ruff `resources/test/fixtures`. They produced surface errors. | Source is what a crate target's module tree reaches (`mod` declarations from lib/main/bin/test/bench roots). Unreachable `.rs` files are data: the map lists them as such, and checks skip them. |
| H11 | **P1 · `harna init` rewrites an existing AGENTS.md.** | All three clones: init added ~65 lines to zed's and ruff's hand-written AGENTS.md and created codex's. The readers flagged it. | Init adds only the `<!-- harna:begin/end -->` block (F2), and only when asked (`--agents`). It never rewrites prose, and it offers rather than creates. |
| H12 | **P2 · Cycles at scale are unreadable.** | Real rings after adoption: codex 450, zed 281, ruff 248. Many share one edge (e.g. `cli/factory.rs` in deno, `analytics/events.rs` ↔ `facts.rs` in codex). | Report the minimal set of edges that breaks every ring (feedback edges), with the rings each one breaks, instead of every ring. Pairs naturally with baselines. |

## R. What the readers found (vs deno)

| # | Gap | Evidence | Extend |
|---|-----|----------|--------|
| R1 | **P2 · Banned APIs name their replacement.** | zed `clippy.toml`: `std::process::Command::spawn` → `smol::process::Command::spawn` (`replacement =`). ruff: `std::fs::read_to_string` → `System::read_to_string`. codex: "create SQLite pools through codex-state's sqlite shim". | `banned-api` values take `path => replacement`; the violation names it and `harna deny` writes clippy's `replacement` field. |
| R2 | **P2 · Who may depend on an external crate.** | codex `deny.toml` `[bans]` `wrappers`: `reqwest` only from `codex-http-client` (its "intended owner"), `async-trait` from 7 listed crates. | Gateways for external crates: `gateway = "crate:reqwest"` makes that container the only one whose Cargo deps (and imports) may name it, checked by cargo-layering's dependency reading and rendered into deny.toml `wrappers`. |
| R3 | **P2 · Regenerate-and-diff as a gate.** | zed: workflows say "Rebuild with `cargo xtask workflows`", and CI fails on a dirty `.github`. codex: `schema_tests.rs` compares the generated config schema to the committed one. ruff has `cargo dev generate-all` with a Check mode that **nothing invokes**. | The `generated` rule gains a `check` command. harna's generated CI runs it (harna itself executes no code locally), and `doc-claims`-style drift names generators with a check mode no gate calls. |
| R4 | **P2 · Companion files per crate.** | codex: every crate has a `BUILD.bazel` mirroring Cargo (`codex_rust_crate` macro); `verify_bazel_clippy_lints.py` keeps their lints in sync. zed `package_conformity.rs`: every crate's Cargo.toml has `lints.workspace = true`. | `crate-conformity` rule data: required companion files per crate, and required keys in each crate's Cargo.toml (`lints.workspace`, inherited versions), named per crate. |
| R5 | **P2 · Test data and snapshots as an artifact class.** | 3,734 insta `.snap` files in ruff (CI: `cargo insta test --unreferenced reject`), 1,599 in codex; ruff runs 424 mdtest `.md` specs; fixture inputs in all three. | Test-data kinds in the map (snapshot, fixture, spec-markdown) tied to their tests. `referenced-by` covers orphans; unreferenced snapshots get their own check. Joins E5. |
| R6 | **P3 · Context-scoped rules.** | zed lint `blocking_io_on_foreground` applies to functions taking `&App`, `&mut Context<T>` or `&mut Window`, not to a path. | A rule scope by signature pattern: the inventory already holds signatures, so `banned-api` and `no-panic` can scope to functions whose signature matches. |
| R7 | **P3 · Scoped agent instructions.** | zed: per-crate `.rules` files, and the root `.rules` forbids "architectural descriptions … traps to avoid, not maps to follow". ruff: CLAUDE.md is `@AGENTS.md`. | Render the harna block per container (rules scoped there) into per-directory instruction files; respect a repo's stated "no architecture in agent docs" by rendering rules and commands only. |
| R8 | **P3 · Audited exception lists.** | codex `check_blob_size.py`: 500 KiB cap with an 11-path allowlist. ruff `hawk.toml` `[[production]]` entries each carry a reason. | A file-size (bytes) budget rule whose exceptions are acknowledgments with reasons, as harna's ledger already does for advisories. |
| R9 | **P3 · CI workflow policy.** | zed xtask: workflows declare at most `contents: read`, and no `${{ }}` inside `run:` blocks (expression injection). | Rules over workflow YAML: harna checks the workflows it generates and the repo's own. |
| R10 | **P2 · Path-routed CI** (reinforces F4) | codex `rust-ci.yml` `changed` job, zed `PathCondition`, ruff `determine_changes` — all hand-written. | Already declared for 0.4.0; three of four studied repos hand-roll it. |
| R11 | **Positive: the model holds.** | codex `verify_tui_core_boundary.py` (tui must not import core) is exactly a `layer-boundary` rule; ruff's IO-free semantic crates (0 `std::fs` hits, a `System` trait injected) are exactly a full `fs` gateway on `ruff_db`; zed's `Fs` trait likewise. | Teach these in `place`: "a trait-injected capability + a gateway on its implementing container". |

## Status table: harna on the three repos

| Measure | codex | zed | ruff |
|---------|-------|-----|------|
| Full scan (0.3.46 → study build) | 0.9s | 15s → 1.4s | 7.3s → 1.6s |
| Full validate after adoption | 168s → 14s | 52s → 12.6s | 17s → 6.3s |
| Crates found by adopt | 0 cross-crate edges → 6,679 | 265 layers (2 named `gpui` → unique) | 56 |
| Errors on adopt's own draft (study build) | 450 cycles | 281 cycles, 89 layer-boundary (H9) | 248 cycles |
| Grade before adoption | ungraded (sees 0%) | ungraded | ungraded |
| Grade after adoption | 58 (F, sees 87%) | 60 (D, sees 92%) | 56 (F, sees 92%) |
| Errors on adopt's draft, 0.3.47 | 428 cycles | 255 cycles, 0 layer-boundary | 244 cycles |
| Files read as data (orphans), 0.3.47 | 14 (lint UI fixtures, Bazel-only runners) | 15 (eval fixtures, stale wit copies) | 1 (an uncompiled copy) |

## Status: 0.3.47 (released)

H1–H11 shipped. H9 masks emitted-code macro bodies and treats string
literals as text, which takes zed's layer-boundary errors from 89 to 0.
H10 builds each crate's module tree. On the first pass it misread six
shapes: a crate-named file that is a `mod`, Cargo-declared target paths,
nested inline modules, `r#` names, `#[path]`/`#[cfg_attr(path)]`/
`include!`, and member path dependencies. Each now has a test, and the
remaining orphans on all four repos are genuine data. deno has 68,
mostly `op2/test_cases`. H11 makes `harna init --agents` opt-in.
H12 and R1–R5 remain for 0.3.48.

## Status: 0.3.48 (released)

R1–R5 and H12 shipped, each proven by writing the repo's own convention
as harna data:

| Item | Proof |
|------|-------|
| R1 replacements | zed's `Command::spawn → smol` shape; clippy.toml gets `replacement` |
| R2 `crate:reqwest` | on codex, names exactly deny.toml's three listed exceptions (code-mode, otel, responses-api-proxy) |
| R3 rule proofs | generated CI `proofs` job; ruff-style unproven `generated` rules are advisory |
| R4 crate conformity | zed `lints.workspace = true`: 4 crates lack it |
| R5 snapshots | 0 on codex and ruff (their CI already rejects unreferenced ones); the first pass misread three shapes (lib+main roots, `#[path] pub(super) mod tests`, explicit names), each now tested |
| H12 feedback edges | cycle findings codex 428 → 220, zed 255 → 96, ruff 244 → 123, deno 80 → 75 |

The DFS ring search sees one ring per back edge, so most edges report
"closes 1 ring". A hub like deno's `cli/factory.rs` still shows as many
edges. Grouping feedback edges by module is a candidate for 0.4.0.

## Suggested order

1. **0.3.47:** release the H1–H8 fixes; then H9, H10, H11, and the existing 0.3.47 items (`harna move` completeness, stale-binary warning).
2. **0.3.48:** R1, R2, R3, R4, R5 and H12.
3. **0.4.0** (extends what is declared): R6, R7, R8, R9, R10 alongside G1, E5, A5 and F4.
