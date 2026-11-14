# Data and memory

Target engine: Godot 4.7. Which container or object type to store data in, how copies and sharing
really behave, how resources are cached and loaded, and how to find memory growth and leaks.

## Use this shelf when

- You pick a data type for many items: bullets, tiles, inventory entries, AI records.
- Two objects change "the same" resource or array by accident, or a copy is not deep enough.
- Memory grows over a session, or the exit log reports leaked instances or resources.
- You think about object pooling, preloading or keeping resources warm.
- You design a custom `Resource` that is edited in the Inspector and also changed at run time.

## How the engine works here

**Cost ladder of object types** (the docs rank them; they give no byte counts, so measure):

| Type | Freed by | Adds | Use for |
|---|---|---|---|
| `Object` | `free()` by hand | Properties, signals, script | Rare; internal records with a clear owner |
| `RefCounted` | Count reaching 0 | Reference counter | Most run-time data classes |
| `Resource` | Count reaching 0 | Path, cache entry, save and load, Inspector | Authored data, shared assets |
| `Node` | `free()` / `queue_free()` | Tree, groups, process, notifications | Things in the scene |

**Containers.** `Array` is a contiguous vector of `Variant` (24 bytes each on 64-bit), shared by
reference. A typed `Array[T]` is the same storage plus a type check on write. A packed array
(`PackedInt32Array`, `PackedVector3Array`, ...) stores raw values: 4 bytes per int32, 12 per
`Vector3` in a default build. `Dictionary` is a hash map from `Variant` to `Variant` that keeps
insertion order. `String` and `StringName` keys with the same text are the same key.

| Operation | Array | Dictionary |
|---|---|---|
| Iterate | Fastest | Fast |
| Get / set | By index, constant | By key, constant (hash per access) |
| Append / `pop_back` | Cheap | - |
| Insert, `erase`, `pop_front`, `remove_at` | Shifts the tail: O(n) | Constant |
| Find a value (`has`, `find`) | O(n) | `has(key)` constant; `find_key(value)` O(n) |

**Sharing.** `Array`, `Dictionary`, packed arrays and objects are passed by reference; assignment
never copies. Exception: an engine property or method that returns a packed array hands out a copy,
so change it and assign it back. Packed storage is the engine `Vector`, which is copy-on-write:
handing it to the engine is cheap; the real copy happens on the first write to a shared buffer.

**Resource cache.** `load()` of a path returns the one live instance for that path. The cache does
not own resources: when the last reference goes, the resource is freed and the next `load()` reads
the disk again. Sub-resources saved inside a scene or resource file are also shared, by every
instance of that scene.

## Advanced techniques

### Choose the container by element count and access pattern

