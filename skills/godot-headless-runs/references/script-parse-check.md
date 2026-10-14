# Script: parse_check.gd

Usage: `godot --headless --path <project> -s <absolute path>/parse_check.gd -- --skip=captures,exports`

The project parse check loads every `.gd`, `.tscn` and `.tres` file under `res://`. It prints 1 `PARSE_CHECK FAIL <path>` line for each file that fails to load and for each script that cannot instantiate. The last line is `PARSE_CHECK checked=<n> failed=<n>`, and the exit code is 1 on a failure. It skips folders that start with `.`, folders that hold a `.gdignore` file, and the folder names in `--skip`. Run the import first. Use it instead of `--check-only`, which has no autoloads and can exit 0 on a parse error.

Copy the block into a file named `parse_check.gd` before you run it.

```gdscript
extends SceneTree
## Run: godot --headless --path <project> -s <absolute path>/parse_check.gd (GDScript, not Python).
## Needs: a Godot 4 binary and an imported project. It loads every .gd, .tscn and .tres file under
## res:// and lists each file that fails to load, and each script that cannot instantiate.
##   godot --headless --path <project> -s <absolute path>/parse_check.gd -- --skip=captures,exports
## --skip lists folder names to leave out. Folders that start with "." and folders that hold a
## .gdignore file are skipped. Run the import first, or new class names read as unknown.
## Output: 1 "PARSE_CHECK FAIL <path>" line per failure, then "PARSE_CHECK checked=<n> failed=<n>".
## The exit code is 0 when no file fails, and 1 otherwise. Read the engine error lines above the
## FAIL lines for the cause. Use this instead of --check-only, which has no autoloads and can
## exit 0 on a parse error.

const EXTENSIONS: PackedStringArray = ["gd", "tscn", "tres"]

var _skip: PackedStringArray = []
var _checked := 0
var _failed: PackedStringArray = []


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
		var path := dir_path.path_join(file_name)
		# Never load this file again: an uncached load of the running script breaks its bytecode.
		if file_name.get_extension() in EXTENSIONS and path != (get_script() as Script).resource_path:
			_check(path)


func _check(path: String) -> void:
	_checked += 1
	var res := ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_IGNORE)
	if res == null:
		_failed.append(path)
	elif res is Script and not (res as Script).can_instantiate():
		_failed.append(path)
```

## Check the copy

This block is POSIX `sh` or GDScript, not Python, so do not check it with `python3`.
Make a tiny project in a temporary folder: a `project.godot` with `config_version=5`. Run
`godot --headless --path <folder> -s <absolute path to the file>`. It prints `PARSE_CHECK checked=0 failed=0`.
