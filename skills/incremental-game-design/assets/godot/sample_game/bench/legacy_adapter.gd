extends "res://addons/tuning_bench/bench_adapter.gd"

var _session: Node = null
var _events: Array = []
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
		_events.append(e)
