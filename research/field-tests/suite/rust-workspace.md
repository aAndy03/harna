# Brief: a Rust workspace, built by layers

You are trying `harna`, a tool that keeps a project's architecture as data
(`.harna/`) and validates code against it. Learn it from `harna --help` and
the README. Build a small product that is real enough to have structure,
and judge whether harna helps.

**What to build.** A Rust workspace (no GUI crates, standard library or
crates already in the local cargo cache, `--offline` if the network fails):
- a domain core with money in minor units;
- a storage layer;
- an application layer;
- two binaries that run as separate services (an API and a worker), which
  share one event type.

**Use harna for the structure.**
- Start with `harna init` (a profile if one fits).
- Use layers and containers, `scaffold`, `scan`, `validate`, `place` and
  `plan` before writing code.
- Use contracts (`contract freeze`), services and a corridor (`service`),
  readiness (`service require/ready`, `preflight`), owners and
  `affected`.
- Before the end, try the MCP server (`harna-mcp`) if your client can
  start it.

## Break list: harna must catch each

| Break | Expected |
|---|---|
| A lower layer imports an upper one | `layer-boundary` (and `no-cycles` when it closes a ring) |
| A Cargo dependency that contradicts the layers | `cargo-layering` |
| A frozen contract's function gains a parameter on a continuation line | `contract` (breaking, with both signatures) |
| Re-freezing that break at a minor version | refused |
| The worker imports the API's code, by `use` or by an inline path | `service-talk` |
| The worker's `Cargo.toml` depends on the API crate, with no import | `service-talk` on the `Cargo.toml` |
| Two services own the same resource (in any case) | refused at write time |
| A required readiness facet unanswered, `n/a` without a reason, `TODO` | preflight `ops` fails |
| A container renamed while a service runs it | the service follows |
| An edit judged right after it is made (`validate --changed`, no scan) | sees the live code |
| A typo'd key in a `.harna/` file (`rationle =`) | `migrate --check` / `doctor` name it |

Note anything a fresh engineer would expect harna to catch that it does
not.
