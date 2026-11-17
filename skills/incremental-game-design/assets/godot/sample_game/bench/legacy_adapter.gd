extends "res://addons/tuning_bench/bench_adapter.gd"
## A hand-written adapter for Lamp Road. `bench/bench.legacy.json` names it.
##
## Lamp Road does not need this adapter: `bench/bench.json` names the core, and the bench then has its own adapter.
## This file shows the way for a game with no core: the adapter drives the game through its autoload.
## The bench runs such an adapter in a second Godot process, so the game that the designer plays does not change.

var _session: Node = null
var _events: Array = []
## The id of the action that the bot asked for with `do_action`, while the session does it.
var _asked: String = ""


func bench_prepare() -> String:
	var loop: MainLoop = Engine.get_main_loop()
	if loop is SceneTree:
		_session = (loop as SceneTree).root.get_node_or_null("Session")
	if _session == null:
		return "the autoload Session is not in the scene tree"
	if not _session.is_connected("event", _on_event):
		_session.connect("event", _on_event)
	return ""


func setup(data: Dictionary) -> void:
	# The session stops its frame clock and writes no save file from here.
	_session.set("driven", true)
	_session.set("data", data)
	_session.call("new_game", bench_seed)
	_events = []


func step(dt: float) -> void:
	_session.call("advance", dt)


func actions() -> Array:
	return _session.call("actions")


func do_action(id: String) -> bool:
	_asked = id
	var done: bool = _session.call("dispatch", id)
	_asked = ""
	return done


func rate() -> float:
	return _session.call("rate")


func progress() -> float:
	return _session.call("progress")


func max_step() -> float:
	return _session.call("max_step")


func wealth() -> float:
	return float((_session.get("state") as Dictionary)["light"])


func active(seconds: float) -> void:
	_session.call("click_for", seconds)


func away(seconds: float) -> void:
	_session.call("away", seconds)


func events() -> Array:
	var out: Array = _events
	_events = []
	return out


func apply_live(data: Dictionary) -> bool:
	_session.call("reload_data", data)
	return true


func _on_event(e: Dictionary) -> void:
	if ["unlock", "advance", "reward"].has(str(e["kind"])):
		_events.append(e)
	elif str(e["kind"]) == "bought" and str(e["id"]) != _asked:
		# A purchase that the bot did not ask for: the lamplighter of the game made it. The bench counts it
		# as an auto purchase.
		_events.append(e)
