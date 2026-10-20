# Roadmap re-planned around tools, profiles and plugins (2026-10-20)

The user's direction after the ledger field test: harna is a toolbox (like Figma) for agents and engineers building long-lived software, including fullstack Rust + gpui-kit desktop apps. Product decisions (offline password, license-based auth, sync) belong to the apps built with harna, never to harna's code; framework knowledge ships as versioned plugins that keep parity with the upstream API (gpui-kit 0.8.0 first).

New path: 0.3.50 toolbox discoverability -> 0.4.0 qualities + profiles (first profile: Rust desktop app with gpui-kit) -> 0.4.1 plugin mechanism with gpui-kit as first plugin (proof: radial context menu on a transactions table) -> 0.5.0 TypeScript plugin -> 0.6.0 ownership + contracts -> 0.7.0 services. The old 0.4.0 bundle is distributed: test kinds, file-size exceptions and context-scoped rules to 0.4.0; cross-language boundary and directory-group adoption to 0.5.0; CI routing, per-scope agent files, CI policy and the archived-decisions viewer to 0.6.0.

Transiters are reframed, not deleted: corridors for real boundaries (interface<->backend command bus, service<->service), where the user's original layer-to-layer idea has a job. Persisted as rule tools-not-prescriptions.
