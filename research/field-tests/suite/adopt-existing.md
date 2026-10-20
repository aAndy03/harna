# Brief: adopt an existing repository

You are trying `harna`, a tool that keeps a project's architecture as data
(`.harna/`) and validates code against it. Learn it from `harna --help` and
the README. Bring it to a large existing repository and judge whether it
helps a team that did not start with it.

**Where.** Use the kept clone of `codex`: the `codex-rs` workspace under
the scratchpad's `codex` folder. Work on a branch, and restore the clone
to `main` with no leftover files when you are done. If it is gone, clone
another mid-size Rust or TS repository (shallow), and delete it at the
end.

**What to do.**
1. Adopt: `harna adopt --by-dir` with `--exclude` for fixtures. Then
   `scan`, `validate` and `doctor`, and time each one.
2. Ownership: `container set --owners`, `harna owners --changed --base
   main`, and `render` (CODEOWNERS, with a hand-written one in the way).
3. Contracts: freeze a widely used crate's surface. Simulate a breaking
   change in a function other crates import by crate name, then run
   `parity open`, `affected --base`, `dependents` and `validate`.
4. Migrations: `harna migrate --check`. Hand-edit a file to an older or
   newer stamp and see what is said.

## Break list: harna must catch each

| Break | Expected |
|---|---|
| A breaking change to a function other crates import as `krate::item` | parity targets each importing crate, and only those naming it |
| A function re-exported from a submodule (`pub use`) | importers through the root are found |
| The same break, judged before any `scan` | `validate --changed` sees it live |
| `affected --base main` after a change to a widely used crate | the importing containers, the crates depending on it (transitively), and the services to redeploy |
| A lockfile or a root `Cargo.toml` change | `affected` widens to everything and says why |
| A `.harna/` file from "the future" (`schema_version = 99`) | refused, never misread |
| A `.harna/` file with an unknown key | named by `migrate --check` and `doctor` |
| Owners on a container | `harna owners` covers the crate root (`Cargo.toml`, `tests/`) |

Also time the scan, a full `validate` and a one-file `validate --changed`.
The targets are under 1 minute and under 2 seconds.
