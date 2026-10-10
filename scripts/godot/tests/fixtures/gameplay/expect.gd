extends RefCounted
## The check counter of the gameplay scenarios (scripts/godot/gameplay_scenarios.py).

var passed: int = 0
var failed: int = 0


## A runtime error stops _initialize before finish(): the watchdog then
## quits with exit 2 instead of letting the run wait for the runner's bound.
func watch(tree: SceneTree, seconds: float = 20.0) -> void:
	tree.create_timer(seconds, true, false, true).timeout.connect(func() -> void:
		printerr("ERROR: scenario watchdog: no finish() after %s s" % seconds)
		tree.quit(2))


func check(ok: bool, what: String) -> void:
	if ok:
		passed += 1
		print("ok   ", what)
	else:
		failed += 1
		printerr("FAIL ", what)


func finish(tree: SceneTree) -> void:
	print("scenario: %d of %d checks pass" % [passed, passed + failed])
	tree.quit(1 if failed > 0 else 0)
