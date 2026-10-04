# Sandbox

Open-ended building and simulation: the player changes the world itself,
and systems interact without a script. Minecraft, Terraria, Noita and
Garry's Mod are the reference points.

## Core loop

Explore → gather or dig → build or combine → watch the systems react →
set a new goal. The player writes the goals; the game supplies the rules.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| World data | chunks of cell ids in packed arrays, not nodes | `godot-gdscript-advanced` |
| Chunk meshes | only exposed faces, rebuilt when a chunk changes | `godot-3d-essentials`, `godot-procedural-generation` |
| Generation | noise terrain, biomes, caves | `godot-procedural-generation` |
| Threads | meshing and generation in worker tasks | `godot-multithreading` |
| Material rules | interactions from properties (density, burns, melts) | this reference |
| Tools | brush, dig, place as interchangeable Resources | `godot-resource-pattern` |
| Gathering | dig or harvest a cell, yield, respawn | `godot-gameplay-loops` (harvest) |
| Saves | compressed chunk data, only changed chunks | `godot-save-load` |
| Multiplayer | server validates every edit | `godot-multiplayer-sync` |

## Scene tree (4.7)

```text
World (Node3D)
├── Chunks (Node3D; Chunk_x_y per loaded chunk)
│   └── Chunk (MeshInstance3D + StaticBody3D/CollisionShape3D)
├── ChunkManager (Node; data, dirty set, worker tasks)
├── Props (Node3D; RigidBody3D only for loose, moving things)
├── Player (CharacterBody3D) → Camera3D, ToolHolder
└── HUD (CanvasLayer; hotbar)
```

A 2D falling-sand game swaps the chunk meshes for an `Image` per chunk
shown on a `Sprite2D` or `TextureRect`, updated with `ImageTexture.update()`.

## Genre code

Store each chunk as one packed array of ids and save it run-length encoded.
Most of a chunk is air or stone, so it shrinks a lot.

```gdscript
extends RefCounted

const SIZE := 16
var cells := PackedInt32Array()   # SIZE^3 ids, x fastest
var dirty := false

func _init() -> void:
	cells.resize(SIZE * SIZE * SIZE)

func index(p: Vector3i) -> int:
	return p.x + SIZE * (p.y + SIZE * p.z)

func set_cell(p: Vector3i, id: int) -> void:
	cells[index(p)] = id
	dirty = true

func encode() -> PackedInt32Array:
	var out := PackedInt32Array()
	var i := 0
	while i < cells.size():
		var run := 1
		while i + run < cells.size() and cells[i + run] == cells[i]:
			run += 1
		out.append(cells[i])
		out.append(run)
		i += run
	return out
```

Find the cell under a point with `Vector3i((pos / cell_size).floor())`;
do not ray-cast against cells.

## Pitfalls

- A node or a body per block. Keep blocks as data; one mesh and one static
  collider per chunk.
- All faces drawn. Emit only faces next to air; merge equal faces (greedy
  meshing) for large worlds.
- Remeshing on every single edit while the player paints. Mark the chunk
  dirty and rebuild once per frame at most, in a worker task.
- The whole world simulated every tick. Simulate only chunks with
  activity.
- Hard-coded pair rules ("if water and fire"). Give materials properties
  and let rules read them.
- Scene tree changes from a worker. Build mesh arrays in the worker; assign
  the mesh on the main thread with `call_deferred`.
- Raw arrays in saves. Run-length encode; save only changed chunks.
- `MultiMesh.instance_count` changed often. Changing it reallocates the
  buffer; allocate enough once and set `visible_instance_count`.
- Client-side placement trusted in multiplayer. The server checks reach and
  materials.
- Precision loss on huge worlds. Shift the origin, as in `open-world.md`.
- Joints left after a body is freed. Free the joint with its bodies.
