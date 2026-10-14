# 0.2.3: parity ergonomics (sweep, bulk, debt view)

auto --all sweeps every event with one fresh scan (auto_sweep takes an injected baseline fetcher so tests stay git-free); resolve bulk applies status/note to many targets atomically with missing-target reporting; show gains --open debt view; ack/noop resolutions now render resolved-by + date provenance. Dogfooded on the ledger change itself: sweep auto-verified the 3 importers, one bulk ack closed the residual debt.
