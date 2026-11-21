extends "res://addons/tuning_bench/bench_adapter.gd"
## The adapter for a game whose math is a pure core.
##
## A core is a script with static functions over a plain-data state:
##     new_state(data, seed) -> Dictionary        step(state, data, dt) -> Array of events
##     actions(state, data) -> Array              apply(state, data, action_id) -> Array of events
##     active(state, data, seconds) -> Array      rate(state, data) -> float
##     progress(state, data) -> float             wealth(state, data) -> float
##     max_step(data) -> float                    away(state, data, seconds) -> Array of events
## `bench.json` names the core with the key "core". The game then needs no adapter of its own.
## This adapter touches no autoload and no scene, so the panel can run the simulation on a thread.
##
## The cost of this adapter is 1 call by name for each contract call: about 0.1 microseconds (measured, Godot
## 4.7). The time of a run is the time of the functions of the core. `bench_cli.gd --run --profile` prints
## the time of each contract function.
##
## An action can have the key "bot": false. It is a free choice of the player (a selector, a switch). The view
## shows it and the bot never does it (`bench_adapter.gd`, `actions`).
##
## A HOLD ACTION (the income is higher while the player holds a button). The contract has no argument for it in
## `step`. Use this pattern:
##     static func active(state, data, seconds) -> Array:
##         state["hold_credit"] = float(state["hold_credit"]) + seconds     # the hand gives hold time
##         return []
##     static func step(state, data, dt) -> Array:
##         var credit: float = float(state["hold_credit"])
##         var held: float = minf(credit, dt)                               # the part of this step with the hold
##         state["hold_credit"] = credit - held
##         ... income = rate x (dt - held) + rate x hold_factor x held ...
## - The session calls `active(dt)` and then `step(dt)` in each frame while the button is down.
## - The bench calls `step` for `decision_s` and then `active(decision_s x active_share)` 1 time. The credit is
##   thus used in the steps of the next decision: the hold of the bot is 1 decision late, and the share of the
##   time with the hold is `active_share`.
## - `hold_credit` is state. It is in the save file, or the session sets it to 0 after a load.

const REQUIRED: Array[String] = ["new_state", "step", "actions", "apply", "rate", "progress"]
## The kinds of event that the adapter passes on to the bench. Each other kind of the game (a kind of its own,
## "reset", "refused") stays in the core: the bench reads a reset and a purchase from the action that it did.
const PASS_ON: Dictionary = {"unlock": true, "advance": true, "reward": true}
## A purchase that the game made by itself. `step`, `active` and `away` pass each one on. `apply` passes on
## only a purchase of another id than the action that the bot asked for.
const BOUGHT := "bought"

var _core: GDScript = null
var _state: Dictionary = {}
var _data: Dictionary = {}
var _events: Array = []
var _has_active: bool = false
var _has_wealth: bool = false
var _has_max_step: bool = false
var _has_away: bool = false
# The functions of the core. The adapter looks each name up 1 time, not at each call.
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


## Push the values into the game that runs: call `reload_data(data)` of the session autoload.
## The key "session" of `bench.json` names the autoload. The default name is "Session".
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


## Return a hash of the state. The replay check compares 2 hashes.
## The hash is the SHA-256 of the state as JSON with sorted keys and full precision. It covers each key of the
## state: a refactor that adds a key to the state gives another hash.
func state_hash() -> String:
	return JSON.stringify(_state, "", true, true).sha256_text()


## Return a copy of the state. `bench_cli.gd --view-texts --state bot:<hours>` gives it to the session.
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

