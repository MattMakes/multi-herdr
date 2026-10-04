# G6 docs-template

Worker sonnet-15, branch `design-skills`.

## Done

1. `README.md` and `docs/recipes/add-teammate.md` say that a skill changed between spawn and launch fails the launch.
2. `teammates/_template.md` already listed `xcode` and `blender` in `requires`. No change.
3. `teammates/_template.md` and `README.md` no longer say that a skillOverrides "off" entry hides a plugin skill. They say it hides a `~/.claude/skills` entry only. On 2.1.289 horch uses `enabledPlugins` (`inherit_plugins: false`) or a filtered copy (`plugin_skills`). `docs/skills-and-teams.md` and `teammates/README.md` do not repeat the claim.
4. `docs/recipes/add-teammate.md` already named `blender`. `docs/skills-and-teams.md` now says `horch doctor` warns first.
5. `Inherited.blender_path` added in `context.rs`. `doctor.rs` reads it.
6. `horch teammates --matrix` already shows `operator_skills` as `operator:<dir>/<name>` (commits 8e052a6, 07ebab7). The test `the_matrix_shows_skill_fields_and_the_offer_gate` covers it. No change.

## Checks

- `horch teammates --check`: roster ok, 4 operator-skill warnings (host lacks the Xcode export).
- arch_scan: 13 passed. doctor tests: 11 passed. teammatescmd tests: 4 passed.
- clippy -D warnings on horch-core and horch: clean. rustfmt: clean.
