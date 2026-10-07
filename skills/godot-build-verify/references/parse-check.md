# The project-wide parse check

`--check-only` prints `Parse Error` and still exits 0 (measured on 4.7.2), so a
worker cannot gate on it. This `SceneTree` script loads every `.gd`, `.tscn`
and `.tres` under `res://` and exits 1 when one does not load.

How it decides, measured on 4.7.2:

- A `.gd` with a parse error still loads as a non-null `GDScript`, but
  `can_instantiate()` returns `false`. A valid script, an `@abstract` script
  included, returns `true`.
- A `.tscn` or `.tres` that does not parse loads as `null`.
- A scene with a wrong `parent=` or a missing `ext_resource` file still loads.
  Only the grep catches those (`godot-scene-files`, "Prove the change").
- A call to a member that an engine class does not have, on a typed
  variable (`var b: Node2D` then `b.no_such_method()`), still loads, and
  `can_instantiate()` is `true`. Only an unknown name on `self` is a parse
  error. The tests and the smoke run catch the rest.

- The work runs in `_initialize()`. In `_init()`, the autoloads do not exist
  yet: `root.has_node("GameState")` is `false`, and a script that names the
  autoload prints `SCRIPT ERROR: Compile Error: Identifier not found:
  GameState` and gives `can_instantiate()` `false`. Measured on a project
  with 4 scripts (2 name an autoload) and 1 scene: in `_init()` the check printed
  `PARSE_CHECK checked=5 failed=2` (2 false failures); in `_initialize()`,
  `failed=0`. A real parse error and an unknown identifier still fail in
  `_initialize()`.

It skips `.godot/`, every folder whose name starts with `.`, every folder that
holds a `.gdignore`, and the folder names given with `--skip=`.

## Run

Write the script below to `.godot/horch-home/parse_check.gd` (outside the
scanned tree, git-ignored), import first, then:

```bash
HOME="$PWD/.godot/horch-home" "$GODOT" --headless --path "$PWD" -s "$PWD/.godot/horch-home/parse_check.gd"
HOME="$PWD/.godot/horch-home" "$GODOT" --headless --path "$PWD" -s "$PWD/.godot/horch-home/parse_check.gd" -- --skip=addons
```

Output: one `PARSE_CHECK FAIL <path>` line per bad file, then
`PARSE_CHECK checked=<n> failed=<m>`. Exit 0 when `m` is 0, else exit 1. The
engine's own `SCRIPT ERROR` lines above it give the file, line and message.

Run it after `--import`. Without the import, a script that names another
file's `class_name` fails, because the global class cache is in `.godot/`.

## Source

```gdscript
# Project-wide parse check for Godot 4.7.
extends SceneTree

const EXTENSIONS: PackedStringArray = ["gd", "tscn", "tres"]

var _skip: PackedStringArray = []
var _checked := 0
var _failed: PackedStringArray = []


# Not _init(): the autoloads exist only from _initialize() on.
func _initialize() -> void:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--skip="):
			_skip = arg.trim_prefix("--skip=").split(",", false)
	_walk("res://")
	for path in _failed:
		print("PARSE_CHECK FAIL ", path)
	print("PARSE_CHECK checked=%d failed=%d" % [_checked, _failed.size()])
	quit(1 if not _failed.is_empty() else 0)


func _walk(dir_path: String) -> void:
	if FileAccess.file_exists(dir_path.path_join(".gdignore")):
		return
	for dir_name in DirAccess.get_directories_at(dir_path):
		if dir_name.begins_with(".") or dir_name in _skip:
			continue
		_walk(dir_path.path_join(dir_name))
	for file_name in DirAccess.get_files_at(dir_path):
		if file_name.get_extension() in EXTENSIONS:
			_check(dir_path.path_join(file_name))


func _check(path: String) -> void:
	_checked += 1
	var res := ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_IGNORE)
	if res == null:
		_failed.append(path)
	elif res is Script and not (res as Script).can_instantiate():
		_failed.append(path)
```

Side effects: loading a script runs its static variable initializers and its
`_static_init()` (measured), and nothing else. A script that writes files from
static code does so during the check. The check does not instantiate scenes;
`godot-scene-files` does that for the scenes you changed.
