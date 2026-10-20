# Brief: a TypeScript app

You are trying `harna`, a tool that keeps a project's architecture as data
(`.harna/`) and validates code against it. Learn it from `harna --help` and
the README. Build a small TypeScript product and judge whether harna
helps.

**What to build.**
- A TypeScript app with a domain folder, a UI folder and a client for a
  server.
- Use `tsconfig.json` `paths` aliases (`@domain/*`) and at least one
  workspace package (`packages/shared`).
- No `npm install` is needed: harna only reads the files.
- If it fits, add a small Rust server crate. Its events are mirrored as TS
  interfaces in the client: two services joined by a corridor.

**Use harna for the structure.**
- `harna init` (the `tauri-react` profile if you build a tauri shape),
  then containers over folders and layers with `depends_on`.
- Gateways (`--gateway npm:axios`) and declared surfaces (`provides`).
- `scan`, `validate`, `dependents`, `affected --base`.
- Services with a corridor whose message shape is the Rust struct.

## Break list: harna must catch each

| Break | Expected |
|---|---|
| The domain imports the UI (relative path, then through a `@` alias) | `layer-boundary` |
| A UI file imports `axios` while a gateway owns it | `gateway-ownership` |
| A file imports another container's private module | `declared-surface` |
| The TS mirror of a corridor message renames a field, adds one, drops one | `corridor-shape` (missing / extra) |
| The mirror `extends` a base interface holding some fields | no false positive |
| A class mirror with constructor parameter properties | its fields read |
| A shape as a JSON Schema file with `properties` | compared against the mirror |
| The client imports the server service's code directly | `service-talk` |
| A deleted file that others imported | `affected` widens, and says why |

Note anything a fresh engineer would expect harna to catch that it does
not.
