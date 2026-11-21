# Fleet panes cannot start subagents of their own

A fleet pane must not start a subagent: a subagent's work never reaches the
ledger or the grid, and deciding to split the work belongs to the
orchestrator. A worker that needs more hands sends `QUESTION:` instead.
Workers are the orchestrator's only way to delegate.
`Teammate::allow_subagents` in `crates/horch-core/src/roster/teammate.rs`
holds the rule, and `check` enforces it by insisting on
`disallowed_tools: [Agent]` on every claude teammate the orchestrator can
spawn and on the orchestrator itself.

## Consequences

- `ORCHESTRATOR_DENIED_TOOLS` are the tools the Claude orchestrator must
  deny: `Agent` starts a subagent, in the foreground or the background, and
  `RemoteTrigger` starts a cloud agent.
- The prose rule in `_base/fleet-worker.md` was not enough on its own, so
  the switch carries it (`subagent_switch_rules` in
  `crates/horch-core/src/roster/validation.rs`). `--disallowedTools Agent`
  takes the tool out of the session's tool set on claude 2.1.278.
- The hidden `orchestration-*` teammates run a fixed recipe, not fleet
  panes, so they are exempt. Only a spawnable teammate and the orchestrator
  are covered. Codex, OpenCode, pi and Prime are handled elsewhere.
- `allow_subagents: true` waives the rule for 1 teammate. No shipped
  teammate sets it.
- `catalog::subagent_skill` (`crates/horch-core/src/skills/catalog.rs`)
  rejects operator skills that only work by starting a subagent: Apple's
  `device-interaction` by name, and any other skill whose SKILL.md says
  so. The reason is `None` for a fleet skill.
