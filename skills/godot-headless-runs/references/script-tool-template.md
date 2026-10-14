# Script: tool_template.gd

Usage: `godot --headless --path <project> -s <absolute path to the copy> -- --limit=60`

The tool template is the start of a headless SceneTree tool: a bot, a probe, a capture, a bench or a fixture maker. Rename the copy and fill in `_run()`. It reaches autoloads by node path, loads game scripts at run time, and starts the work on the first frame. Its watchdog quits with code 2 and prints `TOOL_TIMEOUT` when the limit runs out. The last line of a normal run is `TOOL_DONE code=<n>`. Keep the copy outside the test folder. Scan the log for `SCRIPT ERROR` lines before you trust a result, because a run-time error can still end with `TOOL_DONE code=0`.

Copy the block into a file named `tool_template.gd` before you run it.

```gdscript
extends SceneTree
## Run: godot --headless --path <project> -s <absolute path to the copy> -- --limit=60
## Needs: a Godot 4 binary (GDScript, not Python). A template for a headless tool (a bot, probe,
## capture, bench or fixture maker). Copy it, rename it, and fill in _run(). Keep the copy outside
## the test folder.
## Rules that keep it compiling: a `-s` script compiles before the autoloads exist, so do not
## name an autoload or a `class_name` script here. Reach autoloads with root.get_node() and
## call(), load game scripts with load() at run time, and keep game objects untyped.
## The watchdog quits with code 2 when the limit runs out, because a script error stops the
## normal quit() and the run would hang. The last line is "TOOL_DONE code=<n>" or "TOOL_TIMEOUT".
## A run-time error inside _run() can still end with "TOOL_DONE code=0", so scan the log for
## "SCRIPT ERROR" lines (godot-run.sh does) before you trust any result.

const DEFAULT_LIMIT_S := 120.0

var _ended := false


func _initialize() -> void:
	var limit := DEFAULT_LIMIT_S
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--limit="):
			limit = arg.trim_prefix("--limit=").to_float()
	create_timer(limit).timeout.connect(_on_watchdog)
	# Do no game work here. Autoloads finish their _ready on the first frame.
	process_frame.connect(_start, CONNECT_ONE_SHOT)


func _start() -> void:
	var code: int = await _run()
	_end(code)


## Put the tool's work here and replace the examples. Return 0 for success. Await frames as needed.
func _run() -> int:
	# Example: an autoload by node path, not by its global name.
	var config = root.get_node_or_null("Config")
	if config == null:
		push_error("autoload Config not found")
		return 1
	print("value=", config.call("value"))
	# Example: a game script by path at run time, not by class_name or preload().
	# var stage = load("res://tools/my_stage.gd").new()
	# root.add_child(stage)
	await process_frame
	return 0


func _end(code: int) -> void:
	if _ended:
		return
	_ended = true
	print("TOOL_DONE code=%d" % code)
	quit(code)


func _on_watchdog() -> void:
	if _ended:
		return
	_ended = true
	print("TOOL_TIMEOUT")
	quit(2)
```

## Check the copy

This block is POSIX `sh` or GDScript, not Python, so do not check it with `python3`.
Make a tiny project in a temporary folder: a `project.godot` with `config_version=5`. Run
`godot --headless --path <folder> -s <absolute path to the file>`. It prints `TOOL_DONE code=1`, because the tiny project has no
`Config` autoload.
