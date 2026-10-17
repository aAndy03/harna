# Layers as data: roles, async inventory, role-aware gateways (0.3.25)

The Phase F capstone: the role model stops being prose and becomes
incremental data + checks that interoperate with everything shipped since
0.3.19. THE VOCABULARY IS DATA (`schema/manifest.rs::ROLES` — 8 roles with
one-line meanings: base, engine, gateway, orchestration-sync,
orchestration-async, transit, interface, context); `Layer.role` joins the
schema (serde-defaulted, skip-if-none — pre-0.3.25 manifests load clean),
and a container's OWN role overrides the layer's (`effective_role`;
inheriting wins over inventing). ADOPTION IS DATA-FIRST: an undeclared
role owes nothing, same discipline as `provides` and `gateway`. harna's
own six layers are stamped honestly: memory=base (owns fs + the data
everything reads), search/integrity/construction=engine (sync
computation), agent-context=context (composes briefings), CLI+MCP=interface
(carries the async runtime boundary). THE ASYNC INVENTORY is the map's job
(extractor v3 — maps from older extractors rebuild in full on next scan):
`async fn` declarations become their own inventory kind `async-fn`, pub
and priv, extracted once and read by the gates; nothing re-parses source
text at validate time. THE ROLE-KEYED CHECKS (new `validate-roles`
component, registry 25 -> 27): `role-async` (\u003eerror by default) — one
finding per container whose effective role cannot carry async but whose
modules declare `async fn`s (aggregate, not per-fn noise); allowed roles
are `orchestration-async` and `interface` — engines, bases, gateways,
transiters and sync orchestration stay sync; mapped modules the manifest
homes nowhere are not gated (the map's adoption carve-out). Default-error
is truthful for harna itself: harna-core is 100% sync; the only async lives
in the MCP interface layer. `rules-coverage` (Info — a bound rule may
promote): a declared container no live rule watches (deprecated and
env-mismatched rules stop covering; empty scope = global rule covers all)
is flagged as the advisory island it is. ROLE SCAFFOLDS: `harna scaffold
--role <ROLE>` (CLI + MCP `harna_scaffold` param) validates against the
vocabulary and stamps the container (an unknown role is refused with the
catalog named — data, not invented); `harna transiter` stamps
`role = "transit"` on its corridor container by construction. THE
GATEWAY-OWNERSHIP STORY (this burst collected the deferred register entry
`gateway-ownership-narration`, review due 0.3.25 — entry REMOVED, verdict
folded in): the multi-owner narration is now role-aware — each registered
gateway reads "whose, in what role" (`schema (memory, role base)`,
`briefing (agent-context, role context)`), and the hint states the formal
ownership rule: a facility MAY have several registered gateways when the
owners' roles differ, and the deliberate split is said out loud with the
container `role` beside the `gateway` declaration. fs keeps four owners
here (memory=base the data owner, integrity/generation=engines writing
their own ledgers, briefing=context reading git for release notes) — the
narration now explains WHY that is deliberate instead of advising a
move. THE DEFERRED ENTRY IS COLLECTED (removed from the register;
`harna deferred remove`): no prose deferral survives the capstone.
Dogfood: extractor v3 rebuilt the whole map (57 modules re-scanned), the
role stamping engaged `role-async` zero times and `rules-coverage` zero
times (both rules scope `crates` = every container watched), the role-aware
gateway Info renders the four-owner narration with roles, and tests went
217 -> 225 (+8: role serde/inheritance, async inventory, 5 role-gate
fixtures incl. override/carry/coercion, scaffold role stamp + refusal;
check-registry count 25 -> 27 pinned).
