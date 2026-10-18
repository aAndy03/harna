# no-panic is opt-in by rule binding, unreachable! stays open (0.3.42)

Context: the font-manager agent wrote a 'no unwrap in backend' rule that nothing could enforce. Decision: a no-panic check that runs only where a non-deprecated rule binds check = "no-panic" (its scope selects files); default severity error. unwrap/expect/panic!/todo!/unimplemented! count; unreachable! documents an invariant and stays open. Rejected: on by default (most repos legitimately unwrap in bins and build scripts — a default-on gate would teach agents to acknowledge it away).
