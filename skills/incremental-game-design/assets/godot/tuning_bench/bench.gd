extends Node
## The autoload `TuningBench`.
##
## It loads `res://bench/bench.json`, holds the value k of each control, and starts the simulations.
## The hotkey (default F2) opens the panel. The bench works only in a debug build, or when `bench.json`
## has `"release": true`.
##
## A simulation never changes the game that runs:
## - an adapter that is isolated (a game with a core) runs on a thread with its own state;
## - each other adapter runs in a second Godot process.

const BenchJson := preload("res://addons/tuning_bench/bench_json.gd")
const Knobs := preload("res://addons/tuning_bench/bench_knobs.gd")
const Runner := preload("res://addons/tuning_bench/bench_runner.gd")
const AdapterBase := preload("res://addons/tuning_bench/bench_adapter.gd")
const PANEL_SCRIPT := "res://addons/tuning_bench/bench_panel.gd"
const CLI_SCRIPT := "res://addons/tuning_bench/bench_cli.gd"

const DEBOUNCE_S := 0.25
const POLL_S := 0.05
const WORK_DIR := "user://tuning_bench"
const SETTINGS_DIR := "res://bench/settings"
## The largest count of child processes of the self-check that the panel starts.
const SELFCHECK_JOBS := 4

## A new result of a run is in `result`.
signal result_changed()
## The status line changed.
signal status_changed()
## A value k changed from outside the sliders (load, reset, fit).
signal k_changed()
## `set_k` set the value of 1 control. The panel moves the slider of that control.
signal k_set(id: String, value: float)
## A tool ended. `kind` is "rank", "fit", "selfcheck" or "replay". `payload` is the content of its result file.
signal task_done(kind: String, payload: Dictionary)

## True when the bench loaded and can run.
var enabled: bool = false
## The cause when the bench did not load.
var error: String = ""
var bench_path: String = Knobs.DEFAULT_BENCH
var ctx: Dictionary = {}
## The value k of each control.
var k: Dictionary = {}
## The targets that the designer changed in the panel.
var target_changes: Dictionary = {}
## The last result of a run. Empty before the first run. While `pending` is true it is the result of the main
## player alone (the key "pending_players" lists the players that still run).
var result: Dictionary = {}
var pending: bool = false
## The content of the last self-check result. Empty before the first self-check.
var check: Dictionary = {}
var status: String = ""
var live_note: String = ""
var running: bool = false
var last_seconds: float = 0.0
var mode: String = "process"
## Two stored settings for the comparison: {"A": {"k", "targets", "result"}, "B": ...}.
var slots: Dictionary = {}

## Each file of `sources` parsed, with the controls applied. The game can read its numbers from here.
var data: Dictionary:
	get:
		if _data_cache.is_empty() and enabled:
			_data_cache = Knobs.data_for(ctx, k)
		return _data_cache

var _data_cache: Dictionary = {}
var _adapter: AdapterBase = null
var _panel: CanvasLayer = null
var _hotkey: Key = KEY_F2
var _debounce: float = -1.0
var _poll: float = 0.0
var _serial: int = 0
var _started_ms: int = 0
var _pid: int = -1
var _out_path: String = ""
var _partial_path: String = ""
var _thread: Thread = null
var _cancel: Dictionary = {}
var _old_threads: Array[Thread] = []
var _task_pid: int = -1
var _task_kind: String = ""
var _task_out: String = ""
var _task_started_ms: int = 0


func _ready() -> void:
	set_process(false)
	set_process_input(false)
	# The command line of the bench runs this project too. The autoload then does nothing.
	if DisplayServer.get_name() == "headless":
		return
	for arg: String in OS.get_cmdline_user_args():
		if ["--run", "--selfcheck", "--rank", "--fit", "--replay", "--view-texts", "--probe"].has(arg):
			return
	if not FileAccess.file_exists(bench_path):
		return
	if not load_bench():
		push_warning("tuning bench: " + error)
		return
	set_process(true)
	set_process_input(true)


