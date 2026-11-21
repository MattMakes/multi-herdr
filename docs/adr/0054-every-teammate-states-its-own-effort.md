# Every teammate states its own effort

A teammate's effort is written in its own file, never left to a CLI default
or the operator's configuration. horch updates every CLI before each fleet,
and a CLI's default effort can change between versions, so a teammate's
tier must not depend on a CLI default. The checks live in
`crates/horch-core/src/roster/validation.rs` and
`crates/horch-core/src/roster/effort.rs`.

## Consequences

- `codex_effort_rules` requires every offered codex teammate to set
  effort. A codex worker with no effort silently takes
  `model_reasoning_effort` from the operator's `~/.codex/config.toml` (the
  pane's private `CODEX_HOME` links it). That once made 2 workers run at
  "medium" effort nobody chose.
- `antigravity_rules` requires every antigravity teammate to state its
  effort. A model id with the level in it (`gemini-3.8-flash-low`) states
  it too.
- `model_takes_effort` decides whether a model has any effort setting at
  all on an agent, and each harness module owns its rule. Claude's Haiku 4.5
  has none, so claude rejects `--effort` for it. The OpenCode free-tier
  models report `variants: {}` (`opencode models --verbose`), so an effort
  there is silently a no-op. horch rejects effort for such models, because
  a file that claims a setting that is not happening is worse than an
  error.
- Claude Code resolves effort in this order: env
  `CLAUDE_CODE_EFFORT_LEVEL`, `--effort`, settings, model default
  (cezaar#41). horch passes `--effort`, so only the env var beats it, set in
  the shell or in the `env` block of `~/.claude/settings.json`, which Claude
  Code exports into its own process. It beats it for every pane at once.
  `maxEffortLevel` caps every level above it. A codex pane with no effort
  takes `model_reasoning_effort` from the operator's `config.toml`.
  `effort_override_warnings` (`crates/horch-core/src/roster/operator.rs`),
  the pure half of `operator_effort_warnings`, warns about these overrides.
