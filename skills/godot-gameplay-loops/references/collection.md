# Collection loop

The player finds every item of a set: hidden eggs, coins in a level, lore
pages, a "100%" archive. The loop is: the set is defined, the player picks up
items, progress shows, the set completes, and the state survives a reload.

## Parts

| Part | Job |
| --- | --- |
| `CollectionTracker` (autoload) | Owns each set: all ids and found ids. Emits progress. Saves and loads. |
| Pickup (`Area2D` / `Area3D`) | Has an exported `set_id` and `item_id`. Reports one pickup. Removes itself if already found. |
| Guide (compass, radar) | Asks the tracker which ids remain, then finds the nearest pickup with one of those ids. |
| Archive UI | Draws found and missing items from tracker signals. It never decides what is found. |

## The tracker

```gdscript
class_name CollectionTracker
extends Node

signal item_found(set_id: StringName, item_id: StringName)
signal progress_changed(set_id: StringName, found: int, total: int)
signal set_completed(set_id: StringName)

# set_id -> every item id in the set
var _members: Dictionary[StringName, PackedStringArray] = {}
# set_id -> found item ids, as a lookup table
var _found: Dictionary[StringName, Dictionary] = {}


func define_set(set_id: StringName, item_ids: PackedStringArray) -> void:
	_members[set_id] = item_ids.duplicate()
	if not _found.has(set_id):
		_found[set_id] = {}
	_emit_progress(set_id)


func collect(set_id: StringName, item_id: StringName) -> bool:
	if not _members.has(set_id):
		push_warning("collect: unknown set %s" % set_id)
		return false
	if not _members[set_id].has(item_id):
		push_warning("collect: %s is not in set %s" % [item_id, set_id])
		return false
	var found: Dictionary = _found[set_id]
	if found.has(item_id):
		return false
	found[item_id] = true
	item_found.emit(set_id, item_id)
	_emit_progress(set_id)
	if found.size() == _members[set_id].size():
		set_completed.emit(set_id)
	return true


func is_found(set_id: StringName, item_id: StringName) -> bool:
	return _found.has(set_id) and _found[set_id].has(item_id)


func remaining(set_id: StringName) -> PackedStringArray:
	var left := PackedStringArray()
	for id: String in _members.get(set_id, PackedStringArray()):
		if not is_found(set_id, StringName(id)):
			left.append(id)
	return left


func to_save() -> Dictionary:
	var out: Dictionary = {}
	for set_id: StringName in _found:
		out[String(set_id)] = PackedStringArray(_found[set_id].keys())
	return out


func from_save(data: Dictionary) -> void:
	for key: String in data:
		var table: Dictionary = {}
		for id: String in data[key]:
			table[StringName(id)] = true
		_found[StringName(key)] = table
		if _members.has(StringName(key)):
			_emit_progress(StringName(key))


func _emit_progress(set_id: StringName) -> void:
	var total: int = _members.get(set_id, PackedStringArray()).size()
	progress_changed.emit(set_id, _found.get(set_id, {}).size(), total)
```

Notes:

- `from_save` can run before `define_set`. The tracker keeps found ids for a
  set it has not seen yet, so the load order of levels does not matter.
- `set_completed` fires once, on the pickup that completes the set. A load
  of a complete set does not fire it again. The quest or reward code reads
  `remaining(set_id).is_empty()` after a load if it needs the state.

## The pickup

```gdscript
class_name CollectiblePickup
extends Area3D

@export var set_id: StringName
@export var item_id: StringName


func _ready() -> void:
	var tracker: CollectionTracker = get_node(^"/root/Collection")
	if tracker.is_found(set_id, item_id):
		queue_free()
		return
	body_entered.connect(_on_body_entered)


func _on_body_entered(_body: Node3D) -> void:
	var tracker: CollectionTracker = get_node(^"/root/Collection")
	if not tracker.collect(set_id, item_id):
		return
	set_deferred(&"monitoring", false)
	hide()
	# Start the pickup sound or particles here, then free on their end signal.
	queue_free()
```

The autoload name in this example is `Collection`. Put the pickup on a
collision layer that only the player's mask sees, so `body_entered` only
fires for the player.

## Ids

- Give every placed pickup an id in the editor. The id must be unique inside
  its set and must not change after release, because saves store it.
- For generated pickups, build the id from stable inputs:
  `StringName("%s_%d" % [room_id, index])`. Do not use `get_instance_id()`;
  it changes every run.
- Define the set from the data, not from the nodes in the open scene. A set
  can span many levels. Put the id list in a resource
  (`godot-resource-pattern`) and call `define_set` at boot.

## The guide

Ask the tracker for `remaining(set_id)`, then pick the nearest pickup node
in a group (for example `&"collectibles"`) whose `item_id` is in that list.
Rules:

- Throttle the search. Run it on a `Timer` (4 to 10 times a second), not
  every frame.
- Test the last item. A guide that filters on "more than one left" loses the
  final pickup. Write a check with exactly one remaining id.
- A pickup in an unloaded level has no node. The guide then points at a
  door or a map marker for that level, or shows nothing.

## Random placement

For a hunt with random spots, place `Marker3D` children as candidate spots
in the editor. At level load, shuffle them with a seeded
`RandomNumberGenerator` and spawn the set's pickups on the first N markers.
Store the seed in the save data, so a reload puts each pickup at the same
spot. Never place spawn points by coordinates in code.

## Archive UI

The archive draws each item as found or as a silhouette. It reads the state
from the tracker at open time and then follows `item_found`. A silhouette is
a `TextureRect` with `modulate = Color(0, 0, 0, 0.6)`; a found item uses
`Color.WHITE`. The UI holds no state of its own.

## Checks

- Collect an item twice: the second call returns `false`, and the count
  does not change.
- Collect an id outside the set: the call returns `false` with a warning.
- Save after 2 of 3, load into a new tracker: `remaining()` has 1 id.
- Collect the last item: `set_completed` fires once.
