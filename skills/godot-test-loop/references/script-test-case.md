# Script: test_case.gd (test base class)

Usage: `extends "res://tests/test_case.gd"`

The base class that every test file extends. It gives counted asserts (`assert_true`, `assert_false`, `assert_eq` with a type check, `assert_near`), `skip()`, `skip_if_headless()`, `add_node()` with auto-free, `frames()`, `seconds()`, `tap()` for input injection, and `scratch_path()` for a per-process scratch file. The runner in [script-run-tests.md](script-run-tests.md) loads it. Do not run it alone. Put it in `res://tests/`.

Copy the block into a file named `test_case.gd`.

```gdscript
extends RefCounted
## The base class for test files that run_tests.gd runs (Godot 4.5+). Do not run it alone.
## Usage: a test file res://tests/<folder>/test_<name>.gd starts with
##   extends "res://tests/test_case.gd"
## Every method whose name starts with "test_" is one test. A test can use await.
## Optional hooks: before_each() and after_each().

var tree: SceneTree
var failures: PackedStringArray = []
var skip_reason: String = ""
## The assert calls of this test. The runner lists a passing test with 0 under --list-unchecked.
var checks: int = 0
var _nodes: Array[Node] = []


func fail(msg: String) -> void:
	failures.append(msg)


## Marks the test as skipped. The runner counts it as a skip, not as a pass.
func skip(reason: String) -> void:
	skip_reason = reason


## Skips the test in a headless run. Use it for a test that needs a real renderer or a draw.
func skip_if_headless() -> bool:
	if DisplayServer.get_name() == "headless":
		skip("needs a window")
		return true
	return false


func assert_true(cond: bool, msg: String = "expected true") -> void:
	checks += 1
	if not cond:
		fail(msg)


func assert_false(cond: bool, msg: String = "expected false") -> void:
	checks += 1
	if cond:
		fail(msg)


## Fails on a type difference too, so an int 3 does not equal a float 3.0 from JSON.
func assert_eq(got: Variant, want: Variant, msg: String = "") -> void:
	checks += 1
	if typeof(got) != typeof(want) or got != want:
		fail("%s got %s, want %s" % [msg, var_to_str(got), var_to_str(want)])


func assert_near(got: float, want: float, tol: float = 1e-3, msg: String = "") -> void:
	checks += 1
	if is_nan(got) or absf(got - want) > tol:
		fail("%s got %.9f, want %.9f (tol %s)" % [msg, got, want, str(tol)])


## Adds a node to the scene tree for this test. The runner frees it after the test.
func add_node(n: Node) -> Node:
	tree.root.add_child(n)
	_nodes.append(n)
	return n


## Waits n process frames.
func frames(n: int = 1) -> void:
	for i in n:
		await tree.process_frame


## Waits a time in seconds. Headless frames run faster than real time, so wait on a timer, not a frame count.
func seconds(t: float) -> void:
	await tree.create_timer(t).timeout


## Presses and releases an input action the way a player does. The frame after the release
## lets the code see a new press edge on the next tap.
func tap(action: StringName, hold_frames: int = 1) -> void:
	Input.action_press(action)
	await frames(hold_frames)
	Input.action_release(action)
	await frames(1)


## A file path in user://tests/<pid>/. Two suites that run at the same time never share it.
## The runner removes the folder at the end of the run.
func scratch_path(file_name: String) -> String:
	var dir := "user://tests".path_join(str(OS.get_process_id()))
	DirAccess.make_dir_recursive_absolute(ProjectSettings.globalize_path(dir))
	return dir.path_join(file_name)


func _free_nodes() -> void:
	for n in _nodes:
		if is_instance_valid(n):
			n.queue_free()
	_nodes.clear()
```
