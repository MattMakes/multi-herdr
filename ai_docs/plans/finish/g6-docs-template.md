# G6 docs-template: close the doc follow-ups and BLENDER_PATH

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.

## GOAL

The docs and the teammate template say what the code does now, and
`BLENDER_PATH` is read through `RuntimeContext` like `PATH`.

## STEPS (each with its check)

1. `README.md` ("Apple's Xcode skills") and `docs/recipes/add-teammate.md`:
   say that a skill that changed between spawn and launch fails the launch
   ("the skills changed since <id> was spawned ... Spawn the worker again").
   Source: `ai_docs/reports/finish/operator-digest.md` "Follow-ups".
2. `teammates/_template.md`: `requires` values are `xcode` and `blender`
   (`blender` on PATH or `BLENDER_PATH`, `--version` runs). Check with
   `horch teammates --check` and the test
   `the_template_documents_exactly_the_teammate_fields`.
3. `teammates/_template.md` lines near 148 and 171: the text says
   skillOverrides "off" entries work for plugin skills, "verified against
   Claude Code 2.1.278". On 2.1.289 they do not (F3, F6, and
   `ai_docs/reports/finish/acceptance-fleet.md` LA-4). Make the text match
   the code: horch switches a plugin off with `enabledPlugins` and loads a
   `plugin_skills` plugin as a filtered copy (read
   `crates/horch-core/src/harness/claude_plugins.rs` module doc and
   `ai_docs/reports/finish/plugin-filter.md`). Do the same in
   `docs/skills-and-teams.md` and `teammates/README.md` if they repeat it.
4. `docs/recipes/add-teammate.md:40` names only `requires: [xcode]`: add
   `blender`. `docs/skills-and-teams.md:199`: say `horch doctor` warns first
   when Blender is missing, before a worker reports `BLOCKED:`.
5. `BLENDER_PATH` into `crates/horch-core/src/runtime/context.rs`
   `Inherited` (next to `path`, read where the others are read); 
   `crates/horch/src/cmd/doctor.rs:47` uses `ctx.inherited.blender_path`
   instead of `std::env::var_os`. Tests: doctor tests pass; arch_scan passes.

6. `horch teammates --matrix` does not show `operator_skills` (opus-56,
   O1). Add them to the matrix the way the other skill columns appear (for
   example `op:<name>`), with a test. You own the matrix code in
   `crates/horch/src/cmd/teammatescmd.rs` for this step only.

Source of 2, 4, 5: `ai_docs/reports/finish/usage-doctor.md` "Not done".

## FILES

own: `README.md`, `docs/recipes/add-teammate.md`, `docs/skills-and-teams.md`,
`teammates/_template.md`, `teammates/README.md`,
`crates/horch-core/src/runtime/context.rs` (one field),
`crates/horch/src/cmd/doctor.rs` (the BLENDER_PATH read only),
`ai_docs/reports/finish/docs-template.md`.

do not touch: any other `.rs` file (except the matrix code, step 6), any other teammate file.

## CHECKS

`horch teammates --check` (use `target/debug/horch` after `cargo build -p horch`),
`cargo test -p horch-core --test arch_scan`, the template-fields test
(find it with grep), `cargo test -p horch --bin horch doctor`,
clippy -D warnings on horch-core and horch, rustfmt on the 2 .rs files.

## REPORT

`horch tell orchestrator "NOTE: COMMITTED <sha>..."`, then `horch done`.
