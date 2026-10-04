# Resources that report their own changes, and checks in setters

Adds `emit_changed()` and the `changed` signal for live data, the trap with
in-place edits of arrays and dictionaries, value checks in setters, and a
headless check of every resource in a folder. Read it when a UI or another
system must follow a resource's values, or when a designer-edited resource
needs bounds.

## `emit_changed()` and `changed`

Every `Resource` has a `changed` signal. A custom resource emits it from
its setters:

```gdscript
class_name WalletData
extends Resource

@export var gold: int = 0:
	set(value):
		var clamped := maxi(value, 0)
		if gold == clamped:
			return
		gold = clamped
		emit_changed()

@export var gems: int = 0:
	set(value):
		var clamped := maxi(value, 0)
		if gems == clamped:
			return
		gems = clamped
		emit_changed()
```

A label follows it with no extra signal on the owner:

```gdscript
extends Label

@export var wallet: WalletData


func _ready() -> void:
	if wallet == null:
		return
	wallet.changed.connect(_refresh)
	_refresh()


func _refresh() -> void:
	text = "%d gold, %d gems" % [wallet.gold, wallet.gems]
```

The clamp comes first, and the equality check after it stops an emit when nothing changed. That
keeps a loop of "A changed, so set B, so A changed" from starting.

## In-place edits do not run the setter

`items.append(x)` or `stats["hp"] = 5` changes the array or dictionary in
place. The property setter does not run, so no `changed` is emitted.
Change collections through methods that emit:

```gdscript
class_name Backpack
extends Resource

@export var item_ids: Array[StringName] = []


func add(item_id: StringName) -> void:
	item_ids.append(item_id)
	emit_changed()


func remove(item_id: StringName) -> bool:
	var index := item_ids.find(item_id)
	if index == -1:
		return false
	item_ids.remove_at(index)
	emit_changed()
	return true
```

For a packed array property (`PackedInt32Array` and the like), Godot 4.7
also stopped calling the setter on an element write. The
`godot-gdscript-advanced` skill shows the copy-and-reassign fix.

## Bounds and checks in setters

Clamp a designer value in the setter, so a bad value never reaches the
game. Add `@tool` when the check must also run in the Inspector:

```gdscript
@tool
class_name SpawnRule
extends Resource

@export_range(0.0, 1.0, 0.01) var chance: float = 0.5:
	set(value):
		chance = clampf(value, 0.0, 1.0)
		emit_changed()

@export var min_count: int = 1:
	set(value):
		min_count = maxi(value, 0)
		if max_count < min_count:
			max_count = min_count
		emit_changed()

@export var max_count: int = 3:
	set(value):
		max_count = maxi(value, min_count)
		emit_changed()
```

`@export_range` limits the Inspector slider only. Code and hand-edited
`.tres` files can still set any value, so the setter is the real guard.

## Check every resource in a folder

A headless script proves that each `.tres` in a folder loads as the
expected class. Run it after a bulk edit or a hand edit of `.tres` text.

```gdscript
extends SceneTree

const FOLDER := "res://data/items"
const EXPECTED := &"ItemData"


func _initialize() -> void:
	var bad: PackedStringArray = []
	var checked := 0
	for file in ResourceLoader.list_directory(FOLDER):
		if not (file.ends_with(".tres") or file.ends_with(".res")):
			continue
		checked += 1
		var path := FOLDER.path_join(file)
		var res := ResourceLoader.load(path)
		var script := res.get_script() as Script if res != null else null
		if script == null or script.get_global_name() != EXPECTED:
			bad.append(path)
	if not bad.is_empty():
		push_error("not %s: %s" % [EXPECTED, ", ".join(bad)])
		quit(1)
		return
	print("%d resources OK" % checked)
	quit(0)
```

`ResourceLoader.list_directory()` lists the resources the loader can load
in that folder. Prefer it to a `DirAccess` walk for resource files.
