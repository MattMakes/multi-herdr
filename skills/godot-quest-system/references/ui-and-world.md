# Quest UI and the world

## Tracker HUD

The tracker shows active quests and their visible objectives. It owns no
state: it rebuilds from the quest log and updates from its signals.

```gdscript
extends VBoxContainer

@export var quests: QuestLog

var _lines: Dictionary[StringName, Label] = {}  # "quest/objective" -> label


func _ready() -> void:
	quests.quest_accepted.connect(func(_id: StringName) -> void: rebuild())
	quests.quest_completed.connect(func(_id: StringName, _def: QuestDef) -> void: rebuild())
	quests.quest_failed.connect(func(_id: StringName) -> void: rebuild())
	quests.objective_progressed.connect(_on_progress)
	rebuild()


func rebuild() -> void:
	for child: Node in get_children():
		child.queue_free()
	_lines.clear()
	for quest_id: StringName in quests.quest_ids():
		if quests.status(quest_id) != QuestLog.Status.ACTIVE:
			continue
		var def: QuestDef = quests.definition(quest_id)
		var title := Label.new()
		title.text = tr(def.title_key)
		add_child(title)
		for obj: ObjectiveDef in def.objectives:
			var n: int = quests.count(quest_id, obj.id)
			if obj.hidden and n == 0:
				continue
			var line := Label.new()
			line.text = _objective_text(obj, n)
			add_child(line)
			_lines[StringName("%s/%s" % [quest_id, obj.id])] = line


func _on_progress(quest_id: StringName, objective_id: StringName, count: int, _required: int) -> void:
	var line: Label = _lines.get(StringName("%s/%s" % [quest_id, objective_id]))
	if line == null:
		rebuild()  # a hidden objective became visible
		return
	for obj: ObjectiveDef in quests.definition(quest_id).objectives:
		if obj.id == objective_id:
			line.text = _objective_text(obj, count)


func _objective_text(obj: ObjectiveDef, count: int) -> String:
	return "%s %d/%d" % [tr(obj.text_key), count, obj.required]
```

Pin one tracked quest when the screen is small. Layout and styles
are in `godot-hud-system` and `godot-ui`.

## Localized text

Quest text is a translation key, not the text: `title_key =
"QUEST_SLIMES_TITLE"`. The UI calls `tr(key)`. Rules:

- Put the counts in the format string, not in the translation:
  `tr("QUEST_SLIMES_OBJ_1")` gives "Slimes defeated", and the UI adds
  `3/10`. Languages differ in word order; if the number must sit inside the
  sentence, use a key with a placeholder (`"Defeat {count} slimes"`) and
  `tr(key).format({"count": n})`.
- A plural needs `tr_n(singular_key, plural_key, n)`.
- `godot-localization` covers CSV and PO files.

## Waypoints

A waypoint marks where the current objective is. Give `ObjectiveDef` a
`target_marker: StringName`, and give each world location a `Marker3D` in a
group (for example `&"quest_targets"`) with a matching `marker_id`
metadata or export. On `objective_progressed` or a tracked-quest change:

1. Find the first unfinished, visible objective of the tracked quest.
2. Find the marker node with its `target_marker`.
3. Move the HUD arrow or the minimap icon to it. For a path, set the
   `NavigationAgent3D.target_position` once (`godot-ai-navigation`).

Update the target when the objective changes, not every frame. A marker in
an unloaded level has no node: point to the door or the map exit toward that
level instead.

## Quest givers

An NPC that offers a quest asks the log about its state and picks a line:

| `status(quest_id)` | The NPC says |
| --- | --- |
| `LOCKED` | small talk, or a hint toward the prerequisite |
| `AVAILABLE` | the offer; on "yes", `accept(quest_id)` |
| `ACTIVE` | a reminder; if the turn-in objective is "talk to me", `notify(&"talk", npc_id)` |
| `COMPLETED` | thanks |
| `FAILED` | the failure line |

Show an icon over the NPC from the same table (`!` for available, `?` for
ready to turn in). Dialogue trees and their conditions are in
`godot-dialogue-system` (or `godot-dialogue-manager` when that addon is in
`addons/`): their conditions call `status()`, and their actions call
`accept()` or `notify()`.

## Journal

The journal lists completed and failed quests too. It reads the log the same
way as the tracker. Long descriptions use a `RichTextLabel` with BBCode
for emphasis. Keep the description key in `QuestDef` (`description_key`),
not in the UI.

## Checks

- Accept a quest: the tracker shows it with `0/N`.
- An event: the line updates to `1/N` without a full rebuild.
- A hidden objective: not shown at 0, shown after its first event.
- Complete the quest: it leaves the tracker.
