# Composition for UI screens, tools and editor docks

Adds the component rules for `Control` screens, tool windows and editor
plugin docks: a root that only wires, logic that never touches visuals,
focus owned by the root, and saveable state through a group. Read it when
you build a settings screen, a form, a dashboard or an `EditorPlugin` dock.
For gameplay entities, use `orchestrator-components.md`.

## The same rule as for entities

The screen's root script wires. Components do the work. A form component
validates input, a request component talks to a server, a view component
plays animations. They talk up with signals. The root calls down.

| Part | Does | Never |
| --- | --- | --- |
| Root (`settings_screen.gd`) | Connects signals, calls methods, moves focus | Validates, formats, saves |
| Logic component | Checks data, emits a result signal | Touches a `Control`, a `Tween` or a `Theme` |
| View component | Plays feedback (shake, colour, sound) | Decides if data is valid |

## Logic emits, the view reacts

```gdscript
class_name NameValidator
extends Node

signal accepted(player_name: String)
signal rejected(field: StringName, reason: String)

@export var min_length: int = 3
@export var max_length: int = 16


func check(player_name: String) -> void:
	var trimmed := player_name.strip_edges()
	if trimmed.length() < min_length:
		rejected.emit(&"NameEdit", "Too short")
	elif trimmed.length() > max_length:
		rejected.emit(&"NameEdit", "Too long")
	else:
		accepted.emit(trimmed)
```

```gdscript
class_name ShakeFeedback
extends Node

@export var target: Control
@export var strength: float = 8.0


func play() -> void:
	if target == null:
		return
	var start := target.position
	var tween := target.create_tween()
	for i in 4:
		var offset := strength if i % 2 == 0 else -strength
		tween.tween_property(target, "position:x", start.x + offset, 0.04)
	tween.tween_property(target, "position:x", start.x, 0.04)
```

The root connects them and owns focus:

```gdscript
extends Control

signal name_chosen(player_name: String)

@export var validator: NameValidator
@export var feedback: ShakeFeedback
@export var name_edit: LineEdit
@export var error_label: Label


func _ready() -> void:
	validator.accepted.connect(_on_accepted)
	validator.rejected.connect(_on_rejected)
	name_edit.text_submitted.connect(validator.check)


func _on_accepted(player_name: String) -> void:
	error_label.text = ""
	name_chosen.emit(player_name)


func _on_rejected(_field: StringName, reason: String) -> void:
	error_label.text = reason
	feedback.play()
	name_edit.grab_focus()
	name_edit.select_all()
```

`NameValidator` has no `Control` in it, so a headless test can call
`check()` and read its signals. The view can change (a sound instead of a
shake) without a change to the rules.

## Focus belongs to the root

Only the root calls `grab_focus()` or `release_focus()`. A component that
moves focus fights with other components and with keyboard and gamepad
navigation. The root knows the screen's layout. A component does not.

## Saveable state through a group

A component that has state worth saving joins a group and offers two
methods. The save system collects from the group. No component does file
I/O.

```gdscript
class_name SaveableSettings
extends Node

const GROUP: StringName = &"saveable"

@export var key: StringName = &"settings"

var values: Dictionary = {}


func _enter_tree() -> void:
	add_to_group(GROUP)


func get_save_data() -> Dictionary:
	return {"key": key, "values": values.duplicate(true)}


func load_save_data(data: Dictionary) -> void:
	values = (data.get("values", {}) as Dictionary).duplicate(true)
```

```gdscript
class_name SaveCollector
extends RefCounted


static func collect(tree: SceneTree) -> Dictionary:
	var out: Dictionary = {}
	for node in tree.get_nodes_in_group(SaveableSettings.GROUP):
		if node.has_method(&"get_save_data"):
			var data: Dictionary = node.call(&"get_save_data")
			out[data["key"]] = data
	return out
```

The `godot-save-load` skill covers the file format and the write.

## Editor plugin docks

A dock is a `Control` like any other screen, so the same split applies.
Two extra rules:

- Keep the logic components free of `EditorInterface` calls. Pass the data
  they need in from the root (the plugin script or the dock root). Then the
  logic also runs in a headless test, where no editor exists.
- Do not cache editor nodes across a plugin reload. Get them again in
  `_enter_tree()` of the plugin.
