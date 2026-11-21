extends "res://addons/tuning_bench/bench_adapter.gd"

const REQUIRED: Array[String] = ["new_state", "step", "actions", "apply", "rate", "progress"]
const PASS_ON: Dictionary = {"unlock": true, "advance": true, "reward": true}
const BOUGHT := "bought"

var _core: GDScript = null
var _state: Dictionary = {}
var _data: Dictionary = {}
var _events: Array = []
var _has_active: bool = false
var _has_wealth: bool = false
var _has_max_step: bool = false
var _has_away: bool = false
var _fn_step: Callable = Callable()
var _fn_actions: Callable = Callable()
var _fn_apply: Callable = Callable()
var _fn_rate: Callable = Callable()
var _fn_progress: Callable = Callable()
var _fn_wealth: Callable = Callable()
var _fn_active: Callable = Callable()
var _fn_away: Callable = Callable()


func bench_prepare() -> String:
	var path: String = str(bench_config.get("core", ""))
	if path == "":
		return "bench.json has no key \"core\""
	if not ResourceLoader.exists(path):
		return "the core %s does not exist" % path
	var loaded: Variant = load(path)
	if not loaded is GDScript or not (loaded as GDScript).can_instantiate():
		return "the core %s does not load" % path
	_core = loaded
	var names: Dictionary = {}
	for method: Dictionary in _core.get_script_method_list():
		names[method["name"]] = true
	for method: String in REQUIRED:
		if not names.has(method):
			return "the core %s does not have the static function %s()" % [path, method]
	_has_active = names.has("active")
	_has_wealth = names.has("wealth")
	_has_max_step = names.has("max_step")
	_has_away = names.has("away")
	_fn_step = Callable(_core, "step")
	_fn_actions = Callable(_core, "actions")
	_fn_apply = Callable(_core, "apply")
	_fn_rate = Callable(_core, "rate")
	_fn_progress = Callable(_core, "progress")
	if _has_wealth:
		_fn_wealth = Callable(_core, "wealth")
	if _has_active:
		_fn_active = Callable(_core, "active")
	if _has_away:
		_fn_away = Callable(_core, "away")
	return ""


func setup(data: Dictionary) -> void:
	_data = data
	_events = []
	var made: Variant = _core.call("new_state", data, bench_seed)
	if made is Dictionary:
		_state = made
	else:
		_state = {}
		bench_errors.append("new_state() of the core did not return a dictionary")


func step(dt: float) -> void:
	_keep(_fn_step.call(_state, _data, dt), "")


func actions() -> Array:
	var out: Variant = _fn_actions.call(_state, _data)
	if out is Array:
		return out
	bench_errors.append("actions() of the core did not return a list")
	return []


func do_action(id: String) -> bool:
	var out: Variant = _fn_apply.call(_state, _data, id)
	if not out is Array:
		bench_errors.append("apply() of the core did not return a list")
		return false
	for event: Variant in out:
		if event is Dictionary and str((event as Dictionary).get("kind", "")) == "refused":
			return false
	_keep(out, id)
	return true


func rate() -> float:
	return float(_fn_rate.call(_state, _data))


func progress() -> float:
	return float(_fn_progress.call(_state, _data))


func max_step() -> float:
	if _has_max_step:
		return float(_core.call("max_step", _data))
	return 1.0


func wealth() -> float:
	if _has_wealth:
		return float(_fn_wealth.call(_state, _data))
	return -1.0


func active(seconds: float) -> void:
	if _has_active:
		_keep(_fn_active.call(_state, _data, seconds), "")


func away(seconds: float) -> void:
	if _has_away:
		_keep(_fn_away.call(_state, _data, seconds), "")
	else:
		super.away(seconds)


func events() -> Array:
	var out: Array = _events
	_events = []
	return out


func apply_live(data: Dictionary) -> bool:
	var loop: MainLoop = Engine.get_main_loop()
	if not loop is SceneTree:
		return false
	var name: String = str(bench_config.get("session", "Session"))
	var node: Node = (loop as SceneTree).root.get_node_or_null(NodePath(name))
	if node == null or not node.has_method("reload_data"):
		return false
	node.call("reload_data", data)
	return true


func isolated() -> bool:
	return true


func state_hash() -> String:
	return JSON.stringify(_state, "", true, true).sha256_text()


func state_copy() -> Dictionary:
	return _state.duplicate(true)


func _keep(out: Variant, asked_id: String) -> void:
	if not out is Array or (out as Array).is_empty():
		return
	for event: Variant in out:
		if not event is Dictionary:
			continue
		var kind: String = str((event as Dictionary).get("kind", ""))
		if PASS_ON.has(kind):
			_events.append(event)
		elif kind == BOUGHT and (asked_id == "" or str((event as Dictionary).get("id", "")) != asked_id):
			_events.append(event)