## Load the bench file again. Return false when it does not load. `error` then has the cause.
func load_bench() -> bool:
	enabled = false
	var built: Dictionary = Knobs.build(bench_path)
	if not built["ok"]:
		error = built["error"]
		return false
	var bench: Dictionary = built["bench"]
	if not OS.is_debug_build() and not bench.get("release", false):
		error = "the bench is off in a release build"
		return false
	ctx = built
	var kept: Dictionary = k
	k = Knobs.base_k(ctx)
	for id: Variant in kept:
		var entry: Dictionary = (ctx["by_id"] as Dictionary).get(id, {})
		if not entry.is_empty() and entry["kind"] == "player":
			k[id] = kept[id]
	_data_cache = {}
	var code: Key = OS.find_keycode_from_string(str(bench.get("hotkey", "F2")))
	if code != KEY_NONE:
		_hotkey = code
	var made: Dictionary = Runner.make_adapter(ctx)
	if made["ok"]:
		_adapter = made["adapter"]
		mode = "thread" if _adapter.isolated() else "process"
	else:
		_adapter = null
		mode = "process"
		push_warning("tuning bench: " + str(made["error"]))
	# The result of the last self-check of the project, when it is there. Meter 8 shows its numbers.
	if check.is_empty() and FileAccess.file_exists(Knobs.DEFAULT_SELFCHECK):
		var found: Dictionary = BenchJson.read_file(Knobs.DEFAULT_SELFCHECK)
		if found["ok"] and found["value"] is Dictionary and str((found["value"] as Dictionary).get("format", "")) == "tuning-bench-check":
			check = found["value"]
	error = ""
	enabled = true
	return true


## Return the line "not reviewed: N numbers in M files" of the ledger, or "" when each number was reviewed.
## `pw_bench.py` writes the key "review" of `bench.json`.
func review_line() -> String:
	var review: Variant = (ctx.get("bench", {}) as Dictionary).get("review", {})
	if not review is Dictionary or int((review as Dictionary).get("not_reviewed", 0)) <= 0:
		return ""
	return "not reviewed: %d numbers in %d files" % [int((review as Dictionary)["not_reviewed"]), int((review as Dictionary).get("files", 0))]


## Return the line "N texts hold a number", or "" when the ledger has no such text that nobody accepted.
## `pw_bench.py` writes the key "texts" of `bench.json`: each text of the game that holds a number. An entry
## with the key "ok" was accepted and does not count.
func texts_line() -> String:
	var texts: Variant = (ctx.get("bench", {}) as Dictionary).get("texts", [])
	if not texts is Array:
		return ""
	var open: int = 0
	for entry: Variant in texts:
		if entry is Dictionary and not (entry as Dictionary).has("ok"):
			open += 1
	if open <= 0:
		return ""
	return "%d texts hold a number" % open


func _input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo and (event as InputEventKey).keycode == _hotkey:
		toggle_panel()
		get_viewport().set_input_as_handled()


## Open or close the panel.
func toggle_panel() -> void:
	if _panel == null:
		open_panel()
	else:
		_panel.visible = not _panel.visible


## Open the panel. The first open starts a run.
func open_panel() -> CanvasLayer:
	if not enabled:
		return null
	if _panel == null:
		_panel = (load(PANEL_SCRIPT) as GDScript).new()
		_panel.call("setup", self)
		add_child(_panel)
		if result.is_empty():
			request_run()
	_panel.visible = true
	return _panel


## Return the control with this id, or an empty dictionary.
func control(id: String) -> Dictionary:
	return (ctx["by_id"] as Dictionary).get(id, {})


## Set the value k of 1 control. A run starts after a short pause, so a moving slider starts 1 run only.
## The panel shows the new value: a script that calls this method moves the slider.
func set_k(id: String, value: float) -> void:
	if not enabled or not (ctx["by_id"] as Dictionary).has(id):
		return
	k[id] = value
	_changed()
	k_set.emit(id, value)


## Set each control back to its base value.
func reset_to_base() -> void:
	k = Knobs.base_k(ctx)
	target_changes = {}
	_changed()
	k_changed.emit()


## Replace each value k. A missing control gets its base value.
func use_k(new_k: Dictionary, new_targets: Dictionary = {}) -> void:
	k = Knobs.base_k(ctx)
	for id: Variant in new_k:
		if (ctx["by_id"] as Dictionary).has(id):
			k[id] = float(new_k[id])
	target_changes = new_targets.duplicate(true)
	_changed()
	k_changed.emit()


