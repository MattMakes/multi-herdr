# F3 skill-overrides: disabled skills stay hidden in Claude Code

Plan: `ai_docs/plans/finish/f3-skill-overrides.md`. Worker: opus-63. Date: 2026-10-04.
Claude Code: 2.1.289.

## Result

A Claude pane launched by horch no longer lists `herdr:herdr-worker` or
`herdr:herdr-orchestrator` when its teammate disables them. horch now
switches the whole `herdr` plugin off for that session.

## Source of the skills

Two sources load the herdr skills. Each needs its own switch.

| Listed as | Source | What hides it |
| --- | --- | --- |
| `herdr:herdr-worker`, `herdr:herdr-orchestrator`, `herdr:herdr-atomic` | Plugin `herdr@matts-robot-skills`, enabled in `~/.claude/settings.json`, installed at `~/.claude/plugins/cache/matts-robot-skills/herdr/1.0.0` | `enabledPlugins: {"herdr@matts-robot-skills": false}` only |
| `herdr-worker`, `herdr-orchestrator` | User skills: the symlinks `~/.claude/skills/herdr-worker` and `~/.claude/skills/herdr-orchestrator` into `~/projects/matts-robot-skills/plugins/herdr/skills/` | `skillOverrides: {"herdr-worker": "off", ...}` (worked before this change) |

The docs (https://code.claude.com/docs/en/skills, section on
`skillOverrides`) say: "Plugin skills are not affected by `skillOverrides`.
Manage those through `/plugin` instead." The keys are bare skill names. No
`plugin:skill` key form exists. A `skillOverrides` entry given through
`--settings` applies.

## Reproduction

Run in `.worktrees/_scratch/f3`, with `env -u ANTHROPIC_API_KEY claude -p
--model claude-haiku-4-5-20251001 --settings <json>`, prompt "print every
skill whose name contains herdr":

| `--settings` | Listed |
| --- | --- |
| horch's old overlay: `skillOverrides` off for `herdr-*` and `herdr:herdr-*` | `herdr:herdr-atomic`, `herdr:herdr-orchestrator`, `herdr:herdr-worker` |
| `skillOverrides` off for `herdr-*` plus `enabledPlugins: {"herdr@matts-robot-skills": false}` | NONE |
| only `enabledPlugins: {"herdr@matts-robot-skills": false}` | `herdr-orchestrator`, `herdr-worker` (the user symlinks) |

## Fix

`crates/horch-core/src/harness/claude.rs`, `overlay_skill_switches`: for each
`disabled_skills` entry `<plugin>:<skill>`, every key under which `<plugin>`
is enabled in the operator's settings or installed in
`~/.claude/plugins/installed_plugins.json` goes into `enabledPlugins` as
`false`. The `skillOverrides` entries stay, so the bare names still hide the
user-level copies.

- The insert uses `or_insert`. A plugin that the teammate's `plugin_skills`
  keeps enabled stays enabled.
- Cost: the plugin goes off whole. A pane that disables
  `herdr:herdr-worker` also loses `herdr:herdr-atomic`. Claude Code has no
  switch for one plugin skill.
- Both launch paths use this function: the plain overlay and the
  skill-bundle path (`skills_settings`).

Tests:

- `harness::claude::tests::a_disabled_plugin_skill_switches_its_plugin_off`
  checks the written `enabledPlugins` against a fake home with an enabled
  key and an installed key.
- `harness::claude::tests::plugin_skills_wins_over_a_disabled_plugin_skill`.
- `harness::launch::tests::unset_isolation_fields_add_no_flags` and
  `opting_in_to_claudeai_skills_leaves_the_switch_out` now expect
  `"herdr@m": false`.

## For the operator

The memory note calls the `~/.claude/skills/herdr-*` symlinks stale. horch
hides them in its panes. To remove them from every session, run:

```
rm ~/.claude/skills/herdr-orchestrator ~/.claude/skills/herdr-worker
```

## Found outside scope (not fixed)

`plugin_skills` narrows a plugin with `"<plugin>:<skill>": "off"`
`skillOverrides` entries (`crates/horch-core/src/harness/claude_plugins.rs`,
`overlay_plugin_skills`, and the module doc). Plugin skills ignore those
entries, so `plugin_skills` does not hide a plugin's other skills. The
briefing still names the wanted skills. A fix needs a filtered copy of the
plugin loaded with `--plugin-dir`.

## mp4 line

`skills/ue-build-verify/references/git-lfs.md` tracks `*.mp4` with LFS
again, after `*.mov`. The sentence under the block now gives `.mp3` and
`.webm` as examples. `skills/provenance.json` records no hash for this
own-text skill, and the catalog digest is computed at run time, so nothing
needs a re-hash. The u1 plan's check `grep -rni 'perforce\|p4 '` matches
`*.mp4    ` by design; it is not a test.
