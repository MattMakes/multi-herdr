# Tower defense

Enemies walk a path in waves; the player spends currency on towers that
stop them. Bloons TD, Kingdom Rush and Fieldrunners are the reference
points. "Mazing" variants let towers block and reshape the path.

## Core loop

Prepare (build and upgrade) → a wave walks the path → towers target and
fire → kills pay currency → the next wave is harder → repeat until the last
wave or the base falls.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Paths | `Path2D` with `PathFollow2D` for fixed lanes, or grid paths for mazing | `godot-2d-essentials`, `godot-ai-navigation` |
| Waves | wave Resources: enemy type, count, interval, delay | `godot-gameplay-loops` (waves), `godot-resource-pattern` |
| Towers | idle → acquire → wind-up → fire states, upgrades | `godot-state-machine` |
| Targeting | first, last, strongest, weakest; range cache | this reference |
| Projectiles | pooled nodes; server-level bodies for very large counts | `godot-combat-system`, `godot-optimization` |
| Economy | kill bounties, interest, early-call bonus, sell value | `godot-economy-system` |
| Placement | grid snap, valid cells, path-blocking check | `godot-input-handling` |
| HUD | money, lives, wave counter, tower panel | `godot-hud-system` |

## Scene tree (4.7)

```text
Level (Node2D)
├── Ground (TileMapLayer; buildable cells marked in custom data)
├── Lane (Path2D)
│   └── (PathFollow2D per enemy, created by the spawner)
├── WaveSpawner (Node; plays an Array of wave Resources)
├── Towers (Node2D)
│   └── Tower (Node2D) → Range (Area2D), Turret (Sprite2D), Muzzle (Marker2D)
├── Projectiles (Node2D; pool)
└── HUD (CanvasLayer)
```

## Genre code

Each enemy rides a `PathFollow2D`, so its `progress` is the distance
walked. "First" is the highest progress in range; no path search needed.
Keep the in-range list from area signals, not a query every frame.

```gdscript
extends Node2D

@export var mode: StringName = &"first"
@export var acquire_every_frames := 6
var in_range: Array[Node2D] = []
var target: Node2D = null

func _on_range_area_entered(area: Area2D) -> void:
	in_range.append(area.get_parent())

func _on_range_area_exited(area: Area2D) -> void:
	in_range.erase(area.get_parent())

func _physics_process(_delta: float) -> void:
	if Engine.get_physics_frames() % acquire_every_frames != 0:
		return
	target = null
	var best := -INF
	for e in in_range:
		if not is_instance_valid(e):
			continue
		var follow := e.get_parent() as PathFollow2D
		var score := 0.0
		match mode:
			&"first": score = follow.progress
			&"last": score = -follow.progress
			&"strongest": score = e.get_meta(&"hp", 0.0)
			&"weakest": score = -e.get_meta(&"hp", 0.0)
		if score > best:
			best = score
			target = e
```

Here each enemy is a child of its `PathFollow2D` and keeps its health in
metadata; with a typed enemy class, read the property instead.

## Pitfalls

- `get_overlapping_areas()` every frame on every tower. Cache with
  enter and exit signals, and acquire every few frames.
- Towers that all do the same thing. Give each a role: slow, armour pierce,
  anti-air, burst, splash.
- A death spiral. Add interest on saved money, or a small comeback bonus.
- Busywork early waves. Offer an early-call bonus.
- Mazing that can seal the exit. Before you accept a tower, test that a path
  still exists (`AStarGrid2D` or a navigation path query).
- Navigation rebaked on the main thread after each tower. Bake
  asynchronously, or use a grid search.
- Hundreds of projectile nodes created and freed. Pool them; for thousands,
  use physics server bodies or simple math projectiles.
- Enemies freed while physics still uses them. Disable their collision with
  `set_deferred("disabled", true)` and free them with `queue_free()`.
- Waves in a long `match` block. Use wave Resources.
- Scaled range shapes. Change the shape's radius.
- Co-op money changed on the client. The server owns money.
