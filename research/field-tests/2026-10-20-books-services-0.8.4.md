# Field test: harna 0.8.3, "the 0.8 proof" (books084)

Date: 2026-10-20 · Windows 11, Git Bash · harna 0.8.3 · about 50 minutes

## What I built

A plain Rust workspace with no external crates, built with `cargo build --offline` in 1.4 s, plus a TypeScript folder:

| path | container (layer) | service |
|---|---|---|
| crates/money (Money in minor units, Posting) | money (domain) | shared |
| crates/events (PostingCreated with to_line/from_line) | events (domain) | shared |
| crates/ledger-api (lib+bin: Ledger, publish to outbox.txt) | ledger (app) | api, owns db:ledger |
| crates/sync-worker (bin: reads outbox, prints) | sync (app) | sync, owns queue:sync, db:sync-state |
| web/src/events.ts (interface mirror), feed.ts | webclient (web) | web |
| crates/refunds (added later, consumes PostingCreated) | refunds (app) | refunds, owns db:refunds |

The layer `app` depends_on `domain`. The corridor is `postings` (event): api -> sync, web, carrying `PostingCreated=crates/events/src/lib.rs::PostingCreated`. The required facets are health, backups and migrations, and every service answers them. Both binaries ran: `cash balance: 1250` / `synced posting 1 for cash`.

## Commands used, and whether they helped

| command | helped? | notes |
|---|---|---|
| `harna init` | yes | Quiet and clean. It does not suggest `harna adopt` even when Cargo crates already exist. |
| `harna adopt` | partly | Drafted 5 layers, one per crate. That is a sensible draft, but it is print-only, so I declared everything by hand instead. |
| `harna container add … --layer … --path …` | yes | Creates the layer on the fly and prints the `layer set --depends-on` hint. Very smooth. |
| `harna layer set app --depends-on domain` | yes | |
| `harna scan` | yes | **0.19–0.25 s** (7–9 modules), incremental. |
| `harna validate` / `--changed` | yes | **1.08–1.26 s** full. `--changed` still reaches unchanged TS mirrors and services.toml-wide problems (good). Drift is detected even with a stale map, with a "map is stale" warning. |
| `harna service add/set/remove/show`, `harna services` | yes | Clear one-line output, and the "shared (no service runs them alone)" line is useful. `service show <corridor>` also works. |
| `harna service corridor add/remove` | yes | Refuses unknown services, but accepts everything else (see the bugs below). |
| `harna service require` / `ready` | yes | Preflight `ops` line is crisp and lists exactly what is missing. Unknown facets only get a note. |
| `harna preflight` | yes | **1.38 s.** `[FAIL] ops: unanswered: api/health, …` → PASS after the answers. |
| `harna affected` | partly | 0.35 s. Containers and crates to test are correct, but it says nothing about services or corridors. |
| `harna owners` / `--changed` | no | Ignores service `--owners` (bug 1). With no args it prints "no paths to route". |
| `harna digest` | mostly | Lists services and corridors. It has a newline bug and does not show readiness. |
| `harna status`, `harna doctor` | yes | Doctor has a `services` line. Status does not mention services. |
| `harna plan "<new service idea>"` | weak | 0.73 s. It is not service-aware: it never proposes `service add`/`corridor add`, and it lists service code (ledger, sync) as "candidates to extend". |
| `harna render`, `harna graph` | weak for 0.8 | ARCHITECTURE.md and the graph show no services, corridors or readiness. |

## Break attempts

The legend is CAUGHT / MISSED / CLI-REFUSED (refused at write time). Outputs are trimmed to the relevant lines. The info-level `rules-coverage` and `deny-sync` lines are omitted.

### 1. Cross-service import (service-talk)

- **1a CAUGHT.** `use ledger_api::Ledger;` in sync-worker/main.rs, with a path dep:
  `[!service-talk] crates/sync-worker/src/main.rs — service `sync` imports `crates/ledger-api/src/lib.rs`, code service `api` runs (`ledger_api::Ledger`) — services talk through corridors` / `fix: send it through postings instead; a shape both need moves to shared code`. The verdict is BLOCKED with exit 2, and `validate --changed` gives the same result.
- **1b CAUGHT.** An inline qualified path `ledger_api::Ledger::new()` with no `use`, both before and after a rescan.
- **1c MISSED.** The `ledger-api = { path = "../ledger-api" }` dependency in sync-worker/Cargo.toml with no code using it gives `verdict: PASS`. The deploy unit still links the other service's crate.
- **1d CAUGHT.** `pub use ledger_api as l;` in a new submodule alias.rs: `[!service-talk] crates/sync-worker/src/alias.rs … (`ledger_api`)`.
- **7b CAUGHT.** In TS, admin/src/panel.ts imports `../../web/src/feed` and `import type … from ../../web/src/events`: two `[!service-talk] admin/src/panel.ts — service `admin` imports `web/src/feed.ts`…` errors.

