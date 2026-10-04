# Copies, the resource cache, constructors and what a Resource must not hold

Adds the measured rules for copying resources on Godot 4.5 and later, the
resource cache and its modes, `resource_local_to_scene`, the constructor
rule for custom resources, and what to store instead of a node. Read it
before you copy, reload or construct a custom `Resource`. Every behavior
below was measured on Godot 4.7.2 unless a version is named.

## Copies: `duplicate()`, `duplicate(true)`, `duplicate_deep()`

A sub-resource is either *embedded* (saved inside the parent `.tres` as a
`[sub_resource]`) or *external* (its own `.tres`, referenced by path).

| Call | Embedded sub-resource | External sub-resource |
| --- | --- | --- |
| `duplicate()` | shared | shared |
| `duplicate(true)` | copied | **shared** |
| `duplicate_deep()` (`DEEP_DUPLICATE_INTERNAL`) | copied | shared |
| `duplicate_deep(Resource.DEEP_DUPLICATE_ALL)` | copied | copied |

Since Godot 4.5, `duplicate(true)` no longer copies external
sub-resources. The 4.5 upgrade guide names `duplicate_deep(DEEP_DUPLICATE_ALL)`
as the way to get the old behavior. GW10 (opus-83) checked the same fact
against the godot-docs 4.7 branch.

```gdscript
class_name Loadout
extends Resource

@export var name: String = ""
@export var weapon: Resource


func unique_copy() -> Loadout:
	# Copies the weapon even when it is its own .tres file.
	return duplicate_deep(Resource.DEEP_DUPLICATE_ALL) as Loadout
```

Copy only what an instance changes. A full deep copy of a large tree of
definitions costs memory for every enemy that spawns.

## `resource_local_to_scene`

Set `resource_local_to_scene = true` on a resource (Inspector: Resource >
Local To Scene) and each instance of a scene that uses it gets its own copy
when the scene is instantiated. It suits a resource embedded in a scene,
such as a material whose color each instance changes. For data an entity
loads by path at run time, call `duplicate()` in code: that is easier to
see in review.

## The resource cache

`load()` returns the cached object for a path. Two `load()` calls of one
path give the same object, so a change to one is a change to both, until
the last reference goes.

| `ResourceLoader.load(path, "", mode)` | Result |
| --- | --- |
| `CACHE_MODE_REUSE` (the default) | The cached object if there is one |
| `CACHE_MODE_IGNORE` | A fresh object from disk. The cache is not used or updated. |
| `CACHE_MODE_REPLACE` | A fresh object from disk, which replaces the cached one |
| `CACHE_MODE_IGNORE_DEEP`, `CACHE_MODE_REPLACE_DEEP` | The same, for every sub-resource too |

Measured: after a change to a cached resource, `load()` returned the changed
object, and `CACHE_MODE_IGNORE` returned the value on disk.

```gdscript
class_name DefinitionStore
extends RefCounted


static func pristine(path: String) -> Resource:
	# The on-disk state, not what the running game changed.
	return ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_IGNORE)


static func reload(path: String) -> Resource:
	# A file changed on disk (a hot-reload tool): replace the cached copy.
	return ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_REPLACE)
```

## Constructors must work with no arguments

Godot creates a custom resource with no arguments when it loads a `.tres`,
when the Inspector makes a new one, and in `duplicate()`. A `_init()` with
a required parameter breaks all of these. Measured: loading a saved `.tres`
of such a class printed `Method expected 1 argument(s), but called with 0`
and returned a plain `Resource` without the script. The same holds for a
node script attached to a scene.

Give every parameter a default, and set fields after `new()` where you can:

```gdscript
class_name LootDrop
extends Resource

@export var item_id: StringName = &""
@export var amount: int = 1


func _init(p_item_id: StringName = &"", p_amount: int = 1) -> void:
	item_id = p_item_id
	amount = p_amount
```

## What a Resource must not hold

- **A node.** A node is not saved with a resource, and it is freed with its
  scene while the resource lives on. Store a `NodePath`, a unique name or an
  id, and look the node up at run time.
- **Per-frame values.** Velocity, timers and the like belong in the node
  that uses them. A resource is shared data, not a scratch pad.
- **A reference cycle.** Resources are reference-counted. When A holds B and
  B holds A, neither is ever freed. Keep references one way (parent to
  child), and use an id for the way back.

## Resource, RefCounted or Node

| Need | Use |
| --- | --- |
| Data edited in the Inspector, saved to disk, shared | `Resource` |
| A helper object for a calculation, never saved | `RefCounted` |
| Behavior in the tree: process, input, physics, rendering | `Node` |

A `RefCounted` helper costs less than a `Resource` and does not show up in
the resource picker by mistake.

## Large data sets

One `.tres` per row is fine for hundreds of items. For many thousands of
rows that a tool generates, one data file (JSON, CSV or a single `.res`
that holds an array) loads faster and is easier to regenerate. Keep the
`Resource` classes as the typed in-memory form, and build them from the file
at load time.
