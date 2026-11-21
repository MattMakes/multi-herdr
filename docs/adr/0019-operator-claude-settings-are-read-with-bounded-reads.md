# The operator's Claude settings chain is read with bounded reads

`read_claude_settings` in `crates/horch-core/src/harness/claude.rs` reads
the operator's settings chain: the managed file
(`RuntimeContext.paths.claude_managed_settings`, none when that is
`None`), the user file `<config_dir or home/.claude>/settings.json`, and
the project files. The project files, such as `.claude/settings.json`,
belong to the repository, which is not trusted. Each read is bounded: a
FIFO, a device or a file over 1 MiB counts as absent, so a hostile
repository cannot block or bloat a launch.

## Consequences

- `claude_operator_window` finds the operator's auto-compact window over
  these settings texts (design §6.4). It takes the first source of the
  chain whose `env` sets `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, else the
  process env.
- `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` makes Claude Code read the window as a
  percentage, so the trigger is unknown. A file whose `env` also sets it,
  or a window value that is not a number, gives `tokens: None`.
