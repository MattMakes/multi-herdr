# GW9 godot-gameplay-bundles: report

Unit plan: `ai_docs/plans/godot/gw9-gameplay-bundles.md`. Worker: opus-82.
Engine: Godot 4.7.2.stable.official.ed1daf0bf.

## What

4 own-text bundles under `skills/`:

| Bundle | Files | SKILL.md | Built from (gd-agentic, consulted only) |
| --- | --- | --- | --- |
| `godot-gameplay-loops` | SKILL.md (router), 6 references: collection, harvest, time-trial, waves, revival, secrets | 6,834 B | game-loop-collection, game-loop-harvest, game-loop-time-trial, game-loop-waves, mechanic-revival, mechanic-secrets |
| `godot-combat-system` | SKILL.md, references/real-time.md, references/turn-based.md | 7,479 B | combat-system, turn-system |
| `godot-economy-system` | SKILL.md, references/shops-and-loot.md, references/balancing.md | 7,494 B | economy-system |
| `godot-quest-system` | SKILL.md, references/branching-and-timing.md, references/ui-and-world.md | 9,686 B | quest-system |

Provenance for each: `sources: []`, `vendored: false`, adaptation "own text,
consulted thedivergentai/gd-agentic-skills@4c4d0ff: <dirs>". The README
"Godot skills" table has one `own text` row per bundle.

## Method

- I read each gd-agentic SKILL.md, its references and the main scripts for
  facts. Then I wrote every sentence and every code block new, with my own
  class design. No text, table, `.gd` or `.py` is copied (operator
  decision 1).
- Each bundle points at the combined `godot-*` skills instead of repeating
  them: inventory, save-load, state machine, resource pattern, event bus,
  component system (hitbox, hurtbox, health), ability system, dialogue,
  HUD, localization, multiplayer sync, optimization.
- `godot-combat-system` extends the `godot-component-system` hurtbox with one
  `receive_attack(hit)` method that resolves a typed hit and then calls the
  component's existing `receive_hit(int)`. It does not define a second
  hitbox, hurtbox or health class.
- Drafts were written and checked in `.worktrees/_scratch/godot-opus-82/skills/`
  and committed with `.worktrees/godot-commit.sh`, 1 commit per bundle, from
  fragment folders in `.worktrees/_scratch/godot-opus-82/unit/<bundle>/`.
  Each is listed in `REPO_ORIGINAL` in `skills_catalog.rs`.

## Checks

| Check | Result |
| --- | --- |
| `scripts/godot/gdscript_blocks_check.py` on the 4 bundles | 45 blocks, 45 parse, 0 fail |
| `scripts/godot/api_check.py` on the 4 bundles | 0 unknown names |
| Scratch load check: every block in one project per bundle, `--import`, then `load()` + `can_instantiate()` | 45 of 45 load |
| Scratch name check: every `.method(` against all doctool members | 0 unknown |

## Headless proofs

Each proof project is `.worktrees/_scratch/godot-opus-82/proof/<bundle>/`.
It holds the exact code blocks from the skill text (extracted, not
retyped) and a `SceneTree` script in `tests/check.gd`. Run:
`Godot --headless --path <proof> --import`, then
`Godot --headless --path <proof> --script res://tests/check.gd`.

| Bundle | System proved | Checks | Exit |
| --- | --- | --- | --- |
| gameplay-loops | `WaveDirector` (deferred spawns, `tree_exited` live count, double-start guard, clear, all-clear), `SequenceMatcher` (suffix match, gap limit), `CollectionTracker` (dedupe, JSON round trip, single completion) | 11 of 11 | 0 |
| combat-system | `DamageResolver` (defense, resistances, critical, immunity, multi-type), `SwingTracker`, `RoundQueue` (tie-break, re-sort, dead skip), `TimelineQueue` (preview equals real order), `TurnBudget`, `AStarGrid2D` grid | 21 of 21 | 0 |
| economy-system | `Wallet` (all-or-nothing multi-currency spend, negative cost refused, cap clamp, JSON round trip keeps `int`), `CurrencyDef.format`, `Shop` (spread at 20 multipliers, stock, restock), `LootTable` (seeded weights, all-zero weights), `EconomyReport` | 15 of 15 | 0 |
| quest-system | `QuestLog` (prerequisites, accept once, subject filter, clamp, single completion, JSON round trip), tracker HUD, `QuestTimers` failure | 13 of 13 | 0 |

## Source claims corrected or dropped

The 4.7.2 doctool dump and measurements decided each one:

- **"32-bit `int` caps at about 2.1 billion"** (economy-system): wrong.
  GDScript `int` is 64-bit. The skill says so and moves big numbers to
  idle-game scale (`references/balancing.md`).
- **"Call `update()` after `set_point_solid()`"** (turn-system): wrong on
  4.7.2. Measured: `set_point_solid()` needs no `update()` and leaves
  `is_dirty()` false; `update()` with no changed setting keeps solid
  points; `update()` after a `region` change rebuilds the grid and clears
  solid points. The skill states the measured behavior.
- **`NavigationRegion3D.use_async_iterations`** (game-loop-waves): not a
  `NavigationRegion3D` member in 4.7.2 (the name exists only in
  `ProjectSettings` and the navigation servers). Dropped; async pathing is
  left to `godot-ai-navigation`.
- **`OS.get_ticks_msec()`** (game-loop-harvest, game-loop-time-trial): the
  method is `Time.get_ticks_msec()`. The skill uses `Time.*` throughout.
- **"`PlayerPrefs` (Godot's equivalent of Settings)"** (mechanic-secrets):
  not a Godot concept. Dropped; meta unlocks use `ConfigFile`.
- **"`MultiplayerSynchronizer` supports only primitive types"**
  (game-loop-waves): not verified, not carried.
- **Unmeasured performance numbers** ("10x faster" `AStarGrid2D`, "about 80%"
  smaller files): not carried.

## Not done

- No feel test (hit-stop length, knockback, ghost smoothness). These need a
  human play test.
- The proofs live in the git-ignored scratch folder; they are not in the
  gate. A later unit can move them into a test if the gate needs them.
- `godot-gameplay-loops` blocks for 3D nodes (`CollectiblePickup`,
  `HarvestNode`, `GhostTrack` playback on a node) were parse- and load-checked
  but not run in a scene.
