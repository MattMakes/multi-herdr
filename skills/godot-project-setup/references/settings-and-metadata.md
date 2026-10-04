# Typed-code warnings, build metadata and settings overrides

Adds the GDScript warning settings that enforce typed code, a build-metadata
helper and the `override.cfg` mechanism. Read it when you set up a new
project, add a version string to the game, or save player-chosen engine
settings. Checked on Godot 4.7.2.

## Make untyped code a warning

These keys are in `project.godot` under `[debug]`. The value is `0` (ignore),
`1` (warn) or `2` (error). All of them default to `0`.

| Key | What it reports |
| --- | --- |
| `gdscript/warnings/untyped_declaration` | A variable, parameter or return with no type |
| `gdscript/warnings/inferred_declaration` | A `:=` declaration (set it only if the team bans `:=`) |
| `gdscript/warnings/unsafe_property_access` | Property access on a value of unknown type |
| `gdscript/warnings/unsafe_method_access` | Method call on a value of unknown type |
| `gdscript/warnings/unsafe_cast` | An `as` cast from a value of unknown type |
| `gdscript/warnings/unsafe_call_argument` | An argument of unknown type passed to a typed parameter |

A new project starts well with `untyped_declaration=1`, or `2` when the
headless parse check must enforce it. Do not set `2` on an existing project
without a plan: every untyped line then stops the script from loading.

```ini
[debug]

gdscript/warnings/untyped_declaration=1
gdscript/warnings/unsafe_method_access=1
```

The key `debug/gdscript/warnings/directory_rules` sets warning rules per
folder. Its default, `{ "res://addons": 0 }`, gives third-party code under
`res://addons` its own rule. Keep that entry, so addon code does not flood
the project with warnings you do not own.

Measured on 4.7.2 in headless runs: a warning at level `1` prints nothing
(neither `load()` nor `--check-only` shows it). A warning at level `2` is a
parse error: `load()` prints `Parse Error: ... (Warning treated as error.)`
and the script cannot be instantiated. So the project-wide parse check
enforces only level `2` warnings. Three are level `2` by default:
`onready_with_export`, `native_method_override` and `inference_on_variant`
(plus `get_node_default_without_onready`).

## Build metadata

Put the version in `application/config/version` (Project Settings >
Application > Config > Version). Read it at run time. Do not keep a second
copy in a JSON file.

```gdscript
class_name BuildInfo
extends RefCounted


static func version() -> String:
	return str(ProjectSettings.get_setting("application/config/version", "0.0.0"))


static func build_kind() -> String:
	if OS.has_feature("editor"):
		return "editor"
	if OS.is_debug_build():
		return "debug"
	return "release"


static func label() -> String:
	var engine: Dictionary = Engine.get_version_info()
	return "v%s (%s, %s, Godot %s)" % [version(), build_kind(), OS.get_name(), engine["string"]]
```

`OS.has_feature()` also reads the custom feature tags of an export preset.
Use a custom tag (for example `demo`) to switch content per export, instead
of a constant that someone must edit before each build.

## Player settings with `override.cfg`

`ProjectSettings.save_custom(path)` writes the current project settings to
a file. Godot reads an `override.cfg` at startup and applies it over
`project.godot`. A shipped game cannot write next to its executable on
every platform. So set `application/config/project_settings_override` to a
`user://` path, and save there.

```ini
[application]

config/project_settings_override="user://override.cfg"
```

```gdscript
class_name DisplayPrefs
extends RefCounted


static func apply_and_save(max_fps: int, vsync: bool) -> Error:
	Engine.max_fps = max_fps
	DisplayServer.window_set_vsync_mode(
		DisplayServer.VSYNC_ENABLED if vsync else DisplayServer.VSYNC_DISABLED)
	ProjectSettings.set_setting("application/run/max_fps", max_fps)
	ProjectSettings.set_setting("display/window/vsync/vsync_mode", 1 if vsync else 0)
	return ProjectSettings.save_custom("user://override.cfg")
```

Measured on 4.7.2: with the override key set, a value saved this way is
active on the next start (`Engine.max_fps` read back the saved value).
`set_setting` alone changes nothing at run time. Set the engine property too
(`Engine.max_fps`, the `DisplayServer` call), as the example does.

`save_custom` writes the full settings file, not only the changed keys. For
settings that are game options (volume, key bindings, language), a
`ConfigFile` of your own is simpler and safer. Keep `override.cfg` for engine
settings that must apply before the first scene loads, such as the renderer
or the window mode.
