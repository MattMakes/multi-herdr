# Naming, feature folders and `.gdignore`

Adds the naming rules, the feature-folder variant and `.gdignore` for raw
art sources. Read it when you create files, folders or nodes in a project, or
when you review a change that adds them.

## Naming rules

Godot's style guide sets these. Apply them to every new file, node and
member. Do not rename existing files to match unless the task asks: a rename
breaks every `res://` path that points at the file.

| Thing | Rule | Example |
| --- | --- | --- |
| File and folder | `snake_case` | `player_controller.gd`, `main_menu/` |
| C# script file | `PascalCase`, same as the class | `PlayerController.cs` |
| `class_name` | `PascalCase` | `class_name HealthComponent` |
| Node in a scene | `PascalCase` | `HealthBar`, `Hitbox` |
| Function, variable | `snake_case` | `apply_damage()`, `max_health` |
| Private member | leading `_` | `_current_health`, `_on_timeout()` |
| Constant, enum value | `CONSTANT_CASE` | `MAX_SPEED`, `State.IDLE` |
| Signal | past tense, `snake_case` | `health_changed`, `died` |
| Signal handler | `_on_<node>_<signal>` | `_on_hitbox_area_entered` |

A signal names something that happened (`door_opened`), not an order
(`open_door`). An order is a method call.

## Stable node references

A path such as `$"../../UI/HealthBar"` breaks when a node moves. Mark the
node **Access as Unique Name** in the scene dock (the `.tscn` stores
`unique_name_in_owner = true`) and read it with `%`. The `%` lookup works
from any node in the same scene, at any depth.

```gdscript
extends Control

@onready var _health_bar: ProgressBar = %HealthBar
@onready var _score_label: Label = %ScoreLabel


func show_score(value: int) -> void:
	_score_label.text = str(value)
```

A unique name is local to its scene. It does not reach into an instanced
child scene, and two nodes in one scene cannot share it.

## Feature folders

The SKILL.md shows a split layout and a co-located layout. A third common
form groups by feature at the top level and keeps shared code apart:

```
res://
├── common/          # code and assets used by 2+ features
├── entities/
│   ├── player/      # player.tscn, player.gd, player_states/, sprites
│   └── enemy/
├── ui/
│   └── main_menu/
├── levels/
├── data/            # .tres definitions (items, stats)
└── addons/
```

Pick one layout at project start and record it in the project README. Do not
mix layouts in one project. When a project already has a layout, follow it.

## `.gdignore` for raw sources

Godot imports every file it understands under `res://`. A `.psd`, `.blend` or
`.kra` source next to the exported art doubles the import time and can add
broken imports. Put raw sources in a folder that holds an empty file named
`.gdignore`. Godot does not import or show anything in that folder.

```
res://
└── art_source/
    ├── .gdignore
    ├── player.kra
    └── level_01.blend
```

Keep exported art (`.png`, `.glb`) outside that folder. A `.blend` that the
project imports on purpose (Blender import enabled) must not be under a
`.gdignore`.
