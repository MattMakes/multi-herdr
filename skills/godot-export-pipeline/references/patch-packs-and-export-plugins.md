Adds runtime patch and DLC packs, `--export-patch`, `EditorExportPlugin` hooks and a headless all-presets export script; read it when a build needs patches, DLC, post-export steps or a scripted multi-preset export.

# Patch packs, DLC and export plugins

> ← Back to [SKILL.md](../SKILL.md)

## Mount a patch or DLC pack at runtime

`ProjectSettings.load_resource_pack(pack, replace_files = true, offset = 0)`
mounts a `.pck` or `.zip` over `res://`. With `replace_files` true, a file
in the pack replaces the file with the same path in the base game. It
returns `false` when the file is missing or is not a valid pack.

Mount packs **before** anything loads the resources they replace. A
resource that is already in the cache keeps the old version. The safe
place is the first autoload.

```gdscript
# pack_loader.gd - the first autoload in the list
extends Node

const PACK_DIR: String = "user://packs"

var mounted: PackedStringArray = []


func _init() -> void:
	# _init runs before any scene loads, so no cached resource is stale.
	if not DirAccess.dir_exists_absolute(PACK_DIR):
		return
	var files: PackedStringArray = DirAccess.get_files_at(PACK_DIR)
	files.sort()  # Name packs 001_patch.pck, 002_dlc.pck: the later one wins.
	for file_name: String in files:
		if not file_name.ends_with(".pck"):
			continue
		var path: String = PACK_DIR.path_join(file_name)
		if ProjectSettings.load_resource_pack(path, true):
			mounted.append(path)
		else:
			push_error("pack_loader: cannot mount %s" % path)
```

Rules:
- Keep each pack's content under its own path (`res://dlc/forest/...`)
  unless it is meant to replace base files.
- A pack exported from a different Godot version may not load. Build
  patches with the same engine version as the base game.
- Scripts in a pack run with full rights. Mount only packs the game
  downloaded from its own server over HTTPS, and check a hash before you
  mount.

## Build a patch from the command line

Since Godot 4.4 the editor can export a pack that holds only the files
changed since earlier packs:

```bash
# base.pck is the shipped release. patch_1.pck holds only the changes.
godot --headless --path . --export-patch "Windows Desktop" build/patch_1.pck --patches build/base.pck
# A second patch lists every earlier pack, comma-separated.
godot --headless --path . --export-patch "Windows Desktop" build/patch_2.pck --patches build/base.pck,build/patch_1.pck
```

The SKILL.md section on PCKs covers `PCKPacker` for packs built from code.

## Hook into the export with `EditorExportPlugin`

An `EditorExportPlugin` runs inside every export, in the editor and in a
headless CLI export. The hooks:

| Hook | Use |
|---|---|
| `_export_begin(features, is_debug, path, flags)` | Write a build manifest, check preconditions |
| `_export_file(path, type, features)` | Call `skip()` to drop a file from this export |
| `_export_end()` | Post-process the output next to the export path |
| `_get_export_features(platform, debug)` | Add feature tags for this export |

`add_file(path, bytes, remap)` puts generated data into the pack. The
preset is available from `get_export_preset()`; its output path is
`get_export_preset().get_export_path()`.

```gdscript
# addons/build_info/build_info_export.gd
@tool
extends EditorExportPlugin

var _is_debug: bool = false


func _get_name() -> String:
	return "BuildInfo"


func _export_begin(features: PackedStringArray, is_debug: bool, _path: String, _flags: int) -> void:
	_is_debug = is_debug
	var info: Dictionary = {
		"version": ProjectSettings.get_setting("application/config/version", "dev"),
		"preset": get_export_preset().get_preset_name(),
		"debug": is_debug,
		"features": Array(features),
		"built_unix": int(Time.get_unix_time_from_system()),
	}
	add_file("res://build_info.json", JSON.stringify(info, "\t").to_utf8_buffer(), false)


func _export_file(path: String, _type: String, _features: PackedStringArray) -> void:
	# Release builds never carry the debug tools folder.
	if not _is_debug and path.begins_with("res://debug_tools/"):
		skip()
```

```gdscript
# addons/build_info/plugin.gd - plugin.cfg points script= at this file
@tool
extends EditorPlugin

var _export_plugin: EditorExportPlugin


func _enter_tree() -> void:
	var script: GDScript = load("res://addons/build_info/build_info_export.gd")
	_export_plugin = script.new()
	add_export_plugin(_export_plugin)


func _exit_tree() -> void:
	remove_export_plugin(_export_plugin)
	_export_plugin = null
```

The plugin must be enabled in **Project Settings → Plugins**
(`editor_plugins/enabled` in `project.godot`), or a CLI export ignores it.

## Export every preset in one run

A headless script can read `export_presets.cfg` and start one export
process per preset. The editor binary does the export; the script only
loops.

```gdscript
# tools/export_all.gd
# Run: godot --headless --path . -s res://tools/export_all.gd -- release
extends SceneTree


func _init() -> void:
	var mode: String = "--export-debug" if "debug" in OS.get_cmdline_user_args() else "--export-release"
	var presets := ConfigFile.new()
	if presets.load("res://export_presets.cfg") != OK:
		push_error("export_all: no export_presets.cfg")
		quit(2)
		return
	var failures: int = 0
	for section: String in presets.get_sections():
		if section.count(".") != 1:
			continue  # Skip the [preset.N.options] sections.
		var preset_name: String = presets.get_value(section, "name", "")
		var out_path: String = presets.get_value(section, "export_path", "")
		if preset_name.is_empty() or out_path.is_empty():
			push_warning("export_all: %s has no name or export_path" % section)
			continue
		DirAccess.make_dir_recursive_absolute(ProjectSettings.globalize_path("res://").path_join(out_path.get_base_dir()))
		var args: PackedStringArray = ["--headless", "--path", ProjectSettings.globalize_path("res://"), mode, preset_name, out_path]
		var output: Array = []
		var code: int = OS.execute(OS.get_executable_path(), args, output, true)
		print("%s %s -> %s (exit %d)" % [mode, preset_name, out_path, code])
		if code != 0:
			failures += 1
			print("".join(PackedStringArray(output)))
	quit(1 if failures > 0 else 0)
```

Check the export exit code **and** that the output file exists. A missing
export template makes the export fail with a message, and the next CI step
must not package an old file.

## Signing, notarization and upload are the orchestrator's call

A fleet worker builds and checks export artifacts. It does not upload to a
store, push with `butler`, run `steamcmd +run_app_build`, or submit to
Apple notarization. Those steps publish or use the operator's
credentials. Put them in the `DONE:` report as a plan, for example:

```bash
# macOS direct distribution, after export (operator runs these)
codesign --deep --force --options runtime --sign "Developer ID Application: <Team>" MyGame.app
ditto -c -k --keepParent MyGame.app MyGame.zip
xcrun notarytool submit MyGame.zip --keychain-profile "<profile>" --wait
xcrun stapler staple MyGame.app
```

Credentials (keystore passwords, Apple app-specific passwords, Steam
logins) come from the environment or a keychain profile, never from a file
in the repository.
