# Real-time strategy

The player commands many units at once: gather resources, build a base,
train an army and fight for map control. StarCraft, Age of Empires and
They Are Billions are the reference points.

## Core loop

Gather → build → train → command in battle → expand to new resources →
repeat until one side's base falls.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Selection | drag box, click, shift-add, control groups | `godot-input-handling` |
| Commands | move, attack-move, gather, build; shift-queue | `godot-state-machine`, `godot-gdscript-patterns` (command pattern) |
| Unit movement | paths, avoidance, formations | `godot-ai-navigation` |
| Camera | edge scroll, pan, zoom, minimap jump | `godot-camera-system` |
| Economy | gather, carry, drop off, spend; supply cap | `godot-economy-system`, `godot-gameplay-loops` (harvest) |
| Building placement | grid snap, ghost preview, valid-area check | `godot-physics-system` |
| Fog of war | per-team vision mask | `godot-shader-basics` |
| Enemy commander | build orders, attack waves | `godot-limboai` or `godot-beehave` |
| Mass rendering | MultiMesh for large armies | `godot-optimization`, `godot-multithreading` |

## Scene tree (4.7)

```text
Battle (Node3D)
├── Map (Node3D; NavigationRegion3D baked from simple collision)
├── Resources (Node3D; group "resources")
├── Buildings (Node3D; group "buildings")
├── Units (Node3D; group "units")
│   └── Unit (CharacterBody3D) → NavigationAgent3D (avoidance on), Commands
├── Selection (Node; typed set of selected units, control groups)
├── Fog (SubViewport vision mask + overlay shader)
├── CameraRig (Node3D → Camera3D)
└── HUD (CanvasLayer; resources, minimap, command card)
```

## Genre code

A group move to one point makes every unit fight for the same spot. Keep
each unit's offset from the group's centre and send each unit to the
target plus its offset.

```gdscript
extends Node

func move_group(units: Array[Node3D], target: Vector3, max_spread: float = 6.0) -> void:
	if units.is_empty():
		return
	var center := Vector3.ZERO
	for u in units:
		center += u.global_position
	center /= units.size()
	for u in units:
		var offset := u.global_position - center
		offset.y = 0.0
		offset = offset.limit_length(max_spread)
		var agent := u.get_node(^"NavigationAgent3D") as NavigationAgent3D
		agent.target_position = target + offset
```

With avoidance on, a unit moves by the safe velocity: set
`agent.velocity` to the wanted velocity in `_physics_process()`, and move
in the `velocity_computed` signal handler.

## Pitfalls

- Every unit re-paths in the same frame. Stagger path updates with small
  random offsets.
- Avoidance on idle units. Turn it on while moving; static blockers use
  `NavigationObstacle3D`.
- A unit that never reaches an unreachable target. Add a timeout and
  return to idle.
- `_process()` on hundreds of units. Drive them from a manager, and only
  what needs a per-frame update.
- Hundreds of `MeshInstance3D` soldiers. Past a few hundred units, render
  with `MultiMeshInstance3D` and per-instance custom data, and path by
  squad, not per soldier.
- Selection read from the scene tree. Keep a typed set of selected units;
  saves and netcode need it.
- No command queue. Shift-click adds commands; store them as plain data.
- Too much micro. Give units auto-aggro range and automatic gather return.
- Shared stat Resources. An upgrade on one unit changes all. Duplicate per
  unit, or keep upgrades per team in one place on purpose.
- Fog computed per unit per tile on the CPU each frame. Draw vision circles
  into a mask texture and blend it in a shader.
- Navigation baked from detailed visual meshes. Bake from simple collision.
- A building ghost with collision. The ghost is visual only; test placement
  with a shape query.
