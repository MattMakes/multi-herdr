# horch reads the codex config with a line scanner and profile files

`ConfigLayers` in `crates/horch-core/src/harness/codex.rs` reads the
operator's codex config as 1 launch reads it: `config.toml`, and for
`-p <name>` the `<name>.config.toml` from the codex home layered over it.
Codex 0.163.0 refuses `-p <name>` while `config.toml` holds
`[profiles.<name>]` or a top-level `profile`, so horch reads neither the
legacy `[profiles.<p>]` table nor the top-level `profile` key.

## Consequences

- `codex_config_value` returns the value of a dotted key (for example
  `tui.auto_recap`) at the top level of a `config.toml` and in its tables.
  A string loses its quotes and a number its `_` separators.
- It is a line scanner like `trust::codex_trust`, not a TOML parser,
  because the project takes no TOML crate.
