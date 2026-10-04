# GW8 godot-genre-blueprints

Unit GW8 of the Godot wave (`ai_docs/plans/godot/gw8-genre-blueprints.md`).
Worker: opus-81. Date: 2026-10-04.

## What

`skills/godot-genre-blueprints/` is a new own-text bundle:

- `SKILL.md` (7.4 KB, description 774 bytes): a router. It tells the worker to
  read the project context, pick 1 genre, read only that reference (plus at
  most 1 second reference for a hybrid), and send a `QUESTION:` when the
  genre is not clear. It lists all 27 genres with a one-line "pick this
  when", the rules that hold in every genre, how to prove the work
  (`godot-build-verify`, `godot-scene-files`), and version notes.
- `references/<genre>.md` for each of the 27 gd-agentic `godot-genre-*`
  skills (operator decision 2), plus `references/project-templates.md`
  from `godot-project-templates`. 28 references, 3.8 to 4.5 KB each.

Each reference has the same 5 parts: core loop; required systems as a table
that names the combined `godot-*` skill for each system (names from
`ai_docs/reports/godot-wave.md`); a 4.7 scene tree sketch; 1 own GDScript
block for the one technique no other skill in the catalog covers; pitfalls.

The bundle is 118,456 bytes in total, under the 160 KB directory budget of
the `skills_bundled_size_budget` test. That budget, not the plan's "about
8 KB", set the size: 28 references at 8 KB would be 224 KB and fail the
test. The bundle is not vendored, so it gets no exemption.

## Sources consulted

gd-agentic-skills at `4c4d0ff5c4597938cc9257d99d9e35f7692c9c06`,
read-only clone in `.worktrees/_scratch/godot-src/`. I read the
`SKILL.md` of each of the 28 directories and their script and reference
indexes for facts: core loops, the systems each genre needs, the pitfalls.
I copied nothing (operator decision 1): every sentence, table, scene tree and
code block is written new. The code blocks implement different techniques
from the upstream scripts where possible (for example the storm-circle
pick, the seeded per-system RNG streams, the frame counter, the LIFO stack).

To avoid repeating combined skills, I checked GodotPrompter v1.14.0
(`3e8d0f005f9604e1dbdad3de693e39555384c5af`) section lists. Example:
`player-controller` already has coyote time and jump buffer, so
`platformer.md` has a drop-through block and only names the tuning ranges.

Facts checked against the 4.7.2 class reference (online docs) where the
`--doctool` dump has no descriptions:

- `SceneReplicationConfig`: mode Always sends unreliable, mode On Change
  sends reliable (`battle-royale.md`).
- `AudioServer.get_output_latency()` "can be expensive; it is not
  recommended to call it every frame" (`rhythm.md` caches it per song).

## Upstream claims not carried, or corrected

- gd-agentic says `AStarGrid2D.jumping_enabled` gives "O(1)" queries. Not
  carried. `roguelike.md` says jumping speeds up open grids and ignores
  weights.
- gd-agentic says never use `!` in AnimationTree expressions and always use
  `is_equal_approx` for timers. Not carried: no source in the 4.7 docs.
- gd-agentic `idle-clicker` uses `OS.low_processor_usage_mode`. It exists in
  4.7.2 and is kept.
- gd-agentic `party` says "`Input.is_action_pressed()` cannot join players".
  Rewritten as a fact: an action alone does not carry the device; read
  `InputEvent.device`.
- gd-agentic `project-templates` names 4.7 stretch defaults. Not carried:
  not verified.
- Dropped: every link to `godot-monte-carlo-balancer` (excluded skill) and
  `godot-master` (excluded).

## Checks

All run on the bundle in `.worktrees/_scratch/godot-opus-81/skills/`
before the copy. `godot-commit.sh` ran `skills_catalog` on `skills/`:

- `scripts/godot/api_check.py skills/godot-genre-blueprints`:
  `api_check: 29 files, 0 unknown names`.
- `scripts/godot/gdscript_blocks_check.py skills/godot-genre-blueprints`:
  `28 files, 28 blocks, 28 parse (28 whole, 0 with an added extends, 0 in a
  func), 0 fail`. Every block is a whole script; no skip markers.
- A scratch prose check (`.worktrees/_scratch/godot-opus-81/apicheck.py`):
  every `Class.member` in the prose, not only in code blocks, against the
  4.7.2 dump: 63 pairs, 0 missing. Every backticked engine method and class
  name in the prose exists, except own names (for example `CardData`).
- `cargo test -q -p horch-core --test skills_catalog`: run by
  `godot-commit.sh` before the commit; the commit exists only if it passed.

## Commits

One commit through `.worktrees/godot-commit.sh` (the orchestrator's rule that
replaced the manual lock steps). The plan asked for batches of 6 to 9
genres. The script commits a whole skill directory at once, and the bundle
was complete and checked before the lock, so 1 commit holds the bundle, the
3 index entries (provenance, README row, `REPO_ORIGINAL`) and this report.
The script ran `skills_catalog` before the commit. The SHA is in the
`NOTE: COMMITTED` message to the orchestrator.

## Not done, and notes for other units

- No `godot-*` combined skill exists in `skills/` yet for most names the
  references point at (for example `godot-combat-system`,
  `godot-player-controller`). The names follow the wave report; a unit that
  renames a skill must update these references.
- `godot-gameplay-loops` (GW unit for loops) is not referenced, because no
  genre needs it by name; a reviewer may want a pointer from `simulation.md`
  or `idle-clicker.md`.
