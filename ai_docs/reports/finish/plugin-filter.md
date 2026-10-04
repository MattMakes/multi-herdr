# F6 plugin-filter: plugin_skills really narrows a Claude plugin

Plan: `ai_docs/plans/finish/f6-plugin-filter.md`. Single-branch work on
`design-skills`.

## The gap

F3 (`skill-overrides.md`, "Found outside scope") found that Claude Code
2.1.289 ignores `skillOverrides` for plugin skills. `overlay_plugin_skills`
wrote `"<plugin>:<skill>": "off"` entries, so a teammate with
`plugin_skills: { code: [core] }` still saw and could invoke every skill of
`code`.

## Docs check (Claude Code docs, 2026-10-04)

- Plugin loading reference (https://code.claude.com/docs/en/plugins/loading):
  - "Find where a plugin came from": a `--plugin-dir` plugin has the id
    `<name>@inline`. It is "On for the session unless the manifest sets
    `defaultEnabled: false` or a settings file sets `"<name>@inline": false`".
    For `@inline`, `<name>` is the manifest `name`.
  - "Name conflicts": an enabled `--plugin-dir` plugin replaces a same-named
    installed marketplace plugin silently. A plugin locked in managed
    settings wins over the `--plugin-dir` copy.
  - "In-place and copied plugins": a `--plugin-dir` directory "loads in place
    and is never copied".
- Plugin manifest reference (https://code.claude.com/docs/en/plugins-reference):
  - "Standard layout": skills are `skills/<name>/SKILL.md`, commands are
    `commands/`, agents `agents/`, hooks `hooks/hooks.json`, MCP `.mcp.json`,
    LSP `.lsp.json`, executables `bin/`.
  - "How each key combines with its default location": the manifest `skills`
    key ADDS directories to the `skills/` scan, and `commands` replaces the
    `commands/` scan. So a filtered copy must drop both keys.
  - Component paths must stay inside the plugin root; a symlink that leads
    outside is rejected for a component path.

## Design

- `materialize_filtered` (`crates/horch-core/src/harness/claude_plugins.rs`)
  writes a copy of each `plugin_skills` plugin to
  `<skill bundle>/plugins/<plugin>/`. `Claude::expose_skills`
  (`claude.rs`) calls it and puts the copy in `plugin_dirs`:
  - a plugin from `plugin_dirs`: the copy replaces the original entry, in
    the same position;
  - an installed plugin: the copy is appended.
- What the copy holds (my decision, as the plan asked):
  - the manifest, with the `skills` and `commands` keys removed and `name`
    set (a manifest-less plugin takes its name from its directory, which
    the copy does not share);
  - `skills/<dir>/` for each named skill only;
  - every other top-level file and directory of the plugin: agents, hooks,
    `.mcp.json`, `.lsp.json`, `bin/`, scripts, `node_modules`, docs.
    Reason: `plugin_skills` narrows skills, not tools. A named skill often
    calls the plugin's MCP server or a shared script through
    `${CLAUDE_PLUGIN_ROOT}`. The old (intended) behaviour also kept them.
  - left out: `commands/` (Claude Code lists a command as a skill), the
    unnamed skills, and Claude Code's cache markers `.in_use`,
    `.orphaned_at`, `.git`.
  - A symlink is copied as a symlink (unix). A skill that holds a symlink
    fails the digest check (below).
- Digest check: each named skill directory is digested with
  `horch_marketplace::integrity::tree_digest` before and after the copy.
  A difference fails the launch: "the plugin changed during the launch".
  This is the same copy check `materialize.rs` does for operator skills.
- Settings overlay (`overlay_plugin_skills`): every installed
  `"<plugin>@<marketplace>"` goes `false`, and `"<plugin>@inline"` goes
  `true`. The `skillOverrides` entries are gone. The `false` keys are belt
  and braces: the docs say the copy replaces the original anyway.
  `overlay_skill_switches` keeps its `or_insert` for `disabled_skills`; that
  loop never names `@inline`, so the copy stays on.
- Lifetime: the copy is inside the bundle directory, so it is deleted with
  the bundle when the pane's agent exits.
- No bundle: `claude_command` calls `check_filtered` before the
  `--plugin-dir` flags (unless `disable_skills`). A `plugin_skills` plugin
  that is not a filtered copy fails the launch: "... this launch has no
  bundle; give the teammate a phase or a skill". `horch teammates --check`
  already rejects `plugin_skills` without a phase or skills.

## Tests

- `harness::launch::tests::plugin_skills_load_a_filtered_copy_of_the_plugin`
  (replaces `plugin_skills_switch_off_the_rest_of_the_plugin`). A fixture
  plugin with 3 skills (`review`, `lint`, `format`), `commands/`, `hooks/`,
  `scripts/`, `.in_use/` and a manifest with `skills` and `commands` keys;
  only `review` is named. It checks:
  - from `plugin_dirs`: argv has `--plugin-dir <bundle>/plugins/code` then
    the bundle, not the original; the overlay has `code@inline: true`;
  - the copy: `skills/` holds only `review` (with its script), hooks and
    scripts stay, no `commands/`, no `.in_use/`, the manifest is
    `{"name","version"}`;
  - the briefing names `code:review` and not `code:lint`;
  - the copy is gone after the bundle drops;
  - installed (`dev@market`, `dev@fork`): the copy is appended, both keys go
    `false`, `dev@inline` goes `true`, the install path is not in argv;
  - no bundle: the launch fails ("has no bundle");
  - an unknown skill still fails ("no skill 'deploy'").
- `harness::claude::tests::plugin_skills_wins_over_a_disabled_plugin_skill`
  now checks that `@inline` survives the `disabled_skills` switch-off.

Results:
- `cargo test -p horch-core --lib`: 422 of 422 pass.
- `cargo test -p horch-e2e --test skills_exposure`: 14 of 14 pass.
- `cargo test -p horch-core --test skills_catalog`: pass.
- `cargo clippy -p horch-core --all-targets -- -D warnings`: clean.
- `rustfmt --check` on the 3 changed `.rs` files: clean.

## Live check (Claude Code 2.1.289, OAuth login, no API key)

The scratch dir is `.worktrees/_scratch/f6`. I built a copy of the installed
`code@matts-robot-skills` plugin (skills `core`, `e2e-harness`) by the same
rules, with only `core` named. Each run used
`env -u ANTHROPIC_API_KEY claude -p --model haiku`.

| Run | Skills listed |
| :- | :- |
| Control: no flags | `code:core`, `code:e2e-harness` |
| `--plugin-dir <copy>` + `{"enabledPlugins":{"code@matts-robot-skills":false,"code@inline":true}}` | `code:core` |
| The bundle layout: `--plugin-dir B/plugins/code --plugin-dir B` (`B` is a `horch` plugin with skill `hello`), same settings | `code:core`, `horch:hello` |

In the bundle layout, the Skill tool call for `code:e2e-harness` returned
"Unknown skill: code:e2e-harness". The nested `plugins/` directory does not
add skills to the `horch` plugin.

## Limits

- The ledger does not record the plugin skills (F1 records the bundle plan
  only). The copy check proves the copy equals the source at launch time.
  Recording plugin skills at spawn needs changes in `skills/activation.rs`
  and the spawn path, which are outside this plan.
- A plugin that managed settings lock wins over the copy (docs, "Name
  conflicts"). horch cannot change that.
- A plugin that ships commands loses all of them in a `plugin_skills` pane.
  `plugin_skills` names skills only.

## Found outside scope (not fixed)

- `cargo test -p horch-core --test arch_scan` fails 2 tests on the tip.
  These failures are not from F6:
  - `arc_10_harness_match_only_in_harness`: `HarnessKind` matches in
    `crates/horch-core/src/competition/preflight.rs` and
    `crates/horch/src/dataset/preflight.rs`. Another worker has
    uncommitted edits in these files.
  - `arc_05_no_ambient_env_in_core`: `std::env::var_os("PATH")` in the
    sandbox host check of `crates/horch-core/src/harness/claude.rs`
    (committed earlier by the sandbox work).