### 2. TS mirror drift (corridor-shape)

- **2a CAUGHT.** Renaming `amountMinor` to `amountCents`: `[!corridor-shape] web/src/events.ts — service `web`'s `PostingCreated` differs from corridor `postings`'s shape (crates/events/src/lib.rs): missing [amount_minor], extra [amountCents]`
- **2b CAUGHT.** An extra `memo: string`: `… missing [], extra [memo]`. The empty `missing []` is cosmetic noise.
- **2c CAUGHT.** A missing `postedAt`: `… missing [posted_at], extra []`
- **2d MISSED.** Changing a type from `amountMinor: number` to `amountMinor: string` gives `PASS`. Only field names are compared.
- **2e CAUGHT.** A `type PostingCreated = {…}` alias with drift is caught.
- **2f CAUGHT (debatable).** An optional extra `memo?: string` is flagged as an extra field. A consumer can tolerate an optional extra, so this could be info.
- **2g CAUGHT as info, after a scan only.** If the interface is renamed to `PostingEvent`, you get `[?corridor-shape] … service `web` is on corridor `postings` but its code never names `PostingCreated``. Before a rescan there was no info at all, while drift is read live. Inconsistent.
- **2h FALSE POSITIVE.** `interface PostingCreated extends Base { … }` where Base holds id and account: `… missing [id, account], extra []`. Inherited fields are not followed.
- **2i FALSE POSITIVE plus imprecise.** For `class PostingCreated { constructor(public id: number, …, public posted: string) {} }` it reports `missing [id, account, amount_minor, posted_at]`. Parameter properties are not read, so the one real drift (`posted`) is lost among false ones.
- **2j CAUGHT.** On the Rust side, renaming `amount_minor` to `amount` in events gives the TS error `missing [amount], extra [amountMinor]`. **9b** shows `validate --changed` also catches this even though web/src/events.ts itself is unchanged.
- **6e CAUGHT.** A local Rust mirror struct in sync with `amount` instead of `amount_minor`: `[!corridor-shape] crates/sync-worker/src/main.rs — … missing [amount_minor], extra [amount]`.
- **6f MISSED.** An enum shape `pub enum LedgerEvent { Created{..}, Voided{..} }` against a TS discriminated union with drifted `why` gives PASS. There was also no "never names" info for api, so non-struct shapes are silently skipped.

### 3. Dangling references

- **3a CLI-REFUSED.** `harna service corridor add refunds --kind queue --from api --to billing …` → `error: no service `billing` (harna service add first)`, exit 1.
- **3b CAUGHT at validate, but the CLI accepted it.** With a message type that does not exist (`…::Refund`), you get `added corridor refunds` with exit 0, then `[!service-coherence] … message `Refund` names `Refund`, which `crates/events/src/lib.rs` does not declare`.
- **3c CAUGHT at validate, but the CLI accepted it.** With a shape file that does not exist, you get `[!service-coherence] … names `crates/nope/src/lib.rs`, which the map does not hold`.
- **3d CAUGHT.** Hand-editing a corridor to add `billing`: `[!service-coherence] .harna/services.toml — corridor `postings` joins `billing`, which is not a declared service`. `validate --changed` catches it too.
- **3e CAUGHT at validate, but the CLI was silent.** `harna service remove web` → `removed service `web``, exit 0, with no warning that corridor `postings` still names it. Validate then reports `[!service-coherence] … corridor `postings` joins `web`, which is not a declared service`.
- **3f CAUGHT at validate, but this is a bug.** `harna container rename ledger ledger-core` does not relink services.toml (`containers = ["ledger"]` remains). Validate: `[!service-coherence] … service `api` runs container `ledger`, which the manifest does not declare`, plus a knock-on info: `service `api` is on corridor `postings` but its code never names `PostingCreated``.
- **3g/3h CAUGHT at validate, with a silent CLI.** `harna container remove sync` and `harna layer remove app` succeed without saying that a service runs those containers.

### 4. Ownership

