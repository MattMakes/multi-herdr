# plugin_skills loads a filtered copy of the plugin

A plugin often ships many skills while a teammate should use 2 of them.
`plugin_skills: { <plugin>: [a, b] }` (`crates/horch-core/src/harness/claude_plugins.rs`)
does 3 things. It names `a` and `b`, with their descriptions, in the
worker's briefing as skills it is expected to use (`skills.rs`,
`Bundle::briefing`). It loads a filtered copy of the plugin from the
launch's skills bundle (`<bundle>/plugins/<plugin>/`,
`materialize_filtered`) with `--plugin-dir`: the plugin minus every skill
and command the teammate did not name. And it switches the installed
original off (`"<plugin>@<marketplace>": false`) and the copy on
(`"<plugin>@inline": true`), whatever `inherit_plugins` says. A copy is
needed because Claude Code ignores `skillOverrides` for a plugin skill
(2.1.289), so only a copy without the other skills hides them.

## Consequences

- A plugin is found in the teammate's `plugin_dirs` first, then in the
  operator's `~/.claude/plugins/installed_plugins.json`. Its skills are the
  `skills/*/SKILL.md` files under its root.
- `installed_root` reads `installed_plugins.json`: keys are
  `name@marketplace`, and each value is an install record or, in version 2
  of the file, a list of them, each carrying an `installPath` and a
  recorded `version`. A `~/` path is expanded against `$HOME`.
- `installed_keys` returns every `name@marketplace` key under which a
  plugin is installed, plus a given `always` key, sorted. `always` covers
  a machine where the plugin is not installed yet: an `enabledPlugins`
  entry for a missing plugin does nothing.
- Claude Code names a plugin from `--plugin-dir` `<plugin>@inline` and an
  installed one `<plugin>@<marketplace>`. `NamedPluginSkill::plugin_key`
  and the `enabledPlugins` overlay use these ids.
- `materialize_filtered` returns the teammate's `plugin_dirs` with each
  plugin's original directory replaced by its copy, and the copy of each
  installed plugin appended. The copy keeps the plugin's agents, hooks, MCP
  and LSP servers, scripts and other files: `plugin_skills` narrows
  skills, not tools.
- Each named skill's copy must hold the bytes the bundle's plan pinned for
  `<plugin>:<skill>` (the digest the ledger recorded, SKL-04), or, for a
  skill the plan lacks, the bytes of its source (`tree_digest`). Otherwise
  the launch fails.
- `LEFT_OUT` lists what a copy leaves out of the plugin root. The skills
  and the commands go, because Claude Code lists a command as a skill, and
  are rebuilt from the named skills only. The manifest (`.claude-plugin`)
  is rewritten. Claude Code's own marker files in an installed plugin's
  cache directory are dropped.
- `MANIFEST_LEFT_OUT` drops the manifest keys `skills`, which adds skill
  directories to the `skills/` scan, and `commands`, which names command
  files. Either would bring back skills the teammate did not name.
- `write_filtered_manifest` always writes `name` into the copy's manifest.
  A manifest-less plugin takes its name from its directory, and the copy's
  directory does not share it.
- `overlay_plugin_skills` switches every installed
  `<plugin>@<marketplace>` off, so only the copy can load, and
  `<plugin>@inline` on, even where `inherit_plugins: false` or the
  operator's settings switched it off. It must run after
  `disable_operator_plugins`, so in `skills_settings`
  `overlay_skill_switches` runs after the plugin switch-off and a
  `plugin_skills` plugin stays enabled.
- `check_filtered` fails a launch whose `plugin_skills` plugins are not
  filtered copies. A launch without a skills bundle has nowhere to put a
  copy, and the original plugin would load with every skill, or, if
  installed and switched off, with none.
