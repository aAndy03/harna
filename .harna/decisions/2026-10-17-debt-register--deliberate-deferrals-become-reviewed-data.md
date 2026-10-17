# The debt register: deferrals as reviewed data (0.3.23)

`harna deferred add` turns a deliberate deferral into tracked data -
`.harna/deferred.toml` (schema_versioned, memory-layer owner like every
persisted document; the data structs live in `deferred-data`, the same
pattern as `digest-data` and `parity-data`): what collecting would touch
(component - free-form, NOT manifest-validated, the register is a map's
pointer), why it is deliberate (reason), when to revisit (review: ISO
date or version the tool must reach - zero-padded numeric compare, a
version is due once the shipped tool arrives at it, and the trigger day
itself is DUE - owed now), plus the recorded date for provenance. This
register is distinct from parity debts by construction: parity is
unresolved change propagation the structural tool promises; the register
is a promised reconsideration the humans and agents chose. It surfaces
where you would look anyway - status says `- deferred register: N open`
(and names OVERDUE loudly), and score charges one `DEBT_COST` from the
debts part per entry past its review, the same weight class as an
unresolved parity target, saturating (a register out of control can
only zero the part, never underflow it); unparseable review specs are
never overdue - an unpinnable date cannot demand a review. Old score
snapshots parse with the new field defaulted. No new check id was
added on purpose: the overdue cost rides the existing debts part (the
registry's checks judge code; the register judges promises), and
removal is removal - a collected or re-evaluated deferral leaves the
ledger, its story belongs to the decision log. The register's first
three entries are its own dogfood: the standing prose deferrals from
prior decisions became reviewed data - fixture-builder-dedup (0.4.0),
archived-decisions-viewer (0.4.0), gateway-ownership-narration
(0.3.25, folded into the layers-as-data capstone instead of a loose
paragraph). Score honesty, recorded on the ledger rather than tuned
away: mid-burst the score read 64 with the regression flag on the
STALE map; the fresh scan settled it to 67 (= last) - and the shifts
inside the parts are real and named: coverage ticked up with the
register tests (47/52), `schema/manifest.rs` crossed the coupling
outlier threshold as the digest grew (6 outliers - the project's true
hub now visible under the mean+2 sigma statistic), boundary pressure
sits at 75 percent with briefing at 98. Truer measurements, not worse
code - exactly the class of the 0.3.19 cross-crate fix.