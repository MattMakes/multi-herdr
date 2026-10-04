---
name: godot-quest-system
description: "Use when building quests in Godot 4.7 with GDScript: quest and objective definitions as resources, a quest log autoload that keeps run-time progress apart from the definitions, objectives driven by game events (kill, collect, talk, reach), prerequisites and quest chains, rewards granted once, quest save data as ids and counts, branching outcomes, failure, timed quests, hidden objectives, a quest tracker HUD, waypoints, quest givers and localized quest text. Dialogue is in godot-dialogue-system; rewards go through godot-economy-system and godot-inventory-system."
---

# Godot quest system

Target engine: **Godot 4.7**. Every code block is typed GDScript that parses
on 4.7.2. Every engine API it names is in the 4.7.2 `--doctool` dump.

## Rules

- **Definitions are read-only data.** A `QuestDef` resource in a `.tres` file
  says what the quest is. Run-time progress never goes into it: a loaded
  `.tres` is shared, and a change to it leaks into every user of the file and
  into a later save.
- **One quest log owns progress.** An autoload holds the status and the
  objective counts of every quest, keyed by `StringName` id. The player node
  holds no quest state; a scene change would lose it.
- **Events in, signals out.** Game code reports what happened
  ("killed a slime", "talked to Mara") on an event bus (`godot-event-bus`).
  The quest log decides which objectives that event advances. Enemy and item
  scripts know nothing about quests. Nothing polls objectives in `_process`.
- **Ids are `StringName`.** `&"kill_slimes"`, not a free string, and one id
  per quest, unique in the game.
- **One active copy per quest.** Accepting a quest that is active or done
  does nothing.
- **Rewards are granted once, outside the quest.** The log emits
  `quest_completed`; a reward handler grants gold and items through the
  wallet and inventory. A status check (only `ACTIVE` can complete) stops a
  double reward.
- **Save ids and counts.** The save holds `{quest id: status, objective
  counts}`, never the resources. A load reads the definitions from disk again.

## Definitions

```gdscript
class_name ObjectiveDef
extends Resource

@export var id: StringName
## The event that advances it: &"kill", &"collect", &"talk", &"reach".
@export var event: StringName
## What the event must be about: an enemy kind, an item id, an NPC id.
@export var subject: StringName
@export var required: int = 1
@export var hidden: bool = false
@export var optional: bool = false
## Localization key of the text, for example "QUEST_SLIMES_OBJ_1".
@export var text_key: String
```

```gdscript
class_name QuestDef
extends Resource

@export var id: StringName
@export var title_key: String
@export var objectives: Array[ObjectiveDef] = []
@export var prerequisites: Array[StringName] = []
## Reward data only. The quest log never grants it.
@export var reward_currency: Dictionary[StringName, int] = {}
@export var reward_items: Dictionary[StringName, int] = {}
@export var time_limit_sec: float = 0.0  # 0 means no limit
```

## The quest log

