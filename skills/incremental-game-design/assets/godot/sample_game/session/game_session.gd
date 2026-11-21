extends Node
## The session of Lamp Road. It is the autoload `Session`.
##
## The session is the controller. It loads the data files, owns 1 state, and calls the core.
## It has no math. The view sends an intent with `dispatch`. The view listens to `event` and `changed`.
##
## 3 rules of a session (references/game-core.md, section 5):
## 1. No game time is lost. A long frame (a slow or hidden window) is stepped in larger steps, up to the
##    `max_step` of the core. 1 frame catches up at most `MAX_LARGE_STEPS x max_step` game seconds
##    (4 x 3600 s in this game). The rest stays in `pending_s` for the next frames.
## 2. A tool run does not touch the game of the player. In a headless run of the project (the command line of
##    the tuning bench, a test) and in a run with a bench mode in its arguments, the autoload does not read the
##    save file, does not write it, and does not move the clock. The same holds while a tool drives the session
##    (`driven`).
## 3. A view can get a session of its own. A session that is not the autoload (`Session.new()` of this script,
##    for a view in a SubViewport or in a test) starts a new game, moves its own clock, and never uses the save file.

const Core := preload("res://core/game_core.gd")

## The core made an event. The view shows a reaction.
signal event(e: Dictionary)
## The state changed. The view reads it again.
signal changed()

const DATA_FILES: Array[String] = [Core.GENERATORS_FILE, Core.UPGRADES_FILE, Core.STAGES_FILE, Core.ECONOMY_FILE]
const SAVE_PATH := "user://lamp_road_save.json"
const SAVED_AT := "saved_at"
## The length of 1 step of the live game, in seconds.
const FIXED_DT := 0.1
## A frame longer than this is a slow or hidden window. The session then takes larger steps.
const LONGEST_FRAME_S := 0.5
## The most large steps in 1 frame. Each one is at most `Core.max_step(data)` game seconds.
const MAX_LARGE_STEPS := 4
const AUTOSAVE_S := 20.0
## The arguments of the command line of the tuning bench that start the main scene with a window or with none.
const TOOL_ARGS: Array[String] = ["--view-texts"]

var data: Dictionary = {}
var state: Dictionary = {}

## True while a tool drives the session (the legacy adapter of the tuning bench, a tool run).
## The session then uses no frame time and writes no save file.
var driven: bool = false
## True only for the autoload in the game of the player. Only then the session reads and writes the save file.
var uses_save: bool = false

var _left: float = 0.0
var _since_save: float = 0.0


func _init() -> void:
	# The data and a new game are there before `_ready`: a session of a view or a test needs no scene tree.
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


## True when this node is the autoload `Session` of the project.
func is_autoload() -> bool:
	return is_inside_tree() and get_parent() == get_tree().root and name == &"Session"


## True in a run of a tool: a headless run, or a run of the command line of the tuning bench.
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


## Take the time of 1 frame. A short frame gives steps of `FIXED_DT`. A long frame gives larger steps, so the
## game loses no time. Return the count of steps.
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
		# More time than the catch-up of 1 frame: the rest waits in `_left`.
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


## Return the game time that the session did not step yet.
func pending_s() -> float:
	return _left


## Read each data file. Return the map from the file path to its content.
static func load_data() -> Dictionary:
	var out: Dictionary = {}
	for path: String in DATA_FILES:
		out[path] = JSON.parse_string(FileAccess.get_file_as_string(path))
	return out


## Do 1 action of the player. Return false when the core refuses it.
func dispatch(action_id: String) -> bool:
	var events: Array = Core.apply(state, data, action_id)
	_emit(events)
	changed.emit()
	for e: Dictionary in events:
		if e["kind"] == "refused":
			return false
	return true


## The player clicks 1 time.
func click() -> void:
	click_for(1.0 / Core.CLICKS_PER_S)
	changed.emit()


## Return the actions that the player can do or see now.
func actions() -> Array:
	return Core.actions(state, data)


## Advance the game by a time that the caller gives. A tool that drives the session uses this.
func advance(dt: float) -> void:
	_emit(Core.step(state, data, dt))


## The player does the manual action for this time.
func click_for(seconds: float) -> void:
	_emit(Core.active(state, data, seconds))


## The game was closed for this time. The core applies the offline rule.
func away(seconds: float) -> void:
	_emit(Core.away(state, data, seconds))


func rate() -> float:
	return Core.rate(state, data)


func progress() -> float:
	return Core.progress(state, data)


func max_step() -> float:
	return Core.max_step(data)


## Use new numbers in the game that runs. The state stays.
func reload_data(new_data: Dictionary) -> void:
	data = new_data
	_emit(Core.refresh(state, data))
	changed.emit()


## Start a new game.
func new_game(seed_value: int = 1) -> void:
	state = Core.new_state(data, seed_value)
	_left = 0.0
	changed.emit()


## Use this state in the place of the state of now. A tool calls this (`bench_cli.gd --view-texts --state`), and
## `load_game` uses it. `given` is a state of the core or the content of a save file. No time away is applied.
func use_state(given: Dictionary) -> void:
	var loaded: Dictionary = given.duplicate(true)
	# A save of an older version can miss a key. The new state gives each missing key.
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
		# The time from the save to now is time away. The core has the offline rule.
		away(maxf(Time.get_unix_time_from_system() - saved_at, 0.0))


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST:
		save_game()


func _emit(events: Array) -> void:
	for e: Dictionary in events:
		event.emit(e)