## Change 1 target, for example ("length_h", "about", 5.0).
func set_target(key: String, sub: String, value: float) -> void:
	if not target_changes.has(key):
		target_changes[key] = {}
	(target_changes[key] as Dictionary)[sub] = value
	_cancel_run()
	_debounce = DEBOUNCE_S
	_set_status("simulating…")


## Return the targets with the changes of the designer.
func targets() -> Dictionary:
	return Knobs.targets(ctx, target_changes)


func _changed() -> void:
	_data_cache = {}
	_apply_live()
	# A run with the old values has no use. Stop it now.
	_cancel_run()
	_debounce = DEBOUNCE_S
	_set_status("simulating…")


func _apply_live() -> void:
	var took: bool = false
	if _adapter != null:
		took = _adapter.apply_live(data)
	live_note = "the game that runs has the new values" if took else "restart to apply"


func _set_status(text: String) -> void:
	status = text
	status_changed.emit()


func _process(delta: float) -> void:
	if _debounce >= 0.0:
		_debounce -= delta
		if _debounce < 0.0:
			request_run()
	_poll += delta
	if _poll < POLL_S:
		return
	_poll = 0.0
	_poll_run()
	_poll_task()
	for index: int in range(_old_threads.size() - 1, -1, -1):
		if not _old_threads[index].is_alive():
			_old_threads[index].wait_to_finish()
			_old_threads.remove_at(index)


## Start a run with the values of now. A run that is in progress stops.
func request_run() -> void:
	if not enabled:
		return
	_cancel_run()
	_debounce = -1.0
	_serial += 1
	_started_ms = Time.get_ticks_msec()
	running = true
	pending = false
	_set_status("simulating…")
	if mode == "thread":
		_cancel = {"cancel": false}
		_thread = Thread.new()
		# The thread gives the result of the main player with a deferred call. The main thread takes it.
		var serial: int = _serial
		var on_partial: Callable = func(partial: Dictionary) -> void: _take_partial.call_deferred(partial, serial)
		_thread.start(_thread_run.bind(ctx, k.duplicate(), target_changes.duplicate(true), _cancel, check, on_partial))
		return
	_make_work_dir()
	var setting_path: String = ProjectSettings.globalize_path("%s/setting_%d.json" % [WORK_DIR, _serial])
	_out_path = ProjectSettings.globalize_path("%s/result_%d.json" % [WORK_DIR, _serial])
	_partial_path = ProjectSettings.globalize_path("%s/partial_%d.json" % [WORK_DIR, _serial])
	BenchJson.write_file(setting_path, Knobs.setting(ctx, k, target_changes))
	var args: PackedStringArray = _cli_args(["--run", "--out", _out_path, "--partial-out", _partial_path, "--setting", setting_path])
	var check_path: String = ProjectSettings.globalize_path(WORK_DIR + "/selfcheck.json")
	if not check.is_empty() and FileAccess.file_exists(check_path):
		args.append_array(PackedStringArray(["--check", check_path]))
	_pid = OS.create_process(OS.get_executable_path(), args)
	if _pid <= 0:
		running = false
		_set_status("cannot start a second Godot process for the simulation")


static func _thread_run(run_ctx: Dictionary, run_k: Dictionary, run_targets: Dictionary, cancel: Dictionary, run_check: Dictionary, on_partial: Callable) -> Dictionary:
	return Runner.simulate(run_ctx, run_k, run_targets, {"cancel": cancel, "check": run_check, "on_partial": on_partial})


## Take the result of the main player while the other players still run. The panel shows it at once.
func _take_partial(partial: Dictionary, serial: int) -> void:
	if serial != _serial or not running:
		return
	result = partial
	pending = true
	_set_status("the main player ran in %.2f s; more players run…" % (float(Time.get_ticks_msec() - _started_ms) / 1000.0))
	result_changed.emit()


func _cli_args(tail: Array) -> PackedStringArray:
	var args: PackedStringArray = PackedStringArray(["--headless", "--path", ProjectSettings.globalize_path("res://"),
		"-s", CLI_SCRIPT, "--", "--bench", bench_path])
	for item: Variant in tail:
		args.append(str(item))
	return args


func _make_work_dir() -> void:
	DirAccess.make_dir_recursive_absolute(ProjectSettings.globalize_path(WORK_DIR))