```gdscript
class_name QuestLog
extends Node

enum Status { LOCKED, AVAILABLE, ACTIVE, COMPLETED, FAILED }

signal quest_accepted(quest_id: StringName)
signal objective_progressed(quest_id: StringName, objective_id: StringName, count: int, required: int)
signal quest_completed(quest_id: StringName, def: QuestDef)
signal quest_failed(quest_id: StringName)

var _defs: Dictionary[StringName, QuestDef] = {}
var _status: Dictionary[StringName, Status] = {}
## quest id -> objective id -> count
var _counts: Dictionary[StringName, Dictionary] = {}


func register(def: QuestDef) -> void:
	_defs[def.id] = def
	if not _status.has(def.id):
		_status[def.id] = Status.LOCKED


func definition(quest_id: StringName) -> QuestDef:
	return _defs.get(quest_id)


func quest_ids() -> Array[StringName]:
	return _defs.keys()


func status(quest_id: StringName) -> Status:
	if not _status.has(quest_id):
		return Status.LOCKED
	if _status[quest_id] == Status.LOCKED and _prerequisites_met(quest_id):
		return Status.AVAILABLE
	return _status[quest_id]


func accept(quest_id: StringName) -> bool:
	if status(quest_id) != Status.AVAILABLE:
		return false
	_status[quest_id] = Status.ACTIVE
	_counts[quest_id] = {}
	quest_accepted.emit(quest_id)
	return true


## The event bus calls this for every quest-relevant event.
func notify(event: StringName, subject: StringName, amount: int = 1) -> void:
	for quest_id: StringName in _status.keys():
		if _status[quest_id] != Status.ACTIVE:
			continue
		var counts: Dictionary = _counts[quest_id]
		for obj: ObjectiveDef in _defs[quest_id].objectives:
			if obj.event != event or obj.subject != subject:
				continue
			var before: int = counts.get(obj.id, 0)
			if before >= obj.required:
				continue
			counts[obj.id] = mini(before + amount, obj.required)
			objective_progressed.emit(quest_id, obj.id, counts[obj.id], obj.required)
		_try_complete(quest_id)


func fail(quest_id: StringName) -> void:
	if _status.get(quest_id, Status.LOCKED) == Status.ACTIVE:
		_status[quest_id] = Status.FAILED
		quest_failed.emit(quest_id)


func count(quest_id: StringName, objective_id: StringName) -> int:
	return _counts.get(quest_id, {}).get(objective_id, 0)


func to_save() -> Dictionary:
	var out: Dictionary = {}
	for quest_id: StringName in _status:
		if _status[quest_id] == Status.LOCKED:
			continue
		var counts: Dictionary = {}
		for obj_id: StringName in _counts.get(quest_id, {}):
			counts[String(obj_id)] = _counts[quest_id][obj_id]
		out[String(quest_id)] = {"status": int(_status[quest_id]), "counts": counts}
	return out


## Call after every register(). Unknown ids from an old save are skipped.
func from_save(data: Dictionary) -> void:
	for key: Variant in data:
		var quest_id := StringName(key)
		if not _defs.has(quest_id):
			push_warning("quest save: unknown quest %s" % quest_id)
			continue
		var entry: Dictionary = data[key]
		_status[quest_id] = int(entry.get("status", Status.LOCKED)) as Status
		var counts: Dictionary = {}
		var saved: Dictionary = entry.get("counts", {})
		for obj_key: Variant in saved:
			counts[StringName(obj_key)] = int(saved[obj_key])
		_counts[quest_id] = counts


func _prerequisites_met(quest_id: StringName) -> bool:
	for pre: StringName in _defs[quest_id].prerequisites:
		if _status.get(pre, Status.LOCKED) != Status.COMPLETED:
			return false
	return true


func _try_complete(quest_id: StringName) -> void:
	for obj: ObjectiveDef in _defs[quest_id].objectives:
		if not obj.optional and count(quest_id, obj.id) < obj.required:
			return
	_status[quest_id] = Status.COMPLETED
	quest_completed.emit(quest_id, _defs[quest_id])
```

Register it as an autoload (for example `Quests`), and register every
`QuestDef` at boot from a list resource or a folder scan. Notes:

- `status()` computes `AVAILABLE` from the prerequisites, so a quest unlocks
  as soon as its last prerequisite completes, with no extra bookkeeping.
- `notify` loops over a copy of the keys (`keys()` returns a new array), so a
  listener that accepts a quest during the signal does not break the loop.
- An objective that the player already finished before accepting (an item in
  the bag) needs an explicit check at `accept`: count the items in the
  inventory and call `notify(&"collect", item, n)`. Decide per objective
  whether items collected earlier count.
- JSON turns the enum into a float; `from_save` converts it back with `int()`.

## Wiring

```gdscript
extends Node

# An autoload that turns game signals into quest events and grants rewards.

@export var quests: QuestLog
@export var wallet: Node


func _ready() -> void:
	quests.quest_completed.connect(_on_quest_completed)


func on_enemy_killed(enemy_kind: StringName) -> void:
	quests.notify(&"kill", enemy_kind)


func on_item_picked(item_id: StringName, amount: int) -> void:
	quests.notify(&"collect", item_id, amount)


func _on_quest_completed(_quest_id: StringName, def: QuestDef) -> void:
	for currency: StringName in def.reward_currency:
		wallet.call(&"grant", currency, def.reward_currency[currency], &"quest")
	# Grant def.reward_items through the inventory (godot-inventory-system).
```

The enemy emits `died(kind)` on the event bus; this node listens and calls
`notify`. The wallet is the one in `godot-economy-system`.

## More

| Task | Read |
| --- | --- |
| Branching outcomes, failure, timed quests, hidden objectives, chains, repeatable quests | [references/branching-and-timing.md](references/branching-and-timing.md) |
| Quest tracker HUD, waypoints, quest givers and dialogue, localized text | [references/ui-and-world.md](references/ui-and-world.md) |

## Other skills own these parts

| Need | Skill |
| --- | --- |
| Conversations that offer and complete quests | `godot-dialogue-system`, `godot-dialogue-manager` |
| Game events between systems | `godot-event-bus` |
| Item rewards and "collect N" counts | `godot-inventory-system` |
| Currency rewards | `godot-economy-system` |
| Writing the quest log's save data | `godot-save-load` |
| Quest data in `.tres` files | `godot-resource-pattern` |
| Quest text in several languages | `godot-localization` |
| A collection loop as an objective | `godot-gameplay-loops` |
| Tracker and journal screens | `godot-hud-system`, `godot-ui` |

## Prove it

Test the log headless in a `SceneTree` script: register two quests where B
needs A; B is `LOCKED` until A completes; a kill event advances only the
matching objective; the completion signal fires once even if more events
arrive; a save through `JSON` and a load into a new log restores the status
and the counts. `godot-build-verify` runs the script.
