# 0.2.12: AGENTS.md by init, agent-instructions doctor check, portable install/config

init now writes the stock AGENTS.md (agents learn the contract at init; idempotent - existing files mentioning harna untouched, blind files get the section appended). Doctor gains agent-instructions check. Install docs: cargo install --git with tags (=releases), private repos via git auth, PATH-based binaries. opencode.json no longer hardcodes absolute paths - repo portable to private branches/machines.
