# Roguelike / roguelite

Runs through generated levels with permanent death; a roguelite adds meta
progression between runs. Hades, Dead Cells, The Binding of Isaac and Spelunky
are the reference points.

## Core loop

Prepare (pick a character, spend meta currency) → run through generated
floors → collect temporary power (items, relics, upgrades) → die or win → bank
meta currency → start a new run.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Generation | rooms stitched at connectors, walker, BSP or noise | `godot-procedural-generation` |
| Seeded randomness | one seed per run, separate streams per system | `godot-procedural-generation` |
| Run state vs meta state | two owners; death clears only the run | `godot-save-load`, `godot-dependency-injection` |
| Combat | real-time or turn-based | `godot-combat-system` |
| Items and synergies | relics as Resources with hooks and tags | `godot-resource-pattern`, `godot-ability-system` |
| Pathing on generated maps | `AStarGrid2D`, or a nav mesh baked after generation | `godot-ai-navigation` |
| Heavy generation off the main thread | worker tasks, then build nodes on the main thread | `godot-multithreading` |
| Shops and currency | per run and meta | `godot-economy-system` |

## Scene tree (4.7)

```text
Game (Node)
├── Meta (autoload; unlocks, meta currency; saved to user://)
├── Run (autoload; seed, floor, HP, gold, relics; reset on death)
└── Floor (Node2D; rebuilt each floor)
    ├── Rooms (Node2D; pre-made room scenes with Marker2D connectors)
    ├── Navigation (NavigationRegion2D, baked after rooms are placed)
    ├── Enemies (Node2D)
    ├── Pickups (Node2D)
    └── Player (CharacterBody2D)
```

## Genre code

Give each system its own random stream derived from the run seed. Then
opening an extra chest does not change the next floor's layout, and a
shared seed reproduces the same run.

```gdscript
extends Node

var run_seed := 0
var _streams: Dictionary[StringName, RandomNumberGenerator] = {}

func start_run(seed_value: int) -> void:
	run_seed = seed_value
	_streams.clear()

func stream(system: StringName, floor_index: int = 0) -> RandomNumberGenerator:
	var key := StringName("%s:%d" % [system, floor_index])
	if not _streams.has(key):
		var rng := RandomNumberGenerator.new()
		rng.seed = hash([run_seed, system, floor_index])
		_streams[key] = rng
	return _streams[key]
```

Call `stream(&"layout", floor)`, `stream(&"loot", floor)` and
`stream(&"enemies", floor)` separately. Store `run_seed` with the run save,
and show it so players can share runs.

## Pitfalls

- Global `randi()` or `Array.pick_random()` for content. They use the
  global generator; runs stop being reproducible. Use the streams.
- Pure luck decides the run. Add mitigation: shops, rerolls, pity counters.
  For critical drops, a shuffle bag stops long bad streaks.
- Run data that leaks into the meta save, or the reverse. Keep two owners
  and two files.
- Meta upgrades that are too strong. Keep each small (5 to 15%), so skill
  still decides the run.
- Save scumming. Save the run on floor change and on quit; delete or mark
  the save when it is loaded.
- Generation on the main thread. Build the layout as plain data in a
  `WorkerThreadPool` task; create nodes on the main thread.
- Rooms placed by pixel guesses. Pre-make rooms with `Marker2D` connectors
  and align connector to connector.
- Navigation that ignores new rooms. Bake the region after generation.
- `AStarGrid2D` with stale solids. Call `update()` after changing the region
  or cell size, then set solids. `jumping_enabled` speeds up open grids but
  ignores weights.
- A Manhattan heuristic with 8-way moves. Use octile or Chebyshev.
- Shared stat Resources: one hit damages every enemy. Duplicate on spawn.
- Run state saved as `.tscn`. Save plain data to `user://`.
- Not enough content. Generation only rearranges content; plan many rooms,
  enemies and items.
