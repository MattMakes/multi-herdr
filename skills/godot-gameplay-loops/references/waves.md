# Wave loop

The player survives waves of enemies. A wave spawns over time, the wave ends
when every enemy of it is gone, a break follows, and the next wave starts.
Tower defense, arena survival and horde modes use this loop.

## Data

A wave is a resource, so a designer rebalances it without code
(`godot-resource-pattern`). One entry is "this many of this scene, this far
apart":

```gdscript
class_name WaveEntry
extends Resource

@export var enemy_scene: PackedScene
@export var count: int = 5
@export var interval_sec: float = 0.5
```

```gdscript
class_name WaveDefinition
extends Resource

@export var entries: Array[WaveEntry] = []
@export var break_after_sec: float = 5.0


func enemy_total() -> int:
	var total: int = 0
	for entry: WaveEntry in entries:
		total += entry.count
	return total
```

## The director

The director runs the waves in order. It counts live enemies with a number
that each enemy lowers when it leaves the tree. It never scans
`get_children()` or a group each frame.

```gdscript
class_name WaveDirector
extends Node

signal wave_started(number: int, enemy_total: int)
signal wave_cleared(number: int)
signal break_started(seconds: float)
signal all_waves_cleared

@export var waves: Array[WaveDefinition] = []
@export var spawn_points: Array[Marker3D] = []
@export var enemy_parent: Node

var alive: int = 0
var _wave_index: int = -1
var _spawning: bool = false
var _rng := RandomNumberGenerator.new()


func start_next_wave() -> void:
	if _spawning or alive > 0:
		return
	_wave_index += 1
	if _wave_index >= waves.size():
		all_waves_cleared.emit()
		return
	var wave: WaveDefinition = waves[_wave_index]
	wave_started.emit(_wave_index + 1, wave.enemy_total())
	_spawning = true
	for entry: WaveEntry in wave.entries:
		for i: int in entry.count:
			_spawn(entry.enemy_scene)
			await get_tree().create_timer(entry.interval_sec, false).timeout
			if not is_inside_tree():
				return
	_spawning = false
	_check_cleared()


func _spawn(scene: PackedScene) -> void:
	var enemy: Node3D = scene.instantiate()
	if not spawn_points.is_empty():
		var point: Marker3D = spawn_points[_rng.randi_range(0, spawn_points.size() - 1)]
		enemy.position = point.global_position
	enemy.add_to_group(&"enemies")
	enemy.tree_exited.connect(_on_enemy_gone, CONNECT_ONE_SHOT)
	alive += 1
	enemy_parent.add_child.call_deferred(enemy)


func _on_enemy_gone() -> void:
	alive -= 1
	_check_cleared()


func _check_cleared() -> void:
	if _spawning or alive > 0 or _wave_index < 0 or not is_inside_tree():
		return
	wave_cleared.emit(_wave_index + 1)
	var pause: float = waves[_wave_index].break_after_sec
	break_started.emit(pause)
	await get_tree().create_timer(pause, false).timeout
	if is_inside_tree():
		start_next_wave()
```

Notes:

- `create_timer(time, false)` sets `process_always` to `false`. The timer
  then stops while the tree is paused, so a pause menu also pauses the wave.
- `tree_exited` fires when the enemy is freed, and also when it is removed
  from the tree without a free. A pool that removes and re-adds enemies must
  reconnect the signal on each spawn, as `_spawn` does.
- The position is set before `add_child`. `enemy_parent` must sit at the
  world origin with no transform, or use `global_position` after the node
  enters the tree.
- `break_started` drives a countdown on the HUD. Players need a cue before a
  wave. A "start now" button calls `start_next_wave()` early; the guard at
  the top stops a double start.

## Weighted mixes

For variety, pick each enemy by weight instead of a fixed list. 4.3 and
later have `RandomNumberGenerator.rand_weighted(weights)`, which returns an
index:

```gdscript
extends Node

@export var enemy_scenes: Array[PackedScene] = []
@export var weights := PackedFloat32Array()

var rng := RandomNumberGenerator.new()


func pick_enemy() -> PackedScene:
	return enemy_scenes[rng.rand_weighted(weights)]
```

Set `rng.seed` from the run seed if replays or daily challenges must match.

## Endless mode

Build each `WaveDefinition` in code from the wave number: the count grows
(for example `base + number * 2`), the interval shrinks to a floor, and
stronger scenes join at fixed wave numbers. Give each spawned enemy its own
stats copy: `stats = base_stats.duplicate_deep()`. A shared stats resource
gives every enemy one health pool.

## Enemy death

When an enemy dies, turn off its collision in the same frame with
`$CollisionShape3D.set_deferred(&"disabled", true)`. A corpse with a live
shape blocks towers, bullets and paths. Free it when the death animation
ends.

## Scale

| Live enemies | Approach |
| --- | --- |
| up to about 100 | One node per enemy, as above. Pool the scenes if spawning stutters. |
| hundreds | Pool, and draw look-alike enemies with one `MultiMeshInstance3D`. Simplify their logic. |
| thousands | No node per enemy. Keep enemy data in packed arrays and use `PhysicsServer3D` and `NavigationServer3D` directly. |

The numbers are a starting point; measure with the profiler. Pools,
MultiMesh and server APIs are in `godot-optimization`. Async path queries
are in `godot-ai-navigation`.

## Online waves

Only the server runs the director and spawns enemies. A
`MultiplayerSpawner` copies spawned scenes to clients. `godot-multiplayer-sync`
covers this.

## Checks

- A wave of 3 enemies with interval 0: `alive` reaches 3, and
  `wave_cleared` fires after the third enemy is freed, not before.
- `start_next_wave()` twice in a row starts one wave.
- After the last wave clears, `all_waves_cleared` fires once.