- **4a CAUGHT.** `harna service set sync --owns queue:sync,db:sync-state,db:ledger` → `[!service-coherence] … resource `db:ledger` is owned by 2 services: api, sync`. The same happens through `service add audit --owns db:ledger`, and the CLI accepted both.
- **4b CAUGHT.** `harna service set web --containers webclient,ledger` → `[!service-coherence] … container `ledger` runs in 2 services: api, web`
- **4c MISSED.** `--owns queue:sync,DB:Ledger` while api owns `db:ledger` gives PASS. Resource identity is case-sensitive.
- **4d MISSED.** `--owns ledger` with no kind is accepted, even though help says the format is `kind:name`.
- **4e CAUGHT, with misleading advice.** If the shared `events` container is claimed by api (`--containers ledger,events`): `[!service-talk] crates/sync-worker/src/main.rs — service `sync` imports `crates/events/src/lib.rs`, code service `api` runs (`events::PostingCreated`)`, `fix: send it through postings instead`. That type IS the postings shape, so the advice is circular.
- **4f CAUGHT.** A `--runs crates/sync` that does not exist: `… deploys from `crates/sync`, which does not exist`.
- **4g CLI-REFUSED.** A duplicate service name gives `error: service `api` exists`.
- **4h MISSED.** `harna service set sync --runs crates/ledger-api`, so sync deploys from api's crate, gives PASS. `runs` is not cross-checked against the paths of the service's containers.
- **4i MISSED.** `harna service add ghost --runs crates/ledger-api` declares a service with no containers that shares a deploy folder with api, and gives PASS.

### 5. Readiness

- Before `require`, preflight shows `[PASS] ops: no readiness facets required`.
- After `harna service require health,backups,migrations` (which prints `9 answer(s) missing`), preflight shows `[FAIL] ops: unanswered: api/health, … web/migrations — harna service ready <service> <facet> "<where, or n/a: why>"` and `preflight: FAIL`, exit 2. **CAUGHT.**
- After answering, it shows `[PASS] ops: 3 service(s) answer health, backups, migrations`.
- Adding a new service (refunds) turns preflight back to FAIL with `unanswered: refunds/health, …`. **CAUGHT.**
- `""` and `"  "` remove the answer, and preflight fails again (good).
- **MISSED.** `"n/a"` with no reason, `"n/a:"` with an empty reason, and `"TODO"` are all accepted as answers, and preflight PASSes. The documented form is `n/a: why`.
- **MISSED.** Path-looking answers (`ops/backup-ledger.sh`, `crates/ledger-api/migrations`) are not checked for existence. Neither file existed.
- Unknown facet `harna service ready web uptime x` is accepted, with the note `uptime is not a required facet`. That is fine.
- Unknown service gives `error: no service `billing``.

### 6. Other natural moves

- **6a CAUGHT, with misleading advice.** The shape `LedgerClosed` lives inside api's crate (`pub mod wire`) and is used by sync: `[!service-talk] … (`ledger_api::wire::LedgerClosed`)`, `fix: send it through postings, closing instead`. The type is corridor `closing`'s own declared shape. Better advice would be "corridor `closing`'s shape lives in service `api`'s code; move it to shared code". Corridor add also accepted a shape that lives inside a service's container without any note.
- **6b MISSED.** A JSON Schema shape (`--message AccountOpened=schemas/account-opened.schema.json`, with `account` and `openedAt`) was not compared with the mirrors. TS `openedOn` and Rust `opened_at, owner` both PASS, and there was no "never names" info either.
- **6c CAUGHT.** A schema file that does not exist: `… shape `schemas/nope.json` is not a file`.
- **6d MISSED.** A schema file holding invalid JSON (`{ not json`) is accepted.
- **7a MISSED.** The new service `refunds` reads `PostingCreated` but is not on corridor `postings`, so it is an undeclared consumer, and validate PASSes.
- **8a MISSED.** The corridor `loop` api -> api is accepted.
- **8b.** A corridor with no `--message` is accepted. That is maybe fine for `api` kinds, but nothing notes it.
- **8c CAUGHT at validate, but the CLI accepted it.** Without `--to`, the CLI prints `added corridor half (api): web -> ?` and validate reports `[!service-coherence] corridor `half` needs a sender and a receiver (from: 1, to: 0)`.
- **8d MISSED.** The same message name twice in one corridor with different shapes (`PostingCreated=…events::PostingCreated` and `PostingCreated=…money::Posting`) is accepted: `carrying PostingCreated, PostingCreated`.

## Bugs (exact repro)