func _cancel_run() -> void:
	if _thread != null:
		_cancel["cancel"] = true
		_old_threads.append(_thread)
		_thread = null
	if _pid > 0:
		if OS.is_process_running(_pid):
			OS.kill(_pid)
		_clean_work(_serial)
		_pid = -1
	running = false
	pending = false


func _poll_run() -> void:
	if _thread != null and not _thread.is_alive():
		var out: Dictionary = _thread.wait_to_finish()
		_thread = null
		_finish(out["result"] if out["ok"] else {}, str(out["error"]))
	elif _pid > 0:
		if not pending and FileAccess.file_exists(_partial_path) and not FileAccess.file_exists(_out_path):
			var early: Dictionary = BenchJson.read_file(_partial_path)
			if early["ok"] and early["value"] is Dictionary:
				_take_partial(early["value"], _serial)
		if FileAccess.file_exists(_out_path):
			var read: Dictionary = BenchJson.read_file(_out_path)
			_pid = -1
			_clean_work(_serial)
			if not read["ok"] or not read["value"] is Dictionary:
				_finish({}, "the result file of the simulation does not load")
			elif str((read["value"] as Dictionary).get("format", "")) == "tuning-bench-error":
				_finish({}, str((read["value"] as Dictionary).get("error", "error")))
			else:
				_finish(read["value"], "")
		elif not OS.is_process_running(_pid):
			_pid = -1
			_finish({}, "the simulation stopped with no result (see the output of Godot)")


func _clean_work(serial: int) -> void:
	for file: String in ["setting_%d.json" % serial, "result_%d.json" % serial, "partial_%d.json" % serial]:
		var path: String = ProjectSettings.globalize_path(WORK_DIR + "/" + file)
		if FileAccess.file_exists(path):
			DirAccess.remove_absolute(path)


func _finish(new_result: Dictionary, problem: String) -> void:
	running = false
	pending = false
	last_seconds = float(Time.get_ticks_msec() - _started_ms) / 1000.0
	if problem != "":
		_set_status("error: " + problem)
		return
	result = new_result
	var fails: Array = result.get("failed", [])
	var verdict: String = "no meter fails" if fails.is_empty() else "%d meters fail" % fails.size()
	_set_status("ran in %.2f s (%s); %s" % [last_seconds, mode, verdict])
	result_changed.emit()


## Start a tool in a second Godot process: "rank", "fit" (extra: the control id), "selfcheck" or "replay".
func start_task(kind: String, extra: Array = []) -> bool:
	if not enabled or _task_pid > 0:
		return false
	_make_work_dir()
	_task_kind = kind
	_task_out = ProjectSettings.globalize_path("%s/%s.json" % [WORK_DIR, kind])
	if FileAccess.file_exists(_task_out):
		DirAccess.remove_absolute(_task_out)
	var setting_path: String = ProjectSettings.globalize_path("%s/setting_task.json" % WORK_DIR)
	BenchJson.write_file(setting_path, Knobs.setting(ctx, k, target_changes))
	var tail: Array = ["--" + kind]
	tail.append_array(extra)
	if kind == "selfcheck":
		# The dead-number check runs 1 game for each number. Child processes do it in parallel.
		tail.append_array(["--jobs", clampi(OS.get_processor_count() - 1, 1, SELFCHECK_JOBS)])
	tail.append_array(["--out", _task_out, "--setting", setting_path])
	_task_pid = OS.create_process(OS.get_executable_path(), _cli_args(tail))
	_task_started_ms = Time.get_ticks_msec()
	if _task_pid <= 0:
		_set_status("cannot start a second Godot process for the tool")
		return false
	_set_status("the tool \"%s\" runs…" % kind)
	return true


## True while a tool runs.
func task_running() -> bool:
	return _task_pid > 0


