# harna documentation

| Page | What it covers |
|---|---|
| [Command reference](reference/cli.md) | Every `harna` command, its help and its options |
| [MCP reference](reference/mcp.md) | Every MCP tool with its parameters, and every resource |
| [Checks](reference/checks.md) | Every check a rule can bind: default severity, what it takes, what it holds |
| [The toolbox](reference/tools.md) | Every tool harna offers: what it does, when to reach for it, how |
| [Profiles](reference/profiles.md) | The starting shapes shipped with harna: layers, rules, next steps |
| [Plugins](reference/plugins.md) | The framework packs shipped with harna: rules, capabilities, bridges |
| [The `.harna/` format](FORMAT.md) | Every data file, its fields, and how it is versioned and migrated (frozen at 1.0) |
| [The plugin interface](PLUGINS.md) | How to write a plugin pack: layout, keys, stability (interface 1, stable at 1.0) |
| [Field-test suite](../research/field-tests/suite/README.md) | How every release is tested by fresh agents on three project types |

The reference pages are generated from the data and code that define
them, and the tests fail when a page falls behind:
- `harna-core`'s `reference` test covers checks, tools, profiles and
  plugins;
- `harna-cli`'s `reference` test covers the commands;
- `harna-mcp`'s `reference` test covers the MCP server.

To refresh them after a deliberate change, run `HARNA_BLESS=1 cargo test`.
