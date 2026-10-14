# Script: run_tests.gd (headless test runner)

Usage: `godot --headless --path <project> -s res://tests/run_tests.gd -- --filter=<text> --list-unchecked`

A `SceneTree` script that runs every `res://tests/**/test_*.gd` file. `--filter=<text>` keeps the files whose path holds the text. A registered `Logger` fails a test on any engine error or `push_error`, and a failed assert fails it too. The runner counts skips as skips, prints passed, failed and skipped, and exits 1 on a failure or when 0 tests ran. `--list-unchecked` lists each passing test with 0 assert calls. An optional `res://tests/reset_tests.gd` with a static `reset(tree)` runs before each test file. Put the file and `test_case.gd` (see [script-test-case.md](script-test-case.md)) in `res://tests/`. It needs Godot 4.5 or newer.

Copy the block into a file named `run_tests.gd`.

```gdscript
extends SceneTree
## A headless test runner for Godot 4.5+ (it needs the Logger class).
## Usage: copy this file and test_case.gd to res://tests/, then run
##   godot --headless --path <project> -s res://tests/run_tests.gd
##   godot --headless --path <project> -s res://tests/run_tests.gd -- --filter=<text> --list-unchecked
## It runs every res://tests/**/test_*.gd file. --filter=<text> keeps the files whose path holds <text>.
## A failed assert, push_error or a script error during a test fails that test.
## --list-unchecked prints each passing test that made 0 assert calls (a test audit aid).
## It prints "N passed, N failed, N skipped". It exits 1 on a failure or when 0 tests ran.
## Optional: res://tests/reset_tests.gd with a static reset(tree) runs before each test file,
## so autoload state from one file cannot reach the next.


class ErrorCounter extends Logger:
	var mutex := Mutex.new()
	var lines: PackedStringArray = []

	func _log_error(function: String, file: String, line: int, code: String, rationale: String,
			_editor_notify: bool, error_type: int, _script_backtraces: Array[ScriptBacktrace]) -> void:
		if error_type == ERROR_TYPE_WARNING:
			return
		mutex.lock()
		lines.append("%s:%d %s %s" % [file, line, code, rationale])
		mutex.unlock()

	func take() -> PackedStringArray:
		mutex.lock()
		var out := lines.duplicate()
		lines.clear()
		mutex.unlock()
		return out


const TESTS_ROOT := "res://tests"
const RESET_SCRIPT := "res://tests/reset_tests.gd"
const BASE_FILE := "test_case.gd"

var _errors := ErrorCounter.new()


func _initialize() -> void:
	OS.add_logger(_errors)
	_run.call_deferred()


func _run() -> void:
	var filter := ""
	var list_unchecked := false
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--filter="):
			filter = arg.substr(9)
		elif arg == "--list-unchecked":
			list_unchecked = true
	var reset: Script = load(RESET_SCRIPT) if ResourceLoader.exists(RESET_SCRIPT) else null
	var files := _find_tests(TESTS_ROOT)
	files.sort()
	_errors.take()  # start-up noise is not a test failure
	var passed := 0
	var failed := 0
	var skipped := 0
	var report: PackedStringArray = []
	var unchecked: PackedStringArray = []
	for path in files:
		if filter != "" and not path.contains(filter):
			continue
		if reset != null:
			reset.call("reset", self)
		var script: GDScript = load(path)
		if script == null or not script.can_instantiate():
			failed += 1
			report.append("FAIL %s: the script does not load" % path)
			continue
		var seen := {}
		for m: Dictionary in script.get_script_method_list():
			var test_name: String = m["name"]
			if not test_name.begins_with("test_") or seen.has(test_name):
				continue
			seen[test_name] = true
			var t: Variant = script.new()
			t.tree = self
			_errors.take()
			if t.has_method("before_each"):
				await t.before_each()
			await t.call(test_name)
			if t.has_method("after_each"):
				await t.after_each()
			t._free_nodes()
			await process_frame
			var problems: PackedStringArray = t.failures
			for e in _errors.take():
				problems.append("engine error: " + e)
			if not t.skip_reason.is_empty():
				skipped += 1
				print("  skip %s :: %s: %s" % [path.get_file(), test_name, t.skip_reason])
			elif problems.is_empty():
				passed += 1
				print("  ok   %s :: %s" % [path.get_file(), test_name])
				if t.checks == 0:
					unchecked.append("UNCHECKED %s :: %s" % [path.trim_prefix("res://"), test_name])
			else:
				failed += 1
				print("  FAIL %s :: %s" % [path.get_file(), test_name])
				for p in problems:
					report.append("FAIL %s :: %s: %s" % [path.get_file(), test_name, p])
	print("")
	for r in report:
		print(r)
	if list_unchecked:
		for u in unchecked:
			print(u)
		print("%d unchecked" % unchecked.size())
	print("%d passed, %d failed, %d skipped" % [passed, failed, skipped])
	OS.remove_logger(_errors)
	_clean_scratch()
	# 0 tests is a failure: a wrong filter or a broken file must not look green.
	quit(1 if failed > 0 or passed + skipped == 0 else 0)


## Removes user://tests/<pid>, the scratch folder that test_case.gd scratch_path() fills.
func _clean_scratch() -> void:
	var dir := "user://tests".path_join(str(OS.get_process_id()))
	var access := DirAccess.open(dir)
	if access == null:
		return
	for file_name in access.get_files():
		access.remove(file_name)
	DirAccess.remove_absolute(ProjectSettings.globalize_path(dir))


func _find_tests(dir_path: String) -> PackedStringArray:
	var out: PackedStringArray = []
	var dir := DirAccess.open(dir_path)
	if dir == null:
		return out
	for sub in dir.get_directories():
		out.append_array(_find_tests(dir_path.path_join(sub)))
	for f in dir.get_files():
		if f.begins_with("test_") and f.ends_with(".gd") and f != BASE_FILE:
			out.append(dir_path.path_join(f))
	return out
```

## Smoke test

Smoke test for the GDScript files: make an empty project in a temp folder (a `project.godot` with
`config_version=5`), copy both scripts to `res://tests/`, add 1 `test_*.gd` file, and run the
runner. Check: it prints "1 passed" and exits 0.
