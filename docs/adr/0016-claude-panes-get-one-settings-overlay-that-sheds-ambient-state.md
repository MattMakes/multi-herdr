# Claude panes get 1 settings overlay that sheds the operator's ambient state

Every Claude Code pane gets 1 inline JSON object through `--settings`,
laid over the operator's settings. claude's `--settings` takes a path or a
JSON string and is not repeatable, so `push_settings_arg` in
`crates/horch-core/src/harness/claude.rs` assembles everything horch
overlays into 1 object: plugins, skill switches, status line, sandbox,
horch paths and the compaction window. Claude Code merges settings per
key, so the overlay switches off the operator's plugins and skills for
that session and touches nothing else. A worker starts from what its
teammate file names, not from what the operator has switched on.

## Consequences

- `disable_operator_plugins` switches each globally-enabled operator
  plugin off by name, unless the teammate inherits plugins. A live launch
  showed "Found 3 plugins (0 enabled, 3 disabled)" with every other
  setting intact.
- `default_overlay` always switches off the operator's claude.ai skills,
  whatever `inherit_plugins` says. The generic tiers inherit plugins, but
  no pane has a use for the operator's claude.ai skills.
- `overlay_skill_switches` is shared by the plain overlay and
  `skills_settings`, the path fleet panes take. The skills synced from
  claude.ai (`anthropic-skills:<name>`) go off unless the teammate opts
  back in. `syncClaudeAiSkills: false` given through `--settings` hides
  them for this session only and moves nothing on disk. The same value in
  the operator's settings.json would trash the on-disk cache.
- Each `disabled_skills` entry goes off by name in `skillOverrides`, next
  to the synced-skills switch. The overlay does not restate the operator's
  own `skillOverrides`: they still apply by themselves, because settings
  merge per key (verified on a live launch, Claude Code 2.1.276). The
  orchestrator's `skillOverrides` carries its own 2 entries and the
  ambient skill-creator copies, nothing else. The operator's
  `explain-diff-notion` override is not copied in.
- A plugin skill ignores `skillOverrides` in every key form (tested live,
  Claude Code 2.1.283). `disable_skill_plugins` therefore switches the
  official `skill-creator@claude-plugins-official` plugin off as a plugin, and a disabled
  `<plugin>:<skill>` switches its whole plugin off (Claude Code 2.1.289).
  It uses `or_insert`, so what `plugin_skills` set stays. Its filtered copy
  is keyed `<plugin>@inline`, a key this never names, so it stays on.
- The ambient copies of skill-creator (`AMBIENT_SKILL_CREATOR`: the user
  or project `skill-creator` and `anthropic-skills:skill-creator`) go off
  in every pane. The orchestrator carries its own as `horch:skill-creator`
  and no worker has one.
- `remoteControlAtStartup` is stated in every pane, set to the teammate's
  `remote_control` value (Claude's Remote Control bridge): `true` for a
  teammate that sets it, `false` everywhere else. Remote Control belongs
  to the orchestrator, and an operator or organization default of "on"
  must never reach a worker. Only an orchestrator may set
  `remote_control`, and `check` enforces that.
- Restricting settings would drop the status line too, and
  `--setting-sources ""` sheds it with everything else. `operator_status_line`
  (`crates/horch-core/src/roster/operator.rs`) reads the operator's
  `statusLine` block from `~/.claude/settings.json`, and
  `copy_operator_status_line` copies it into the overlay, so every pane,
  orchestrator and workers alike, keeps it and a worker reads like any
  session. A pane with no status line gives no model, no context usage and
  no session cost, and a fleet is where that information matters most.
  The `home` parameter of `operator_status_line` is the operator's home
  directory, and `None` means it is unknown.
