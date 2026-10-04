# Project templates

How to lay out a new Godot 4.7 project before the genre work starts. Use it
with `godot-project-setup` (project settings, input map, autoloads) and
`godot-scene-organization` (scene boundaries). Write the outcome into
`.agents/godot-project-context.md` through `godot-project-context`.

## Folder by feature

Keep everything for one thing in one folder: its scene, scripts, art and
local Resources. A move or delete then touches one folder.

```text
res://
├── autoloads/        # thin globals: clock, save, events, settings
├── entities/
│   ├── player/       # player.tscn, player.gd, sprites, player_stats.tres
│   └── slime/
├── levels/           # or maps/, rooms/, chunks/ for the genre
├── systems/          # combat/, inventory/, quests/ when not per entity
├── ui/               # menus, HUD, themes
├── resources/        # shared data: items/, waves/, cards/
├── audio/
└── addons/           # pinned third-party addons only
```

Genre starting points:

| genre group | extra folders | first scenes |
|---|---|---|
| 2D platformer, metroidvania | `levels/` or `rooms/`, `tiles/` | player, one room, HUD, pause menu |
| Top-down or action RPG | `maps/`, `systems/combat`, `systems/inventory`, `resources/items` | player, one map, inventory UI |
| 3D FPS or TPS | `weapons/`, `enemies/`, `levels/` | player with camera, test range, HUD |
| Strategy and simulation | `units/` or `buildings/`, `resources/data` | camera rig, grid, build menu |
| Narrative (VN, romance) | `story/` (script files), `characters/` | stage, text box, save menu |

## Autoloads

- Keep autoloads few and thin: state that must outlive scene changes, and
  services every scene uses.
- Autoloads are added to the tree in the order listed in Project Settings.
  When one needs another in `_ready()`, order the list on purpose (settings
  and save first, audio and UI after) and write the order down.
- Prefer one events autoload (`godot-event-bus`) over many autoloads that
  call each other.

## Pause

`get_tree().paused = true` stops every node whose process mode resolves to
pausable. The pause menu must run while paused.

- Gameplay root: `PROCESS_MODE_PAUSABLE` (or the default, Inherit).
- Pause menu and its overlay: `PROCESS_MODE_ALWAYS`.
- Things that run only while paused (a photo mode): `PROCESS_MODE_WHEN_PAUSED`.

## Platform forks and DLC

Branch on export feature tags, not on hand-made flags: `OS.has_feature("mobile")`,
`OS.has_feature("web")`, `OS.has_feature("dedicated_server")`, or a custom
tag set in the export preset. Mount extra content packs with
`ProjectSettings.load_resource_pack()` before you load their scenes.

```gdscript
extends Node

const DLC_DIR := "user://dlc"

func mount_all() -> PackedStringArray:
	var mounted := PackedStringArray()
	var dir := DirAccess.open(DLC_DIR)
	if dir == null:
		return mounted
	for file in dir.get_files():
		if file.get_extension() == "pck":
			var path := DLC_DIR.path_join(file)
			if ProjectSettings.load_resource_pack(path, false):
				mounted.append(path)
			else:
				push_warning("Could not mount %s" % path)
	return mounted
```

Pass `false` as the second argument so a pack cannot replace files the base
game already has. Load a pack only from a source you trust; a pack can
contain scripts.

## Pitfalls

- Scene paths typed in many scripts. Keep them in one registry (constants
  or exported `PackedScene` fields); a moved file then breaks one place.
- Designer drops of raw source files (PSD, Blender, WAV masters) in a
  folder the importer reads. Put an empty `.gdignore` file in such folders.
- Copying a template project as-is. Rename the project, set the input map
  and the window settings for this game, and remove unused systems.
- One large "GameManager" autoload that owns everything. Split by service.
- A pause menu that freezes with the game. Set its process mode to Always.
- An FPS template that never captures the mouse. Set
  `Input.mouse_mode = Input.MOUSE_MODE_CAPTURED` when play starts and
  release it in menus.
- Fixed pixel UI. Use anchors and containers (`godot-responsive-ui`).
- Strings with no context. Use `tr()` with a context where one English word
  has two meanings.
- Big levels loaded with `load()`. Use `ResourceLoader.load_threaded_request()`
  behind a loading screen.
- A fresh clone with no `.godot/`. Import headless before checks
  (`godot-build-verify`), and commit the `.uid` files the import writes.
