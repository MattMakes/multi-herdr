extends Node

const Core := preload("res://core/game_core.gd")

signal event(e: Dictionary)
signal changed()

const DATA_FILES: Array[String] = [Core.GENERATORS_FILE, Core.UPGRADES_FILE, Core.STAGES_FILE, Core.ECONOMY_FILE]
const SAVE_PATH := "user://lamp_road_save.json"
const SAVED_AT := "saved_at"
const FIXED_DT := 0.1
const LONGEST_FRAME_S := 0.5
const MAX_LARGE_STEPS := 4
const AUTOSAVE_S := 20.0
const TOOL_ARGS: Array[String] = ["--view-texts"]

var data: Dictionary = {}
var state: Dictionary = {}

var driven: bool = false
var uses_save: bool = false

var _left: float = 0.0
var _since_save: float = 0.0


func _init() -> void:
	data = load_data()
	state = Core.new_state(data)


func _ready() -> void:
	if not is_autoload():
		return
	if is_tool_run():
		driven = true
		return
	uses_save = true
	load_game()


func is_autoload() -> bool:
	return is_inside_tree() and get_parent() == get_tree().root and name == &"Session"


static func is_tool_run() -> bool:
	if DisplayServer.get_name() == "headless":
		return true
	for arg: String in OS.get_cmdline_user_args():
		if TOOL_ARGS.has(arg):
			return true
	return false


func _process(delta: float) -> void:
	if driven:
		return
	advance_frame(delta)
	if not uses_save:
		return
	_since_save += delta
	if _since_save >= AUTOSAVE_S:
		_since_save = 0.0
		save_game()


func advance_frame(delta: float) -> int:
	_left += maxf(delta, 0.0)
	var steps: int = 0
	if _left > LONGEST_FRAME_S:
		var large: float = Core.max_step(data)
		while _left >= FIXED_DT and steps < MAX_LARGE_STEPS:
			var dt: float = minf(_left, large)
			_left -= dt
			_emit(Core.step(state, data, dt))
			steps += 1
		if steps > 0:
			changed.emit()
		return steps
	while _left >= FIXED_DT:
		_left -= FIXED_DT
		_emit(Core.step(state, data, FIXED_DT))
		steps += 1
	if steps > 0:
		changed.emit()
	return steps


func pending_s() -> float:
	return _left


static func load_data() -> Dictionary:
	var out: Dictionary = {}
	for path: String in DATA_FILES:
		out[path] = JSON.parse_string(FileAccess.get_file_as_string(path))
	return out


func dispatch(action_id: String) -> bool:
	var events: Array = Core.apply(state, data, action_id)
	_emit(events)
	changed.emit()
	for e: Dictionary in events:
		if e["kind"] == "refused":
			return false
	return true


func click() -> void:
	click_for(1.0 / Core.CLICKS_PER_S)
	changed.emit()


func actions() -> Array:
	return Core.actions(state, data)


func advance(dt: float) -> void:
	_emit(Core.step(state, data, dt))


func click_for(seconds: float) -> void:
	_emit(Core.active(state, data, seconds))


func away(seconds: float) -> void:
	_emit(Core.away(state, data, seconds))


func rate() -> float:
	return Core.rate(state, data)


func progress() -> float:
	return Core.progress(state, data)


func max_step() -> float:
	return Core.max_step(data)


func reload_data(new_data: Dictionary) -> void:
	data = new_data
	_emit(Core.refresh(state, data))
	changed.emit()


func new_game(seed_value: int = 1) -> void:
	state = Core.new_state(data, seed_value)
	_left = 0.0
	changed.emit()


func use_state(given: Dictionary) -> void:
	var loaded: Dictionary = given.duplicate(true)
	var fresh: Dictionary = Core.new_state(data)
	for key: Variant in fresh:
		if not loaded.has(key):
			loaded[key] = fresh[key]
	loaded.erase(SAVED_AT)
	state = loaded
	_left = 0.0
	Core.refresh(state, data)
	changed.emit()


func save_game() -> void:
	if driven or not uses_save:
		return
	var file: FileAccess = FileAccess.open(SAVE_PATH, FileAccess.WRITE)
	if file != null:
		var saved: Dictionary = state.duplicate()
		saved[SAVED_AT] = Time.get_unix_time_from_system()
		file.store_string(JSON.stringify(saved))


func load_game() -> void:
	if driven or not uses_save or not FileAccess.file_exists(SAVE_PATH):
		return
	var loaded: Variant = JSON.parse_string(FileAccess.get_file_as_string(SAVE_PATH))
	if loaded is Dictionary and (loaded as Dictionary).has("light") and (loaded as Dictionary).has("levels"):
		var saved_at: float = float((loaded as Dictionary).get(SAVED_AT, Time.get_unix_time_from_system()))
		use_state(loaded)
		away(maxf(Time.get_unix_time_from_system() - saved_at, 0.0))


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST:
		save_game()


func _emit(events: Array) -> void:
	for e: Dictionary in events:
		event.emit(e)