For small sets use typed arrays and dictionaries for readability. From tens of thousands of
elements (the docs' own threshold), packed arrays win on memory, iteration and write speed.
Struct-of-arrays works well in GDScript: one `PackedVector2Array` for positions, one
`PackedFloat32Array` for lifetimes, one "alive count". Remove by swapping the last element in.

### Typed collections

`Array[T]` and, since 4.4, `Dictionary[K, V]` check every write and let the analyzer type reads
with `[]`. Limits: methods such as `get()`, `pop_back()`, `values()` still return `Variant`; nested
typed collections (`Array[Array[int]]`) are not supported; either side of a dictionary can be
`Variant`. An untyped array cannot be assigned to a typed variable: use `typed.assign(untyped)`,
which converts and reports bad elements. `is_same_typed` compares two containers' types.

### Know what a copy copies

| Call | Arrays and dictionaries inside | Resources inside |
|---|---|---|
| `Array.duplicate()` / `Dictionary.duplicate()` | Shared | Shared |
| `Array.duplicate(true)` | Copied, recursive | Shared |
| `Array.duplicate_deep(mode)` (since 4.5) | Copied | By `Resource.DeepDuplicateMode` |
| `Resource.duplicate()` | Shared | Shared |
| `Resource.duplicate(true)` | Copied | Only local ones (no path, or inside the same file) |
| `Resource.duplicate_deep(Resource.DEEP_DUPLICATE_ALL)` | Copied | All, also external files |

Since 4.5 `Resource.duplicate(true)` no longer copies external sub-resources; 4.4 did. Within one
deep copy a sub-resource referenced twice is copied once and stays shared inside the copy.
`duplicate()` fails on a custom resource whose `_init` has required parameters. Only stored
properties (`@export` or `PROPERTY_USAGE_STORAGE`) are copied; plain script variables are not.

### Per-instance resources without manual copies

Set `resource_local_to_scene = true` on a sub-resource (a material, a shape, a stats resource).
`PackedScene.instantiate()` then gives each scene instance its own copy and calls
`_setup_local_to_scene()` on it. Changing the flag at run time does nothing for existing copies.
Cost: one resource per instance, so sharing (and batching by material) is lost.

### Control the cache

`ResourceLoader.load(path, type_hint, cache_mode)`:

| Mode | Effect |
|---|---|
| `CACHE_MODE_REUSE` (default) | Return the cached instance if there is one. |
| `CACHE_MODE_IGNORE` | Fresh, private instance; dependencies still come from the cache. |
| `CACHE_MODE_IGNORE_DEEP` | Fresh instance and fresh dependencies. |
| `CACHE_MODE_REPLACE` | Reload from disk into the existing instance: every holder sees new data. |
| `CACHE_MODE_REPLACE_DEEP` | The same, also for dependencies. |

`CACHE_MODE_IGNORE` gives a throwaway copy of authored data. `CACHE_MODE_REPLACE` is live reload
for data files and mods. `take_over_path(path)` makes a resource built in code the cached answer
for a path. To keep an asset warm between levels, hold a reference in a long-lived object;
`ResourceLoader.has_cached(path)` tells whether a load will hit the disk.

### Threaded loading

`ResourceLoader.load_threaded_request(path, type_hint, use_sub_threads, cache_mode)` starts a load.
Poll `load_threaded_get_status(path, progress)` once per frame. `load_threaded_get(path)` blocks
when the load is not done, so call it only after `THREAD_LOAD_LOADED`. `use_sub_threads = true` is
faster but can slow the main thread: use it on a loading screen, not during play. Instancing the
loaded scene still runs on the main thread. Step-by-step code: `godot-scene-organization`.

### Break reference cycles

Two `RefCounted` objects that point at each other are never freed. Typical cases: parent and child
data records, a resource that stores its owner, a `Callable` with a bound `RefCounted` kept by that
same object. Make the back-pointer weak:

```gdscript
class_name ItemRecord
extends RefCounted

var children: Array[ItemRecord] = []
var _parent: WeakRef = null

func add(child: ItemRecord) -> void:
	children.append(child)
	child._parent = weakref(self)

func get_parent_record() -> ItemRecord:
	if _parent == null:
		return null
	return _parent.get_ref() as ItemRecord
```

`get_reference_count()` is the probe when an object will not die. Signal connections hold no
reference to the target.

### Object pooling: when it pays

GDScript objects are freed at once, with no collector, so pooling is not needed to avoid
collection pauses. The docs name pooling only as a workaround for C# collection pauses. Pool when a
profile shows that `instantiate()` plus tree entry is the cost (many spawns per frame, heavy scenes
with physics bodies or particles). A pooled node must be fully reset. Park it with `remove_child`
(tree entry is paid again) or keep it in the tree with `PROCESS_MODE_DISABLED` and `hide()`. Call
`request_ready()` if `_ready` must run again. For thousands of simple items, packed arrays drawn
with one `MultiMesh` need no nodes at all.

### Read the memory monitors

| Monitor | Meaning | Note |
|---|---|---|
| `Performance.MEMORY_STATIC` | Bytes from the engine allocator (CPU side) | Debug builds only |
| `Performance.OBJECT_COUNT` | Live `Object` instances | Rising without limit means a leak |
| `Performance.OBJECT_ORPHAN_NODE_COUNT` | Nodes outside the tree | 0 in release builds |
| `Performance.RENDER_VIDEO_MEM_USED` | GPU memory, total | Larger than textures plus buffers |
| `Performance.RENDER_TEXTURE_MEM_USED` | GPU textures | See also `RENDER_BUFFER_MEM_USED` |

Static and video memory are separate budgets: a texture is GPU side, an `Image` is CPU side.
`OS.get_static_memory_peak_usage()` gives the CPU-side peak (debug builds). Log the object counters
at the same point of each level loop; equal counts mean no leak.

### Find leaks

```gdscript
print("objects: ", Performance.get_monitor(Performance.OBJECT_COUNT))
print("orphans: ", Performance.get_monitor(Performance.OBJECT_ORPHAN_NODE_COUNT))
Node.print_orphan_nodes()
```

- Orphan nodes: `remove_child` without `free()`, `replace_by`, a scene instanced and never added.
  `Node.get_orphan_node_ids()` (since 4.5) returns the ids for a test assertion. Debug builds only.
- At exit the engine warns "ObjectDB instances leaked at exit" and "resources still in use at
  exit". Run with `--verbose` to get one line per leaked instance (class and id) and per resource
  path.
- Since 4.6 the Debugger has an ObjectDB Profiler tab: take a snapshot before and after a level
  loop, diff them by class, and follow Inbound References to see who keeps an object alive.
  Snapshots are `.odb_snapshot` files in `user://objectdb_snapshots/`. It does not see memory of
  unexposed native classes.

## Niche but useful

- `Array.make_read_only()` / `Dictionary.make_read_only()`: freeze shared config tables. `const`
  arrays and dictionaries are read-only already. Nested containers are not covered.
- `Dictionary.get_or_add(key, default)`: one lookup for "fetch or create" (grouping, counters).
- `is_same(a, b)`: true when two variables are the same array or dictionary, not equal copies.
- `Dictionary.sort()` (since 4.4): stable key order for deterministic iteration and text output.

## Traps

- Outdated belief (3.x): packed arrays are copied on assignment. In 4.x they are shared.
- Editing a loaded `Resource` at run time edits it for every user of that path until it is freed.
  Duplicate it for per-instance state, or use `resource_local_to_scene`.
- Erasing from an `Array` or `Dictionary` while iterating it gives undefined results. Iterate a
  copy or `keys()`, or go backwards by index.
- `Array[Node]` keeps entries for freed nodes. Filter with `is_instance_valid` before use.
- In C#, `RefCounted` objects are freed when the .NET collector runs, not at the last reference.
- The docs page on data preferences is marked outdated: its "objects are slower than dictionaries"
  ranking predates typed GDScript. Measure before you replace classes with dictionaries.

## Version notes

- 4.4: typed dictionaries; `Dictionary.sort`; `Array.find_custom`; `ResourceLoader.get_cached_ref`.
- 4.5: `duplicate_deep` on `Resource`, `Array`, `Dictionary`; `duplicate(true)` copies only local
  sub-resources; `Node.get_orphan_node_ids`.
- 4.6: ObjectDB Profiler; `Node.DUPLICATE_INTERNAL_STATE`.
- 4.7: writing one element of a packed-array property in GDScript no longer calls the property
  setter with the whole array.

## Related skills

- `godot-resource-pattern`: authoring custom resources step by step.
- `godot-optimization`: general memory and CPU tuning workflow.
- `godot-save-load`: serializing these containers to disk.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/best_practices/data_preferences.html
- https://docs.godotengine.org/en/stable/tutorials/best_practices/node_alternatives.html
- https://docs.godotengine.org/en/stable/tutorials/io/background_loading.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/debug/objectdb_profiler.html
- https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.5.html
- https://github.com/godotengine/godot/blob/4.7-stable/core/io/resource.cpp
