# harna migrate: data stamped with the schema it needs

`harna migrate` upgraded .harna/ data files:
- templates.toml: v0 -> v1
- deferred.toml: v0 -> v1
- non-goals.toml: v0 -> v1
- lessons.toml: v0 -> v1
- qualities.toml: v0 -> v1
- roadmap.toml: v0 -> v1
- acknowledged.toml: v0 -> v1
- commands.toml: v0 -> v1
Backups: .harna/backup/ (each verified readable before the transform touched the live file). Old harna versions may refuse to read these files.
