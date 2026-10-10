# Survival / crafting

The player keeps a character alive in a hostile world by gathering,
crafting and building. Don't Starve, The Forest, Valheim and Rust are the
reference points.

## Core loop

Check needs → gather → craft better tools → build shelter and storage →
survive the night or a threat → push into a harder area or tier.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Needs | hunger, thirst, warmth, stamina; decay scaled by activity | this reference |
| Inventory | stack limits or weight, durability per item | `godot-inventory-system` |
| Crafting | recipes as data, discovery when materials are found | `godot-resource-pattern` |
| Gathering | harvestable nodes, yield scaled by tool tier | `godot-gameplay-loops` (harvest), `godot-component-system` |
| Building | grid or socket snapping, placement checks | `godot-3d-essentials` (GridMap), `godot-physics-system` |
| World | noise terrain, biomes, threaded chunk generation | `godot-procedural-generation`, `godot-multithreading` |
| Day and night | one clock; spawns and temperature follow it | `godot-event-bus` |
| Threats | spawners with safe zones, AI | `godot-ai-navigation` |
| Saves | world changes, base, inventory | `godot-save-load` |

## Scene tree (4.7)

```text
World (Node3D)
├── Clock (autoload; day, hour, signals)
├── Terrain (chunks; MultiMeshInstance3D for trees and rocks)
├── Harvestables (Node3D; each with a ResourceNode component)
├── Base (Node3D; GridMap or snapped building pieces)
├── Spawners (Node3D; check distance to beds before spawning)
├── Player (CharacterBody3D)
│   ├── Needs (Node; the component below)
│   └── Inventory (Node; holds an inventory Resource)
└── HUD (CanvasLayer; need bars, hotbar, crafting panel)
```

## Genre code

Needs drain faster when the player works harder, and an empty need drains
health slowly with a warning, never an instant death.

```gdscript
extends Node

signal need_changed(need: StringName, value: float)
signal starving(need: StringName)

const BASE_DRAIN := {&"hunger": 0.12, &"thirst": 0.2}   # per second at rest
const ACTIVITY := {&"idle": 1.0, &"walk": 1.3, &"sprint": 3.0}

var values := {&"hunger": 100.0, &"thirst": 100.0}
var activity: StringName = &"idle"
var health := 100.0

func _physics_process(delta: float) -> void:
	var mult: float = ACTIVITY.get(activity, 1.0)
	for need in values:
		var before: float = values[need]
		var now := maxf(before - BASE_DRAIN[need] * mult * delta, 0.0)
		values[need] = now
		if not is_equal_approx(before, now):
			need_changed.emit(need, now)
		if now <= 0.0:
			health -= 0.5 * delta
			starving.emit(need)
```

The movement controller sets `activity`. The HUD warns at about 25%, and
again at 0, with sound and screen effects.

## Pitfalls

- Constant decay. Scale it with what the player does.
- Instant death at zero. Drain health and warn first.
- Gathering that never gets faster. Tie yield to tool tier (stone axe 1,
  steel axe 5).
- Unlimited stacks. Use stack limits or weight, and give storage.
- Hidden recipes. Unlock recipes when the player first holds a material.
- One durability Resource shared by every axe. Duplicate items when they
  enter an inventory.
- Item and recipe data in node properties. Use Resources or data files that
  designers and modders can edit.
- Threats that spawn at the bed. Reject spawn points inside a safe radius
  around beds and the respawn point.
- Ten thousand tree nodes. Use `MultiMeshInstance3D` and swap a tree to a
  real node only when the player hits it.
- Noise generation on the main thread. Use worker tasks; add nodes on the
  main thread.
- Time of day kept in a UI script. Keep it in the clock autoload.
- A 2D survival game on the old `TileMap`. Use `TileMapLayer` nodes.
- No goals beyond survival. Add a tech tree, bosses or a destination.
