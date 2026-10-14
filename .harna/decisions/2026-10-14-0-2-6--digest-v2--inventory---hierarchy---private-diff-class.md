# 0.2.6: digest v2 (inventory + hierarchy + private diff class)

Module gains inventory (every declared item pub/private with kind+name, impls included) and mods (declared submodules). pub(crate) is treated as a pub-boundary form — the first build parsed its lid as private (strip bug: 'pub(' vs 'pub '), pinned by test. render_tree exposes the hierarchy (harna tree). diff gains a private items_changed class, informational only; fan-out stays public-driven by design. Digest.json remains one diffable artifact; hierarchy encoded as data.
