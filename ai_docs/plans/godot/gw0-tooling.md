# GW0 godot-tooling: rename script, API check, code-block check, size exemption, gate step

Follow `ai_docs/plans/godot/00-godot-conventions.md`. Other Godot units wait
on your NOTEs, so deliver in this order and send a NOTE after each step.

## GOAL

The tools every Godot content unit needs exist, are tested and are in the
gate.

## STEPS

1. `scripts/godot/rename.py <upstream skills dir> <skill> [<skill>...] --out skills/`:
   copies a GodotPrompter skill directory (minus dotfiles), renames it to
   `godot-<name>` (keep a name that already starts with `godot-`), sets the
   frontmatter `name:`, rewrites cross-references (`godot-prompter:<x>`,
   bold `**<x>**` in "Related skills" lines, and any other reference form you
   find in the 56 upstream skills — grep them all) to the new names, using the
   full upstream skill list as the map, and copies the upstream `LICENSE`
   next to `SKILL.md`. Print the sha256 of each upstream file copied (for
   provenance). `--dry-run` shows the diff. Never touch `description:`.
   Test it (a small fixture under `scripts/godot/tests/`, run by
   `python3 -m unittest`). COMMITTED, then
   `horch tell orchestrator "NOTE: rename.py ready <sha>"`.
2. Doctool dump: `Godot --headless --doctool <dir>` once into
   `.worktrees/_scratch/godot-doctool-4.7.2/` (git-ignored; check it
   is). `scripts/godot/api_check.py [--doctool DIR] <paths...>`: finds every
   `Class.member` / `Class.method(` use of a real engine class (and
   `Variant` built-ins: Array, Dictionary, String, Vector2/3, ...) in
   `.md` code blocks and `.gd` files, resolves inheritance, setters/getters,
   constants, signals, enums; prints each unknown name with file:line and
   exits 1 if any. It generates the dump itself when `--doctool` is missing
   and Godot is found (`GODOT_PATH`, PATH `godot`, the macOS app bundle);
   with no Godot it prints "skipped: no Godot" and exits 0. A
   `# api-check: allow <Name>` comment on the line allows a name on purpose
   (for example a "this does not exist" example). Test with fixtures. Run it
   on the 56 GodotPrompter skills: it should find about 2 (`OS.gc`); record
   the result in your report. COMMITTED + NOTE.
3. `scripts/godot/gdscript_blocks_check.py <paths...>`: extracts every
   ```gdscript (and ```gd) block from `.md`, writes each to a scratch
   project under `.worktrees/_scratch/godot-blockcheck/` (or a temp dir),
   and parse-checks each with a headless `SceneTree` script that `load()`s
   it (never trust `--check-only`'s exit code). Report file:line of each
   failing block with Godot's error. Blocks that are deliberately partial
   (a fragment that is not a whole script) need a rule: try it as a whole
   script; if that fails, wrap it in `extends Node` + `func _f():` and retry;
   a `<!-- gdscript-check: skip -->` line just before the block skips it.
   Run it on GodotPrompter and record the pass rate. COMMITTED + NOTE.
4. Size exemption in `crates/horch-core/tests/skills_catalog.rs`
   (`budget_exempt`): today every `vendored: true` skill is exempt. Combined
   Godot skills are a renamed upstream SKILL.md plus own references, also
   marked `vendored: true`, so check whether anything needs to change. If the
   rule already covers them, write that down in the test's doc comment and
   change nothing else. COMMITTED.
5. Gate: add both checks to `scripts/phase-gate.sh` for `skills/godot-*`
   (skip cleanly when no Godot or no `skills/godot-*`). Time them: the gate
   must not get slower by more than 60 s; if it would, run the block check
   only on files changed since `origin/main`. COMMITTED + NOTE.

## FILES

own: `scripts/godot/`, `scripts/phase-gate.sh` (the Godot step only),
`crates/horch-core/tests/skills_catalog.rs` (step 4 only),
`ai_docs/reports/godot/gw0-tooling.md`.

## DONE WHEN

The 3 scripts run with tests, the gate runs them, and the report shows the
GodotPrompter baseline numbers.