1. **Service `--owners` is not used by `harna owners`.** Run `harna service set api --owners "@ledger-team"`, then `echo '// x' >> crates/ledger-api/src/lib.rs; harna owners --changed`. The output is `unowned — 1 path(s) (harna container set <c> --owners @team): crates/ledger-api/src/lib.rs`. `harna owners crates/ledger-api/src/lib.rs` gives the same result.
2. **`harna container rename` does not relink services.toml.** Run `harna container rename ledger ledger-core`. services.toml keeps `containers = ["ledger"]`, so validate reports `service-coherence` plus a knock-on `corridor-shape` info. Rename advertises relinking, so it should carry services along.
3. **Digest has a missing newline.** `harna digest` prints `No rules recorded for this scope yet.services (each runs its containers alone; …)`.
4. **The "never names" info depends on a rescan, but drift is read live.** Rename the TS interface without `harna scan`: validate shows nothing. After a scan it shows `[?corridor-shape] … never names PostingCreated`.
5. **corridor-shape false positives.** TS `interface X extends Base` (inherited fields are reported missing), and TS `class` with constructor parameter properties.
6. **Non-struct shapes (enum) and schema-file shapes are silently skipped by corridor-shape.** There is not even an info saying the shape cannot be compared.

## Friction

- The CLI validates inconsistently at write time. `corridor add` refuses an unknown service but accepts a missing type, a missing file, no `--to`, from == to, duplicate message names, and invalid JSON. `service remove`, `container remove/rename` and `layer remove` succeed silently while a service or corridor still references the item. Each one is only discovered at the next validate.
- The service-talk advice "send it through <corridor> instead" is circular when the imported type IS that corridor's shape (4e, 6a).
- The name "corridor" is overloaded: transiter corridors (layers) versus service corridors. The digest's built-in "concepts" glossary defines transiter=corridor and has no entry for service, service corridor, facet or readiness.
- `affected`, `plan`/`place`, `render` (ARCHITECTURE.md), `graph` and `status` are not service-aware.
- In the corridor-shape message, `missing [], extra [memo]` prints empty lists.
- `harna owners` with no args prints "no paths to route". It could default to a per-owner overview.
- `harna init` on a repo with existing crates does not point to `harna adopt`.

## Suggestions, in priority order

1. **Make `container rename/remove`, `layer remove` and `service remove` service-aware.** Rename should relink `services.toml` containers. Removes should warn or refuse while a service or corridor references the item, or offer `--cascade`.
2. **Feed service `--owners` into `harna owners`** as owners of the service's container paths, at the same precedence as container owners. Add an `owners` no-arg overview.
3. **Run the same checks at write time in `corridor add` that `service-coherence` runs** (type exists, file exists, from/to non-empty, from ∩ to = ∅, unique message names, schema parses), and in `service set --owns` (`kind:name` format; same-resource check case-insensitive).
4. **Extend corridor-shape:**
   - Compare field types, at least on a primitive mapping (i64/u64 → number, String → string).
   - Follow TS `extends` and class parameter properties.
   - Compare JSON Schema `properties`/`required` for schema shapes.
   - Emit an info when a shape kind (enum/union) cannot be compared, instead of skipping silently.
5. **Flag undeclared consumers and producers as info.** If a service names a corridor's message type but is not on that corridor (7a), suggest `corridor add … --to refunds`.
6. **Fix service-talk advice when the imported item is itself a corridor's declared shape:** "move `LedgerClosed` (shape of corridor `closing`) to shared code". Also add an info when a corridor's shape lives inside a service's container.
7. **Hold readiness answers to the documented form.** Require `n/a:` to carry a non-empty reason, treat `TODO`/`tbd` as unanswered, and check that path-looking answers exist, or at least warn.
8. **Cross-check `runs`.** The service's containers should live under its `runs` folder, two services should not share one `runs` folder, and a service with no containers should get an info. Add a Cargo.toml path-dependency check across services (1c).
9. **Make affected, plan, render, graph and digest service-aware.** affected should report services to redeploy and corridors whose shape changed. plan should propose `service add`/`corridor add` and skip service code as an extension candidate. ARCHITECTURE.md and the graph should show services and corridors. The digest should show `require` and which answers are missing. The glossary should gain service, corridor (service) and facet entries.
10. Fix the cosmetic issues: the digest newline and the empty `missing []`.

## What worked well

- Every primary break in the brief (1, 2, 3, 4, 5) is caught with an error that names both sides and gives a fix line.
- service-talk catches `use`, qualified paths, `pub use` aliases, and relative TS imports, including type-only ones.
- corridor-shape normalises case and `_` (`posted_at` = `postedAt`), and catches Rust-side renames against an untouched TS mirror even under `--changed`.
- The readiness gate is simple and effective, and a newly added service immediately turns preflight to FAIL.
- Speed: scan is about 0.2 s, validate about 1.1–1.3 s, preflight about 1.4 s, affected 0.35 s, and plan 0.73 s.
