# Fleet additions: GDScript on Godot 4.7

Own text. These facts are not in the upstream skill. Back to [SKILL.md](../SKILL.md).

Sources: the Godot 4.7.2 doctool dump (`@GDScript`) and the godot-docs 4.7 branch,
checked by the fleet's Godot reviewer. Issue numbers are from the Godot repository.

## `@export_file` stores a `uid://` path (since 4.4)

- Since 4.4, a path that you pick in the Inspector for an `@export_file` property is stored as
  a `uid://` path (GH-97912). Before 4.4 it was stored as a `res://` path.
- A `uid://` path survives a file move or rename. Resolve it with `ResourceUID.ensure_path()`
  or load it directly: `load()` accepts both forms.
- Do not compare the property with a `res://` string. A comparison like that fails
  after the editor stores a `uid://` path.

## `@export_file_path` keeps a `res://` path (since 4.5)

- Use `@export_file_path` when code or a tool needs a literal `res://` path.
- It takes the same filter arguments as `@export_file`.

```gdscript
extends Node

@export_file("*.tscn") var next_scene: String        # stored as uid://... since 4.4
@export_file_path("*.tscn") var next_scene_path: String  # stored as res://... since 4.5


func _ready() -> void:
	var scene: PackedScene = load(next_scene)  # load() takes uid:// and res://
	print(next_scene_path)
```
