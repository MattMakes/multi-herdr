# GW0 godot-tooling: report

Plan: `ai_docs/plans/godot/gw0-tooling.md`. Worker: opus-74. Date: 2026-10-04.

## Commits

| step | commit | what |
|---|---|---|
| 1 | `c1aa388`, `3164b3b`, `a4f9bde` | `scripts/godot/rename.py` with its test. `__pycache__` was committed by mistake in `c1aa388`. `3164b3b` adds `scripts/godot/.gitignore` and `a4f9bde` removes the bytecode. |
| 2 | `85cc43c` | `scripts/godot/api_check.py` with its test |
| 3 | `ee48348` | `scripts/godot/gdscript_blocks_check.py` with its test |
| 5 | `bfb43eb` | the `godot_skills` gate step, and `--strict-own` (the orchestrator's decision) |
| 4 | `135899f` | doc comment on `budget_exempt` in `crates/horch-core/tests/skills_catalog.rs` |

Tests: `python3 -m unittest discover -s scripts/godot/tests`. 19 of 19 pass. 2 of them need Godot and skip without it.

## 1. rename.py

`scripts/godot/rename.py <upstream skills dir> <skill>... --out skills/ [--dry-run] [--json] [--force]`

- The script copies the skill directory without dotfiles, renames it to `godot-<name>` (a name that starts with `godot-` stays the same), and sets `name:`. It does not change `description:` or any other frontmatter line.
- The script copies the upstream `LICENSE` (the parent of the skills dir) next to `SKILL.md`.
- The script prints `sha256  <upstream path>` for each upstream file. `--json` also prints the `sources` entries for `provenance.json` (repository, revision from `git rev-parse HEAD`, path, sha256, `MIT`).
- `--dry-run` prints a unified diff and writes nothing. If the target exists, the script stops unless you give `--force`. `--force` overwrites only the copied files, so your own references stay.
- The map is the full upstream list (56 skills), so a reference to a skill that you did not copy is also renamed. I searched all 56 skills for reference forms. The script rewrites these forms in the body of every `.md`:
  - `godot-prompter:<x>` and `godot-prompter:*` (the plugin form, 30 uses);
  - `**<x>**` (Related-skills lines and "see **x**" in prose);
  - `` `<x>` `` (the routing tables in `using-godot-prompter`, `godot-mentor`, `godot-grill`, the addon skills);
  - `skills/<x>/` (for example `` `skills/physics-system/SKILL.md` ``, and `~/.gemini/config/skills/<x>/`);
  - `<x> skill` (for example "see save-load skill" in `audio-system/references/audio-settings.md`);
  - an indented `<x>/` line (the directory tree in `using-godot-prompter/references/antigravity-tools.md`).
- After a run on all 56 skills, no upstream name is left as a reference. The remaining matches are addon paths (`addons/beehave/`, `res://addons/limboai/...`), the `#[gdextension]` attribute and plain words ("built-in localization"). All 56 `description:` lines are byte-identical to upstream.
- `using-godot-prompter` becomes `godot-using-godot-prompter`, because the rule only keeps names that start with `godot-`.

## 2. api_check.py

`scripts/godot/api_check.py [--doctool DIR] <paths...>`

- Input: `.gd` files, and the ```` ```gdscript ````, ```` ```gd ````, ```` ```csharp ```` and ```` ```cs ```` blocks in `.md` files. Prose and untagged blocks are not checked.
- A use is `Class.name` where `Class` is an engine class, a Variant built-in or a global enum (`Key`, `MouseButton`, `Variant.Type`). The name must be a method, member, setter, getter, constant, enum, signal or theme item of the class or of an ancestor. `new` is always allowed.
- C#: the script matches names in snake_case (`IsActionPressed`) or UPPER_SNAKE (`Vector2.Zero`). It matches global enum values without their prefix (`Key.Space` is `KEY_SPACE`) and `XxxEnum` types. The bindings-only names `Singleton`, `IsInstanceValid`, `SignalName`, `PropertyName`, `MethodName`, `From`, `Create` and `CreateFrom` are allowed, and `GodotObject` means `Object`. No GodotSharp assembly is on this Mac, so I could not check other C#-only names.
- `api-check: allow <Name>` on the line allows `Class.member` or `member`.
- The doctool dump is in `.worktrees/_scratch/godot-doctool-4.7.2/` (git-ignored by `.worktrees/`; I checked with `git check-ignore`). It has 1,076 class XML files and took 10 s to make. If you do not give `--doctool`, the script reuses or makes `.worktrees/_scratch/godot-doctool-<version>/`. If no Godot is found, the script prints `skipped: no Godot` and exits 0.

**GodotPrompter baseline (56 skills, 252 files): 10 unknown names, exit 1.** The plan expected about 2. 7 are real GDScript errors on 4.7.2, and the content units should fix them:

| file:line (upstream) | name | fact in the 4.7.2 dump |
|---|---|---|
| `godot-optimization/references/memory-management.md:76` | `OS.gc` | no such method |
| `godot-optimization/references/memory-management.md:17` | `Performance.MEMORY_DYNAMIC` | removed; `MEMORY_STATIC`, `MEMORY_STATIC_MAX` exist |
| `3d-essentials/references/environment-and-post.md:47` | `Environment.TONE_MAP_FILMIC` | the name is `TONE_MAPPER_FILMIC` |
| `3d-essentials/references/environment-and-post.md:122` | `Environment.TONE_MAP_AGX` | the name is `TONE_MAPPER_AGX` |
| `gdscript-advanced/references/tool-script-recipes.md:42` | `Mesh.PRIMITIVE_LINE_LOOP` | no such constant (`PRIMITIVE_LINE_STRIP` exists) |
| `xr-development/references/hand-tracking.md:28`, `:30` | `XRHandTracker.HAND_JOINT_INDEX_TIP` | the name is `HAND_JOINT_INDEX_FINGER_TIP` |

The other 3 are C# `StringName.Empty` (`ability-system/references/stat-modifiers.md:64`, `tags-and-conditions.md:323`, `:347`). The dump has no C# names, so the tool cannot confirm them. If `StringName.Empty` is real in GodotSharp 4.7, mark the line with `// api-check: allow StringName.Empty`.

On the 18 `godot-*` skills already in `skills/` (GW1), the script reports 0 unknown names in 58 files.

## 3. gdscript_blocks_check.py

`scripts/godot/gdscript_blocks_check.py [--strict-own skills/provenance.json] [--scratch DIR] [--keep] <paths...>`

- The script writes each ```` ```gdscript ```` / ```` ```gd ```` block to a new scratch project under `.worktrees/_scratch/godot-blockcheck/run-*/`. Each run has its own project, so workers do not collide, and the script deletes the project unless you give `--keep`. One headless `SceneTree` script `load()`s every file.
- **`load()` returns a script even when the script has a parse error.** So the result comes from Godot's error text between the `@@BEGIN`/`@@END` markers on stderr, not from the return value or the exit code.
- Tries, in order. The block passes on the first try that parses:
  1. the block as a whole script;
  2. if the block has no top-level `extends`: `extends <B>` + the block;
  3. `extends <B>` + `func _f() -> void:` + the block, indented with the block's own indent (tab or 4 spaces).

  `<B>` is each of Node, Node2D, Node3D, Control, CharacterBody2D, CharacterBody3D, Resource. The plan named only Node. With Node only, fragments such as `z_index = 10` (a Node2D property) failed.
- Every `class_name` is registered in a hand-written `.godot/global_script_class_cache.cfg`, so a block can use a class from another block. When 2 blocks declare the same `class_name`, the first block registers it. The other block is checked without its `class_name` (otherwise Godot reports "hides a global script class").
- The script reports the whole-script error at its markdown line. Exception: for a statement list (no `extends`, no `func`) whose error is "... in class body", the script reports the error from inside a function of Node.
- `<!-- gdscript-check: skip -->` on the line before the fence (blank lines between are allowed) skips the block.
- `--strict-own` (orchestrator decision): a failing block in a file that a `sources` path in `provenance.json` lists (`skills/<upstream>/<rel>` maps to `skills/<skill>/<rel>`) prints `(upstream, report only)` and does not change the exit code. Own references are not in `sources`, so they stay strict.

**GodotPrompter baseline (56 skills, 252 files): 329 of 655 blocks parse (50.2%):** 201 as whole scripts, 56 with an added `extends`, 72 inside a function. 326 fail. A run takes 12 s. The failures are mostly fragments that use names from outside the block, not engine errors:

| count | error |
|---|---|
| 154 | Identifier not declared (fragment variables `health`, `mat`, autoloads `GameState`, `EventBus`) |
| 35 | Function not found in base self (helpers outside the block) |
| 27 | Could not find type (addon and project types: `PopochiuInventoryItem`, `BeehaveTree`, `AudioManager`) |
| 25 | Preload file does not exist |
| 21 | Could not find base class (`GutTest`, `GdUnitTestSuite`, `ActionLeaf`, `PhantomCamera2D`) |
| 17 | Unexpected ... in class body (statement and function mixes) |
| 12 | Function has the same name as a previously declared function (2 versions in 1 block) |
| 10 | `$` on a class that is not a node |
| 25 | other (wrong signatures, type inference, a real `HAND_JOINT_INDEX_TIP` member error, and others) |

On the 18 GW1 skills (58 files, 167 blocks): 59 parse, and 108 fail. All 108 are in copied files, so the run reports them and exits 0.

## 4. Size exemption

`budget_exempt()` exempts every skill whose provenance has `vendored: true`, from both the 12 KB `SKILL.md` limit and the 160 KB directory limit. A combined skill sets `vendored: true`, so the rule already covers it. I changed only the doc comment. One consequence: the own references in a combined skill do not count against the 160 KB directory limit. That matches the rule for all vendored skills, and `skills/README.md` already says it.

## 5. Gate

The `godot_skills` step in `scripts/phase-gate.sh`, after `verify-telemetry-e2e.sh`:

1. `python3 -m unittest discover -s scripts/godot/tests`;
2. if `skills/godot-*` is absent: `skipped: no skills/godot-*`;
3. `api_check.py skills/godot-*/` (strict on everything);
4. `gdscript_blocks_check.py --strict-own skills/provenance.json skills/godot-*/`.

With no Godot, both checks print `skipped: no Godot` and pass, and the Godot unit tests skip. Time on the tip with 18 `godot-*` skills: 24 s (unit tests 14 s, block check 9 s, API check 1 s). With all 56 upstream skills, the block check takes 12 s, so the step stays well under the 60 s limit. A changed-files-only mode is not needed.

## Notes outside my scope

- `crates/horch-core/tests/skills_catalog.rs` has an uncommitted test from another worker (`skl_07_named_plugin_skills_enter_the_plan_as_plugin_refs`). `rustfmt --check` reports 3 diffs in it (lines 618, 637 and 671 at the time). I committed only my hunk (`git apply --cached`).
- The content units must fix the 7 API errors above when they copy those files. `api_check.py` is strict on copied text too, so the gate fails until they fix them.
