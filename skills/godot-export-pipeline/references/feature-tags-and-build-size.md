Adds runtime feature tags, debug stripping, export filters and a build-size audit; read it when one codebase must ship several build variants or when an export is larger than expected.

# Feature tags, build variants and build size

> ← Back to [SKILL.md](../SKILL.md)

## Feature tags decide the variant at runtime

`OS.has_feature(tag)` answers from the running binary, not from the editor
settings. Use it, not your own `is_debug` constants, to fork behaviour.

| Tag | True when |
|---|---|
| `editor` | Running inside the editor (also a game run started from the editor) |
| `template` | Running an exported template (any export) |
| `debug` / `release` | Debug template or editor / release template |
| `pc`, `mobile`, `web` | Platform family |
| `windows`, `linuxbsd`, `macos`, `android`, `ios` | Platform |
| `dedicated_server` | A preset exported with **Export As Dedicated Server** |
| your own tag | Listed in the preset's **Custom Features** field |

Custom features are the clean way to build store variants from one
project: one preset with `steam`, another with `itch`, a third with `demo`.

```gdscript
# build_flags.gd - autoload named BuildFlags
extends Node

## Central place for every build-variant decision.
var is_demo: bool = false
var store: StringName = &"none"


func _ready() -> void:
	is_demo = OS.has_feature("demo")
	if OS.has_feature("steam"):
		store = &"steam"
	elif OS.has_feature("itch"):
		store = &"itch"
	if OS.has_feature("release"):
		_strip_debug_tools()


func _strip_debug_tools() -> void:
	# Debug overlays and cheat consoles live under one group so a release
	# build removes them in one pass.
	for node: Node in get_tree().get_nodes_in_group(&"debug_only"):
		node.queue_free()


func api_base_url() -> String:
	return "https://staging.example.com" if OS.has_feature("debug") else "https://api.example.com"
```

Rules:
- A cheat or debug console must be gated on `release` being false, or
  excluded from the release preset. A hidden key combination is not a gate.
- Project settings can also take a feature override: a setting written as
  `key.feature` (for example `display/window/size/viewport_width.mobile`)
  wins when that feature is present. Prefer this over code for settings.

## `res://` is read-only in an export

An exported game reads `res://` from the `.pck`. Writes fail. Saves,
settings, logs, caches and downloaded patches go to `user://`. Code that
works in the editor and writes to `res://` breaks only in the export, so
review for it before the first export.

## Keep authoring files out of the pack

The preset's **Resources** tab has an include filter and an exclude filter
(comma-separated globs, stored as `include_filter` and `exclude_filter` in
`export_presets.cfg`). Exclude what the game never loads:

```ini
exclude_filter="*.md,*.txt,docs/*,art_source/*,*.psd,*.kra,*.blend1"
```

A directory with an empty `.gdignore` file is not imported and is not
exported. Use it for source art, design documents and tool output that
lives inside the project folder.

For a dedicated server preset, also exclude client-only content (music,
high-resolution textures). Do not enable the Shader Baker on a server
preset: a server renders nothing, so the bake only adds build time.

## Texture compression per target

| Target | Project setting to enable |
|---|---|
| Desktop (Forward+ / Mobile renderer) | `rendering/textures/vram_compression/import_s3tc_bptc` |
| Android, iOS, standalone XR, most web | `rendering/textures/vram_compression/import_etc2_astc` |
| Pixel art | Set the texture's import **Compress Mode** to Lossless |

The export fails with a clear message when the preset's texture format is
not imported. Enabling a format reimports every texture, so do it once,
early, and commit the result.

## Find what makes a build large

Run this headless script against the project. It lists the largest files
under `res://` that are not excluded by a `.gdignore`.

```gdscript
# tools/size_report.gd
# Run: godot --headless --path . -s res://tools/size_report.gd
extends SceneTree

const TOP_N: int = 25


func _init() -> void:
	var sizes: Array[Dictionary] = []
	_collect("res://", sizes)
	sizes.sort_custom(func(a: Dictionary, b: Dictionary) -> bool: return a.bytes > b.bytes)
	var total: int = 0
	for entry: Dictionary in sizes:
		total += entry.bytes
	print("files=%d total=%.1f MiB" % [sizes.size(), total / 1048576.0])
	for i: int in mini(TOP_N, sizes.size()):
		print("%8.2f MiB  %s" % [sizes[i].bytes / 1048576.0, sizes[i].path])
	quit(0)


func _collect(dir_path: String, out: Array[Dictionary]) -> void:
	if FileAccess.file_exists(dir_path.path_join(".gdignore")):
		return
	for file_name: String in DirAccess.get_files_at(dir_path):
		var path: String = dir_path.path_join(file_name)
		var file := FileAccess.open(path, FileAccess.READ)
		if file != null:
			out.append({"path": path, "bytes": file.get_length()})
	for sub: String in DirAccess.get_directories_at(dir_path):
		if not sub.begins_with("."):
			_collect(dir_path.path_join(sub), out)
```

The report measures source files. The exported pack holds the imported
versions from `.godot/imported/`, which are often larger (uncompressed
audio) or smaller (VRAM-compressed textures). Use the report to find
candidates, then compare the `.pck` size before and after a change.
