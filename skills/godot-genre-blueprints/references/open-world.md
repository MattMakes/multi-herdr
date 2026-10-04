# Open world

A large continuous map the player crosses freely, streamed in pieces, with
places of interest close together. Breath of the Wild, Skyrim and The
Witcher 3 are the reference points.

## Core loop

Travel → discover a point of interest → do a quest or activity → the world
changes and remembers → travel on. Density matters more than size: aim for
something to notice every 30 seconds of travel.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Chunk streaming | load and free chunks around the player, threaded | `godot-scene-organization`, `godot-multithreading` |
| Precision far from origin | origin shift, or a double-precision build | this reference |
| Distance detail | visibility ranges (HLOD), MultiMesh foliage, occlusion | `godot-optimization`, `godot-3d-essentials` |
| Terrain and placement | heightmaps, GridMap, noise | `godot-procedural-generation`, `godot-3d-essentials` |
| Persistent changes | only the deltas: looted, killed, opened | `godot-save-load` |
| Quests and markers | quest log, compass, map | `godot-quest-system`, `godot-hud-system` |
| Day, night and weather | one clock broadcast to groups | `godot-event-bus` |
| Far AI | sleep or simplify outside the active area | `godot-ai-navigation` |

## Scene tree (4.7)

```text
World (Node3D)
├── WorldEnvironment + DirectionalLight3D (sun driven by the clock)
├── Streamer (Node; active chunk set, threaded loads)
├── Chunks (Node3D; one child per loaded chunk scene)
│   └── Chunk_3_-2 (Node3D)
│       ├── Terrain (MeshInstance3D + StaticBody3D)
│       ├── Foliage (MultiMeshInstance3D, visibility_range_end set)
│       ├── Props (HLOD: proxy mesh with detail children via visibility_parent)
│       └── Entities (each with a stable id for delta saves)
├── Player (CharacterBody3D)
└── HUD (CanvasLayer; compass, markers)
```

## Genre code

The streamer turns the player position into a chunk coordinate, and wants
every chunk within a radius. It requests missing chunks on a thread and adds
them on the main thread.

```gdscript
extends Node

@export var chunk_size := 128.0
@export var radius := 2
@export var chunk_root: Node3D

var loaded: Dictionary[Vector2i, Node3D] = {}
var pending: Dictionary[Vector2i, String] = {}

func update(player_pos: Vector3) -> void:
	var c := Vector2i(floori(player_pos.x / chunk_size), floori(player_pos.z / chunk_size))
	for x in range(c.x - radius, c.x + radius + 1):
		for y in range(c.y - radius, c.y + radius + 1):
			var key := Vector2i(x, y)
			if not loaded.has(key) and not pending.has(key):
				var path := "res://chunks/chunk_%d_%d.tscn" % [x, y]
				if ResourceLoader.exists(path):
					ResourceLoader.load_threaded_request(path)
					pending[key] = path
	for key in pending.keys():
		if ResourceLoader.load_threaded_get_status(pending[key]) == ResourceLoader.THREAD_LOAD_LOADED:
			var scene := ResourceLoader.load_threaded_get(pending[key]) as PackedScene
			var node := scene.instantiate() as Node3D
			chunk_root.add_child(node)
			loaded[key] = node
			pending.erase(key)
	for key in loaded.keys():
		if absi(key.x - c.x) > radius + 1 or absi(key.y - c.y) > radius + 1:
			loaded[key].queue_free()
			loaded.erase(key)
```

Unload one ring further out than you load, so the player does not thrash a
chunk at a border. Write a chunk's deltas before you free it.

## Pitfalls

- Saving the whole world. Save only what changed, keyed by chunk and entity
  id, in a binary file (`FileAccess.store_var()`).
- Jitter far from the origin. Single precision floats lose detail past
  several thousand units. Shift the world back to the origin when the
  player goes far, or build the engine with `precision=double` (large world
  coordinates); the official binaries are single precision.
- Two origin-shift systems on one world. Pick one owner.
- `load()` for chunks. It stalls the frame. Use threaded requests.
- Tree changes from a worker thread. Add nodes on the main thread.
- One huge collision mesh. Give each chunk its own.
- Thousands of `MeshInstance3D` trees. Use `MultiMeshInstance3D`.
- Moving `OccluderInstance3D` nodes at runtime. It rebuilds occlusion data
  each time; keep occluders static.
- `CSGShape3D` left in the shipped world. Bake it to a mesh.
- Shader compile stutter the first time an effect appears. Show new
  materials during a loading screen first.
- Path queries across the whole world. Limit them to the active region.
