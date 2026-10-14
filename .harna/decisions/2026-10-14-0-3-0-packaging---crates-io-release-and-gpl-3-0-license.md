# 0.3.0 packaging: first crates.io release, license -> GPL-3.0

The three crates (`harna-core`, `harna-cli`, `harna-mcp`) publish to
crates.io: workspace metadata gains `readme = "../README.md"` (shared view
of the one README), per-crate keywords/categories, and the path dependency
`harna-core` gets a semver version clause as crates.io requires. The
version crank to 0.3.0 rides the front's completion.

License changes MIT -> `GPL-3.0-only` with the verbatim GNU GPL v3 text as
`LICENSE` at the repo root (packaged into every crate automatically). The
choice was made by the user at release time; the license field without a
file (the pre-0.3.0 state) is not a convention worth keeping.

Deliberately omitted: `repository`/`homepage` (no canonical public URL
exists for this repo yet — add when one is real, not with placeholder
links) and `authors` (optional for crates.io; the git identity is a
placeholder). README install docs gain the crates.io route next to the
git route.
