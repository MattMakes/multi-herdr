# A Claude teammate can start clean with `--setting-sources ""`

A claude teammate can start with `--setting-sources ""`
(`Teammate::setting_sources` in `crates/horch-core/src/roster/teammate.rs`),
which loads none of the operator's user, project or local settings: no
`enabledPlugins`, no `permissions.deny`. That is the lever for starting an
agent clean, and it is how a teammate sheds the operator's globally enabled
plugins. `None` omits the flag, so the operator's settings apply as usual.
What a clean start drops by accident is put back or checked explicitly, and
operator defaults that would change a worker's behaviour are overridden.

## Consequences

- Dropping user settings also drops `disableBundledSkills`, so bundled
  skills come back. `setting_sources: []` must be paired with
  `disable_skills` when the intent is "no skills".
  `empty_setting_sources_rules` in `crates/horch-core/src/roster/validation.rs`
  checks this, because "clean" can silently mean "more skills than before".
- `disable_skills` renders `--disable-slash-commands`, which disables all
  skills, including the ones a `plugin_dirs` entry brings in. It is never
  set alongside plugins.
- `plan_mode_rules` requires a plan-mode claude worker to be able to leave
  plan mode, because a worker stuck in plan mode stalls in a pane nobody is
  watching. Claude denies normally beat allows, so naming `ExitPlanMode` in
  `allowed_tools` is not accepted as proof on its own.
- `plan_escapes_settings` takes `setting_sources: []` as the proof: with
  `--setting-sources ""` no settings file loads, so a `permissions.deny`
  written for an attended session (which may deny `ExitPlanMode`) cannot
  reach the worker.
