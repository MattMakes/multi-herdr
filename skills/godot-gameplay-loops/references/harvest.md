# Harvest loop

The player hits a node (tree, rock, ore vein, crop) with a tool, the node
takes damage, the node breaks and gives a yield, and later the node comes
back. Idle and farming games add gains while the game is closed.

## Data

Two resources hold the numbers, so a designer tunes them in `.tres` files
(`godot-resource-pattern`):

```gdscript
class_name HarvestTool
extends Resource

enum Kind { AXE, PICKAXE, SICKLE }

@export var kind: Kind = Kind.AXE
@export_range(1, 10) var tier: int = 1
@export var power: int = 1
```

```gdscript
class_name HarvestSource
extends Resource

@export var source_id: StringName
@export var needs_kind: HarvestTool.Kind = HarvestTool.Kind.AXE
@export_range(1, 10) var min_tier: int = 1
@export var hits_to_break: int = 3
@export var yield_item: StringName
@export var yield_min: int = 1
@export var yield_max: int = 3
@export var respawn_seconds: int = 120
```

Use an `enum` for the tool kind. A string such as `"axe"` fails silently on
a typo.

## The node

The node keeps its own hit count, so many nodes share one `HarvestSource`
without changes to it.

```gdscript
class_name HarvestNode
extends StaticBody3D

signal hit_landed(hits_left: int)
signal harvested(item_id: StringName, amount: int)
signal hit_refused(reason: StringName)

@export var source: HarvestSource
@export var node_id: StringName

var _hits_left: int = 0
var _rng := RandomNumberGenerator.new()


func _ready() -> void:
	_hits_left = source.hits_to_break


func hit(tool: HarvestTool) -> void:
	if _hits_left <= 0:
		return
	if tool.kind != source.needs_kind:
		hit_refused.emit(&"wrong_tool")
		return
	if tool.tier < source.min_tier:
		hit_refused.emit(&"tier_too_low")
		return
	_hits_left = maxi(_hits_left - tool.power, 0)
	hit_landed.emit(_hits_left)
	if _hits_left == 0:
		var amount: int = _rng.randi_range(source.yield_min, source.yield_max)
		harvested.emit(source.yield_item, amount)
		_deplete()


func _deplete() -> void:
	# Hide and stop collisions now; free nothing. The respawn
	# registry decides when this node comes back.
	hide()
	$CollisionShape3D.set_deferred(&"disabled", true)


func restore() -> void:
	_hits_left = source.hits_to_break
	show()
	$CollisionShape3D.set_deferred(&"disabled", false)
```

The player finds the node with a ray or an interaction area and calls
`hit(equipped_tool)`. Connect `harvested` to the inventory
(`godot-inventory-system`) and `hit_refused` to feedback (a sound, a
"needs a better pickaxe" label).

## Respawn that survives a quit

Store the time each node broke, in Unix seconds, in an autoload. A node
that loads checks the registry and stays hidden until its time is up.

```gdscript
class_name HarvestRegistry
extends Node

# node_id -> Unix time when the node comes back
var _back_at: Dictionary[StringName, int] = {}


func mark_depleted(node_id: StringName, respawn_seconds: int) -> void:
	_back_at[node_id] = _now() + respawn_seconds


func is_available(node_id: StringName) -> bool:
	return _back_at.get(node_id, 0) <= _now()


func seconds_left(node_id: StringName) -> int:
	return maxi(_back_at.get(node_id, 0) - _now(), 0)


func to_save() -> Dictionary:
	return {"back_at": _back_at.duplicate()}


func from_save(data: Dictionary) -> void:
	_back_at.clear()
	var saved: Dictionary = data.get("back_at", {})
	for key: Variant in saved:
		_back_at[StringName(key)] = int(saved[key])


func _now() -> int:
	return int(Time.get_unix_time_from_system())
```

A JSON save turns `StringName` keys into strings and numbers into floats.
`from_save` converts them back. `godot-save-load` covers the file format.

## Offline gains

An idle loop pays for the time the game was closed:

1. On quit and on each autosave, store `Time.get_unix_time_from_system()`.
2. On load, compute `elapsed = now - saved`.
3. Clamp it: `clampi(elapsed, 0, max_offline_seconds)`. A negative value
   means the clock went back. A huge value means the clock jumped forward.
   The cap is a design number (for example 8 hours).
4. Pay `int(rate_per_second * elapsed)`, and show the player a summary.

Do not use `Time.get_ticks_msec()` or `Time.get_ticks_usec()` for this.
They count from engine start and reset on every launch.

## Rates while the game runs

A passive rate is a per-second float. Add it in `_physics_process` with
`delta`, keep the fraction in a float, and move whole units into the `int`
count:

```gdscript
extends Node

@export var rate_per_second: float = 0.5

var stored: int = 0
var _fraction: float = 0.0


func _physics_process(delta: float) -> void:
	_fraction += rate_per_second * delta
	if _fraction >= 1.0:
		var whole: int = floori(_fraction)
		stored += whole
		_fraction -= whole
```

A rate without `delta` runs faster on a faster machine.

## Clusters from noise

For ore veins and forests, sample a `FastNoiseLite` at candidate cells and
place a node where the value is above a threshold. Set `seed` from the world
seed and store that seed in the save data. Store depleted node ids, not the
nodes. A reload with the same seed rebuilds the same field, and the
registry hides the depleted ones. `Noise.get_noise_2d(x, y)` returns about
-1 to 1. `godot-procedural-generation` covers noise setup.

## Tool wear

Keep durability on a duplicate of the tool resource that the player owns,
not on the shared `.tres`. Lower it on each hit that lands, and emit a
signal at 0. A shared template with wear makes every copy of that tool break
together.

## Checks

- Wrong tool and low tier: `hit_refused` fires, and the hit count does not
  change.
- `hits_to_break` hits with power 1: `harvested` fires once with an amount
  in range.
- `mark_depleted` with 60 s, then `is_available` is `false`. A registry
  loaded from save data with a past time says `true`.
- Offline gain with a negative elapsed time pays 0.