func _poll_task() -> void:
	if _task_pid <= 0:
		return
	var payload: Dictionary = {}
	if FileAccess.file_exists(_task_out):
		var read: Dictionary = BenchJson.read_file(_task_out)
		if read["ok"] and read["value"] is Dictionary:
			payload = read["value"]
		else:
			payload = {"format": "tuning-bench-error", "error": "the result file of the tool does not load"}
	elif not OS.is_process_running(_task_pid):
		payload = {"format": "tuning-bench-error", "error": "the tool stopped with no result"}
	else:
		return
	_task_pid = -1
	var seconds: float = float(Time.get_ticks_msec() - _task_started_ms) / 1000.0
	var failed: bool = str(payload.get("format", "")) == "tuning-bench-error"
	if failed:
		_set_status("error: " + str(payload.get("error", "")))
	else:
		_set_status("the tool \"%s\" ran in %.1f s" % [_task_kind, seconds])
	task_done.emit(_task_kind, payload)
	if failed:
		return
	if _task_kind == "selfcheck":
		check = payload
		# The default place of the result is in the project: the next start and the command line read it there.
		if not OS.has_feature("template"):
			BenchJson.write_file(ProjectSettings.globalize_path(Knobs.DEFAULT_SELFCHECK), payload)
		request_run()
	elif _task_kind == "fit":
		var fitted: Dictionary = k.duplicate()
		fitted[str(payload["control"])] = float(payload["k"])
		use_k(fitted, target_changes)


## Return the folder of the saved settings. It is in the project when the project folder takes a file.
func settings_dir() -> String:
	var dir: String = ProjectSettings.globalize_path(SETTINGS_DIR)
	if not OS.has_feature("template") and DirAccess.make_dir_recursive_absolute(dir) == OK:
		return SETTINGS_DIR
	DirAccess.make_dir_recursive_absolute(ProjectSettings.globalize_path(WORK_DIR + "/settings"))
	return WORK_DIR + "/settings"


## Return the names of the saved settings.
func setting_names() -> PackedStringArray:
	var out: PackedStringArray = PackedStringArray()
	var dir: DirAccess = DirAccess.open(settings_dir())
	if dir == null:
		return out
	for file: String in dir.get_files():
		if file.ends_with(".json"):
			out.append(file.trim_suffix(".json"))
	out.sort()
	return out


## Save the values of now as a setting. Return the path, or "" when the write fails.
func save_setting(setting_name: String) -> String:
	var clean: String = setting_name.strip_edges().validate_filename().replace(" ", "_")
	if clean == "":
		clean = "setting"
	var path: String = "%s/%s.json" % [settings_dir(), clean]
	if not BenchJson.write_file(ProjectSettings.globalize_path(path), Knobs.setting(ctx, k, target_changes), false):
		_set_status("cannot write %s" % path)
		return ""
	_set_status("saved %s" % path)
	return path


## Load a saved setting and start a run.
func load_setting(setting_name: String) -> bool:
	var path: String = "%s/%s.json" % [settings_dir(), setting_name]
	var read: Dictionary = Knobs.read_setting(ctx, path)
	if not read["ok"]:
		_set_status("error: " + str(read["error"]))
		return false
	use_k(read["k"], read["targets"])
	return true


## Keep the values and the result of now as "A" or "B".
func store_slot(slot: String) -> void:
	slots[slot] = {"k": k.duplicate(), "targets": target_changes.duplicate(true), "result": result}


## Write each changed value into the data files of the game. Each file gets a backup copy (.bak) first.
## Return the list of the files that changed.
func write_to_data_files() -> PackedStringArray:
	var written: PackedStringArray = PackedStringArray()
	var vals: Dictionary = Knobs.values(ctx, k)
	var base: Dictionary = ctx["base"]
	var files: Dictionary = {}
	for id: Variant in vals:
		if vals[id] != base[id] or typeof(vals[id]) != typeof(base[id]):
			files[str((ctx["numbers"][id] as Dictionary)["file"])] = true
	if files.is_empty():
		_set_status("no number changed; no file to write")
		return written
	var new_data: Dictionary = Knobs.apply(ctx, vals)
	for file: Variant in files:
		var path: String = ProjectSettings.globalize_path(str(file))
		if DirAccess.copy_absolute(path, path + ".bak") != OK or not BenchJson.write_file(path, new_data[file], false):
			_set_status("error: cannot write %s" % file)
			return written
		written.append(str(file))
	# The new values are the base now. The player controls keep their values.
	load_bench()
	k_changed.emit()
	_apply_live()
	request_run()
	_set_status("wrote %d files (backup: .bak). Run the inventory again to update the ledger" % written.size())
	return written


func _exit_tree() -> void:
	_cancel_run()
	for thread: Thread in _old_threads:
		thread.wait_to_finish()
	_old_threads.clear()
