# OpenCode effort goes through the config overlay

OpenCode calls reasoning effort a model "variant", and the TUI horch
launches has no `--variant` flag: OpenCode 1.18.2 swallows it silently. So
`config_env` in `crates/horch-core/src/harness/opencode.rs` sets the level in
the `OPENCODE_CONFIG_CONTENT` overlay as `agent.build.variant`
(`opencode_variant_config`), or returns `None` to leave the env alone. `build`
is the primary agent the TUI starts in, and a variant set there applies to
that agent's model, which is the one horch passes with `--model`. The
overlay is the teammate's config, else the operator's, with the harness
default `config` keys merged under it.

## Consequences

- OpenCode takes its prompt as the `--prompt` flag, not a positional.
- A worker also passes `--auto`, or OpenCode stops on the first permission
  prompt in a pane nobody is watching.
- `model_takes_effort` is false for `opencode/*-free` models and
  `opencode/big-pickle`. These free-tier models report `variants: {}` in
  `opencode models --verbose`, so an effort there is silently a no-op.
- opencode 1.18.34 opens `--session <id>` and ignores `--prompt` (LA-3,
  2026-10-04, re-checked live 2026-10-06, U-64,
  `docs/live-checks/harnesses.md`). So the resume argv carries no
  `--prompt` (`opencode_command`), and the launch types the prompt into the
  pane once the agent is idle. The typed prompt is the 1 delivery path, so
  a later opencode that honours the flag cannot deliver the task twice.
