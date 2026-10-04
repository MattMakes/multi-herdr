# F6 plugin-filter: plugin_skills really narrows a Claude plugin

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.

## GOAL

A teammate with `plugin_skills: [<plugin>:<skill>, ...]` sees only the named
skills of that plugin in its Claude pane; the plugin's other skills are not
listed and cannot be invoked.

## CONTEXT

- Found by F3 (`ai_docs/reports/finish/skill-overrides.md`, "Found outside
  scope"): Claude Code 2.1.289 ignores `skillOverrides` for plugin skills
  (docs confirm), so `overlay_plugin_skills` in
  `crates/horch-core/src/harness/claude_plugins.rs` cannot hide a plugin's
  other skills.
- Proposed fix: materialize a filtered copy of the plugin (its manifest and
  only the named skills; agents/commands/hooks/MCP per a decision you write
  down) in the launch bundle, load it with `--plugin-dir`, and disable the
  original with `enabledPlugins: {"<plugin>@<marketplace>": false}` (the F3
  mechanism). Check the plugin directory layout and `--plugin-dir` semantics
  in Claude Code's plugin docs; cite them.
- The copy is digest-checked like other bundled skills (see F1,
  `ai_docs/reports/finish/operator-digest.md`).
- Do not edit `crates/horch-core/src/harness/claude.rs` while opus-65 (S6)
  has uncommitted edits there; if you need a change in it, ask opus-65 by
  `horch tell` to include it or wait until it commits.

## FILES

own: `crates/horch-core/src/harness/claude_plugins.rs`, the bundle/materialize
code it needs, tests, `docs/` mention of plugin_skills,
`ai_docs/reports/finish/plugin-filter.md`.

## STEPS

1. Docs check and design (report). 2. Filtered copy + launch + tests (a
fixture plugin with 3 skills; only 1 named). 3. Live check with a real
Claude pane if cheap (`claude --plugin-dir ... ` listing skills; never with
an API key). 4. Targeted checks; COMMITTED.
