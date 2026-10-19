# 0.3.47: source is what a crate target reaches; init never rewrites AGENTS.md

Context: the codex/zed/ruff study (H9-H11). zed's eval fixtures (Rust text nothing compiles) and gpui_macros' quote! bodies produced 89 false layer-boundary errors; harna init appended ~65 lines to hand-written AGENTS.md files.

Decision: (1) a .rs file is source only when a crate target's module tree reaches it (tree.rs + tree/decls.rs: mod incl. test-gated/nested/r#, #[path], #[cfg_attr(path)], include!; roots = lib/main, Cargo-declared target paths, tests/benches/examples/src/bin/build.rs, then crate-named files not reached by mod). Unreached files are Digest.orphans, kept out of modules so every map-based check skips them by construction; validate names each once (Info unreachable-module). A crate whose roots the scan cannot see, and files of packages no workspace lists, get no verdict - an unknown is not an orphan. (2) Emitted-code macro bodies and string-literal contents are masked from dependency reading. (3) harna init leaves AGENTS.md alone; --agents adds only the managed block (stock contract only when no file exists).

Why not a path heuristic (fixtures/ dirs)? Proven on four repos: the first tree pass misread six real shapes, each fixed generally; the remaining orphans on codex/zed/ruff/deno are all genuine data. A naming heuristic would have missed zed's src/tools/evals and deno's op2/test_cases.

No-test-update notes: store.rs only lost ensure_agents_md (moved to views::agents::opt_in, tested there); parity.rs only wires policy::breaks, tested in policy's suite.
