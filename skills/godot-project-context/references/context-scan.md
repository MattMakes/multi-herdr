# The context scan

A headless `SceneTree` script that prints the facts for
`.agents/godot-project-context.md`, one `key: value` line each. It reads
`project.godot`, `addons/*/plugin.cfg` and `export_presets.cfg` with
`ConfigFile`. It needs no import, saves nothing, and does not load scripts or
scenes. Run on Godot 4.7.2 against a blank project, a GUT project, a gdUnit4
project, a C# project, and a project with an autoload, an input action, an
addon, Jolt, a stretch mode and an export preset.

## Run

`gd` runs Godot through `godot-build-verify`'s wrapper; see its `references/commands.md`, Common setup.

Write the script to `.godot/horch-home/context_scan.gd` (git-ignored, not
scanned), then:

```bash
mkdir -p .godot/horch-home
gd -s "$PWD/.godot/horch-home/context_scan.gd"
```

The wrapper keeps Godot's user files out of the operator's directory.
Exit 1 with `SCAN_ERROR:` when `project.godot` cannot be read.

Example output (the scratch project with an autoload, an addon and Jolt):

```text
name: CtxProj
features: 4.7, Forward Plus
main_scene: res://main.tscn
rendering_method: mobile
physics_3d: Jolt Physics
physics_2d: DEFAULT (GodotPhysics2D)
stretch_mode: canvas_items
stretch_aspect: keep (default)
csharp: no
csproj:
dotnet_assembly:
autoload: Events = *res://autoload/events.gd
input_action: jump
addon: fancy_tool | Fancy Tool | 1.4.2 | enabled
export_preset: macOS (macOS)
```

A gdUnit4 project adds `addon: gdUnit4 | gdUnit4 | 6.2.1 | enabled` and
`test_framework: gdUnit4`. A C# project prints `csharp: yes`,
`csproj: CsProj.csproj` and `dotnet_assembly: CsProj`.

## Source

```gdscript
# Prints the facts for .agents/godot-project-context.md, one "KEY: value"
# line each. Reads files only; it saves nothing and needs no import.
extends SceneTree


func _init() -> void:
	var cfg := ConfigFile.new()
	if cfg.load("res://project.godot") != OK:
		print("SCAN_ERROR: cannot read res://project.godot")
		quit(1)
		return
	var features: PackedStringArray = cfg.get_value("application", "config/features", PackedStringArray())
	_out("name", cfg.get_value("application", "config/name", "[unknown]"))
	_out("features", ", ".join(features))
	_out("main_scene", cfg.get_value("application", "run/main_scene", "[unknown]"))
	_out("rendering_method", cfg.get_value("rendering", "renderer/rendering_method", "forward_plus (default)"))
	_out("physics_3d", cfg.get_value("physics", "3d/physics_engine", "DEFAULT (GodotPhysics3D)"))
	_out("physics_2d", cfg.get_value("physics", "2d/physics_engine", "DEFAULT (GodotPhysics2D)"))
	_out("stretch_mode", cfg.get_value("display", "window/stretch/mode", "disabled (default)"))
	_out("stretch_aspect", cfg.get_value("display", "window/stretch/aspect", "keep (default)"))
	var csharp := features.has("C#") or not _root_files("csproj").is_empty()
	_out("csharp", "yes" if csharp else "no")
	_out("csproj", ", ".join(_root_files("csproj")))
	_out("dotnet_assembly", cfg.get_value("dotnet", "project/assembly_name", ""))
	for key in _keys(cfg, "autoload"):
		_out("autoload", "%s = %s" % [key, cfg.get_value("autoload", key)])
	for key in _keys(cfg, "input"):
		_out("input_action", key)
	var enabled: PackedStringArray = cfg.get_value("editor_plugins", "enabled", PackedStringArray())
	var addons := DirAccess.get_directories_at("res://addons") if DirAccess.dir_exists_absolute("res://addons") else PackedStringArray()
	for dir_name in addons:
		var plugin_path := "res://addons/%s/plugin.cfg" % dir_name
		var plugin := ConfigFile.new()
		if plugin.load(plugin_path) != OK:
			_out("addon", "%s (no plugin.cfg)" % dir_name)
			continue
		_out("addon", "%s | %s | %s | %s" % [dir_name, plugin.get_value("plugin", "name", "?"),
				plugin.get_value("plugin", "version", "[unknown]"),
				"enabled" if enabled.has(plugin_path) else "not enabled"])
	if DirAccess.dir_exists_absolute("res://addons/gut"):
		_out("test_framework", "GUT")
	if DirAccess.dir_exists_absolute("res://addons/gdUnit4"):
		_out("test_framework", "gdUnit4")
	var presets := ConfigFile.new()
	if presets.load("res://export_presets.cfg") == OK:
		for section in presets.get_sections():
			if section.count(".") == 1:
				_out("export_preset", "%s (%s)" % [presets.get_value(section, "name", "?"),
						presets.get_value(section, "platform", "?")])
	quit(0)


func _out(key: String, value: Variant) -> void:
	print("%s: %s" % [key, value])


func _keys(cfg: ConfigFile, section: String) -> PackedStringArray:
	return cfg.get_section_keys(section) if cfg.has_section(section) else PackedStringArray()


func _root_files(extension: String) -> PackedStringArray:
	var found := PackedStringArray()
	for file_name in DirAccess.get_files_at("res://"):
		if file_name.get_extension() == extension:
			found.append(file_name)
	return found
```
