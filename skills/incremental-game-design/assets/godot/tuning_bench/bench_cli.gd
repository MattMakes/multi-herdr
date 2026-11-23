extends SceneTree

const BenchJson := preload("res://addons/tuning_bench/bench_json.gd")
const Knobs := preload("res://addons/tuning_bench/bench_knobs.gd")
const Runner := preload("res://addons/tuning_bench/bench_runner.gd")
const SelfCheck := preload("res://addons/tuning_bench/bench_selfcheck.gd")

const MODES: Array[String] = ["--run", "--selfcheck", "--rank", "--fit", "--replay", "--probe", "--view-texts"]
const REPLAY_CHECKPOINTS := 19
const DEFAULT_FRAMES := 3
const LATER := -1

var _started: bool = false
var _out: String = ""


func _process(_delta: float) -> bool:
	if _started:
		return false
	_started = true
	var code: int = _main(OS.get_cmdline_user_args())
	if code == LATER:
		return false
	quit(code)
	return true


func _fail(message: String) -> int:
	printerr("tuning bench: " + message)
	if _out != "":
		BenchJson.write_file(_out, {"format": "tuning-bench-error", "version": 1, "error": message})
	return 2


func _main(args: PackedStringArray) -> int:
	var mode: String = ""
	var bench_path: String = Knobs.DEFAULT_BENCH
	var setting_path: String = ""
	var check_path: String = ""
	var player: String = ""
	var fit_control: String = ""
	var target_h: float = -1.0
	var child: bool = true
	var read_check: bool = true
	var partial_out: String = ""
	var only: Array = []
	var jobs: int = 0
	var part: String = ""
	var profile: bool = false
	var against: String = ""
	var scene_path: String = ""
	var states: Array = []
	var frames: int = DEFAULT_FRAMES
	var visible_only: bool = false
	var k_args: Array = []
	var set_args: Array = []
	var at: int = 0
	while at < args.size():
		var arg: String = args[at]
		var value: String = args[at + 1] if at + 1 < args.size() else ""
		if MODES.has(arg):
			if mode != "":
				return _fail("give 1 mode only: %s and %s are 2 modes" % [mode, arg])
			mode = arg
			if arg == "--fit":
				fit_control = value
				at += 1
		elif arg == "--no-child":
			child = false
		elif arg == "--no-check":
			read_check = false
		elif arg == "--profile":
			profile = true
		elif arg == "--visible-only":
			visible_only = true
		elif ["--out", "--bench", "--setting", "--k", "--set", "--player", "--check", "--target-h", "--only", "--jobs",
				"--part", "--partial-out", "--against", "--scene", "--state", "--frames"].has(arg):
			if at + 1 >= args.size():
				return _fail("the option %s needs a value" % arg)
			at += 1
			match arg:
				"--out":
					_out = value
				"--bench":
					bench_path = value
				"--setting":
					setting_path = value
				"--k":
					k_args.append(value)
				"--set":
					set_args.append(value)
				"--player":
					player = value
				"--check":
					check_path = value
				"--target-h":
					target_h = value.to_float()
				"--only":
					only.append(value)
				"--jobs":
					if not value.is_valid_int() or value.to_int() < 1:
						return _fail("--jobs needs a count of 1 or more, not \"%s\"" % value)
					jobs = value.to_int()
				"--part":
					part = value
				"--partial-out":
					partial_out = value
				"--against":
					against = value
				"--scene":
					scene_path = value
				"--state":
					states.append(value)
				"--frames":
					if not value.is_valid_int() or value.to_int() < 1:
						return _fail("--frames needs a count of 1 or more, not \"%s\"" % value)
					frames = value.to_int()
		else:
			return _fail("the option %s is not known" % arg)
		at += 1
	if mode == "":
		return _fail("give a mode: --run, --selfcheck, --rank, --fit <control>, --replay or --view-texts")
	if _out == "" and mode == "--selfcheck":
		_out = Knobs.DEFAULT_SELFCHECK
	if _out == "":
		return _fail("give the result file: --out <file>")

	var started: int = Time.get_ticks_msec()
	var ctx: Dictionary = Knobs.build(bench_path)
	if not ctx["ok"]:
		return _fail(ctx["error"])

	if jobs == 0:
		jobs = SelfCheck.default_jobs() if child else 1

	if not only.is_empty() and SelfCheck.select_numbers(ctx, only).is_empty():
		return _fail("--only %s matches no tunable number of the bench file (a glob has * and ? only)" % " ".join(PackedStringArray(only)))

	var k: Dictionary = {}
	var target_changes: Dictionary = {}
	if setting_path != "":
		var setting: Dictionary = Knobs.read_setting(ctx, setting_path)
		if not setting["ok"]:
			return _fail(setting["error"])
		k = setting["k"]
		target_changes = setting["targets"]
	for pair: String in k_args:
		var cut: int = pair.rfind("=")
		if cut <= 0 or not pair.substr(cut + 1).is_valid_float():
			return _fail("--k needs <control>=<number>, not \"%s\"" % pair)
		var id: String = pair.substr(0, cut)
		if not (ctx["by_id"] as Dictionary).has(id):
			return _fail("the control %s is not in the bench file" % id)
		k[id] = pair.substr(cut + 1).to_float()
	for pair: String in set_args:
		var cut: int = pair.rfind("=")
		if cut <= 0 or not pair.substr(cut + 1).is_valid_float():
			return _fail("--set needs <number id>=<number>, not \"%s\"" % pair)
		var control_id: String = Knobs.control_of_number(ctx, pair.substr(0, cut))
		if control_id == "":
			return _fail("the number %s is not a tunable balance number of the bench file" % pair.substr(0, cut))
		k[control_id] = pair.substr(cut + 1).to_float()

	var check: Dictionary = {}
	if check_path != "":
		var read: Dictionary = BenchJson.read_file(check_path)
		if not read["ok"] or not read["value"] is Dictionary:
			return _fail("the self-check result %s does not load" % check_path)
		check = read["value"]
	elif read_check and mode == "--run" and FileAccess.file_exists(Knobs.DEFAULT_SELFCHECK):
		var found: Dictionary = BenchJson.read_file(Knobs.DEFAULT_SELFCHECK)
		if found["ok"] and found["value"] is Dictionary and str((found["value"] as Dictionary).get("format", "")) == "tuning-bench-check":
			check = found["value"]
		else:
			print("tuning bench: %s does not load; meter 8 does not use it" % Knobs.DEFAULT_SELFCHECK)

	var code: int = 0
	match mode:
		"--run":
			code = _run(ctx, k, target_changes, player, check, partial_out, profile)
		"--selfcheck":
			code = _selfcheck(ctx, child, only, jobs)
		"--probe":
			code = _probe(ctx, only, part)
		"--rank":
			code = _rank(ctx, k, target_changes)
		"--fit":
			code = _fit(ctx, k, target_changes, fit_control, target_h)
		"--replay":
			if against != "":
				code = _replay_against(ctx, k, against, setting_path != "" or not k_args.is_empty() or not set_args.is_empty())
			else:
				code = _replay(ctx, k)
		"--view-texts":
			return _view_texts(ctx, k, scene_path, states, frames, visible_only)
	print("tuning bench: %s took %.2f s" % [mode, float(Time.get_ticks_msec() - started) / 1000.0])
	return code


func _run(ctx: Dictionary, k: Dictionary, target_changes: Dictionary, player: String, check: Dictionary, partial_out: String, profile: bool = false) -> int:
	var opts: Dictionary = {"player": player, "check": check}
	var spent: Array = []
	if profile:
		opts["profile"] = spent
	if partial_out != "":
		opts["on_partial"] = func(partial: Dictionary) -> void: BenchJson.write_file(partial_out, partial)
	var out: Dictionary = Runner.simulate(ctx, k, target_changes, opts)
	if not out["ok"]:
		return _fail(out["error"])
	var result: Dictionary = out["result"]
	if not BenchJson.write_file(_out, result):
		return _fail("cannot write %s" % _out)
	for meter: Dictionary in result["meters"]:
		print("%s %-24s %-16s %s" % [meter["id"], meter["name"], meter["status"], meter["text"]])
		for line: Variant in meter.get("lines", []):
			print("%s %s" % [" ".repeat(44), line])
	for entry: Dictionary in spent:
		var title: String = "of the %s player (%d steps, %d decisions, %d looks at the actions)" % [entry["player"], entry["steps"], entry["decisions"], entry["scans"]]
		for line: String in (load(Runner.PROFILE_ADAPTER) as GDScript).call("lines", title, entry["spent"], entry["usec"]):
			print(line)
	return 1 if not (result["failed"] as Array).is_empty() else 0


func _selfcheck(ctx: Dictionary, child: bool, only: Array, jobs: int) -> int:
	var out: Dictionary = SelfCheck.run_check(ctx, {"child": child, "print": true, "only": only, "jobs": jobs})
	if not out["ok"]:
		return _fail(out["error"])
	var result: Dictionary = out["result"]
	if not only.is_empty() and FileAccess.file_exists(_out):
		var old: Dictionary = BenchJson.read_file(_out)
		if old["ok"] and old["value"] is Dictionary:
			result = SelfCheck.merge(ctx, old["value"], result)
	if not BenchJson.write_file(_out, result):
		return _fail("cannot write %s" % _out)
	for check: Dictionary in result["checks"]:
		print("%-16s %s" % [check["name"], "ok" if check["ok"] else "FAIL"])
		for line: Variant in check["detail"]:
			print("    " + str(line))
	for item: Dictionary in result["dead"]:
		if item.has("dead_ok"):
			print("    no effect, accepted: %s (%s)" % [item["id"], item["dead_ok"]])
	if result["partial"]:
		print("partial: the dead-number check tried %d of %d tunable numbers" % [(result["checked"] as Array).size(), (ctx["tunable"] as Array).size()])
	print("result: %s" % _out)
	return 0 if result["ok"] else 1


func _probe(ctx: Dictionary, only: Array, part: String) -> int:
	var pieces: PackedStringArray = part.split("/")
	if pieces.size() != 2 or not pieces[0].is_valid_int() or not pieces[1].is_valid_int() or pieces[1].to_int() < 1:
		return _fail("--probe needs --part <index>/<count>")
	var out: Dictionary = SelfCheck.run_probe_part(ctx, only, pieces[0].to_int(), pieces[1].to_int(), _out + ".progress")
	if not out["ok"]:
		return _fail(out["error"])
	if not BenchJson.write_file(_out, out["result"]):
		return _fail("cannot write %s" % _out)
	return 0


static func _meter_values(result: Dictionary) -> Dictionary:
	var out: Dictionary = {}
	for meter: Dictionary in result["meters"]:
		out[meter["id"]] = meter["value"]
	return out


static func _meter_status(result: Dictionary) -> Dictionary:
	var out: Dictionary = {}
	for meter: Dictionary in result["meters"]:
		out[meter["id"]] = meter["status"]
	return out


func _rank(ctx: Dictionary, k: Dictionary, target_changes: Dictionary) -> int:
	var made: Dictionary = Runner.make_adapter(ctx)
	if not made["ok"]:
		return _fail("adapter error: " + str(made["error"]))
	var base_out: Dictionary = Runner.simulate(ctx, k, target_changes, {"adapter_made": made})
	if not base_out["ok"]:
		return _fail(base_out["error"])
	var base_values: Dictionary = _meter_values(base_out["result"])
	var rows: Array = []
	for control: Dictionary in ctx["controls"]:
		if control["generated"]:
			continue
		if control["kind"] == "numbers" and (control["targets"] as Array).is_empty():
			continue
		var now: float = float(k.get(control["id"], control["base"]))
		var moved: float = now * 1.25
		if control["op"] == "add":
			moved = now + 0.25
		elif now == 0.0:
			moved = 0.25
		moved = clampf(moved, float(control["min"]), float(control["max"]))
		var trial_k: Dictionary = k.duplicate()
		trial_k[control["id"]] = moved
		var out: Dictionary = Runner.simulate(ctx, trial_k, target_changes, {"adapter_made": made})
		if not out["ok"]:
			return _fail(out["error"])
		var values: Dictionary = _meter_values(out["result"])
		var change: Dictionary = {}
		var size: float = 0.0
		for id: Variant in values:
			var before: float = float(base_values[id])
			var rel: float = 0.0
			if before != 0.0:
				rel = (float(values[id]) - before) / absf(before)
			elif float(values[id]) != 0.0:
				rel = 1.0
			change[id] = BenchJson.sig(rel)
			size += absf(rel)
		rows.append({"id": control["id"], "name": control["name"], "group": control["group"], "k": moved,
			"values": values, "change": change, "status": _meter_status(out["result"]), "size": BenchJson.sig(size)})
	rows.sort_custom(func(a: Dictionary, b: Dictionary) -> bool:
		if float(a["size"]) != float(b["size"]):
			return float(a["size"]) > float(b["size"])
		return str(a["id"]) < str(b["id"]))
	var result: Dictionary = {"format": "tuning-bench-rank", "version": 1, "move": 0.25, "base": base_values,
		"base_status": _meter_status(base_out["result"]), "controls": rows}
	if not BenchJson.write_file(_out, result):
		return _fail("cannot write %s" % _out)
	for row: Dictionary in rows:
		print("%-22s M1 %+6.1f%%  total %.2f" % [row["id"], float(row["change"]["M1"]) * 100.0, row["size"]])
	return 0


func _fit(ctx: Dictionary, k: Dictionary, target_changes: Dictionary, control_id: String, target_h: float) -> int:
	if not (ctx["by_id"] as Dictionary).has(control_id):
		return _fail("--fit needs a control of the bench file, not \"%s\"" % control_id)
	var control: Dictionary = ctx["by_id"][control_id]
	var targets: Dictionary = Knobs.targets(ctx, target_changes)
	if target_h <= 0.0:
		var length: Variant = targets.get("length_h", {})
		if length is Dictionary and (length as Dictionary).has("about"):
			target_h = float((length as Dictionary)["about"])
		else:
			return _fail("the bench file has no target length_h.about; give --target-h <hours>")
	var made: Dictionary = Runner.make_adapter(ctx)
	if not made["ok"]:
		return _fail("adapter error: " + str(made["error"]))
	var main_id: String = str(SelfCheck._main_player(ctx)["id"])
	var steps: Array = []
	var measure: Callable = func(value: float) -> float:
		var trial_k: Dictionary = k.duplicate()
		trial_k[control_id] = value
		var out: Dictionary = Runner.simulate(ctx, trial_k, target_changes, {"adapter_made": made, "player": main_id})
		if not out["ok"]:
			return -1.0
		var info: Dictionary = out["result"]["players"][main_id]
		var hours: float = float(info["length_s"]) / 3600.0
		if not info["ended"]:
			hours = Runner._sim(ctx, "max_hours", 18.0) * 2.0
		steps.append([BenchJson.sig(value), BenchJson.sig(hours)])
		return hours
	var low: float = float(control["min"])
	var high: float = float(control["max"])
	var log_scale: bool = low > 0.0 and control["op"] != "add"
	var at_low: float = measure.call(low)
	var at_high: float = measure.call(high)
	if at_low < 0.0 or at_high < 0.0:
		return _fail("adapter error in the fit")
	var best_k: float = low
	var best_h: float = at_low
	var reached: bool = false
	if (at_low - target_h) * (at_high - target_h) > 0.0:
		if absf(at_high - target_h) < absf(at_low - target_h):
			best_k = high
			best_h = at_high
	else:
		for round: int in range(18):
			var mid: float = sqrt(low * high) if log_scale else (low + high) * 0.5
			var at_mid: float = measure.call(mid)
			if at_mid < 0.0:
				return _fail("adapter error in the fit")
			if absf(at_mid - target_h) < absf(best_h - target_h):
				best_k = mid
				best_h = at_mid
			if absf(at_mid - target_h) <= target_h * 0.01:
				break
			if (at_mid - target_h) * (at_low - target_h) > 0.0:
				low = mid
				at_low = at_mid
			else:
				high = mid
				at_high = at_mid
	reached = absf(best_h - target_h) <= target_h * 0.05
	var result: Dictionary = {"format": "tuning-bench-fit", "version": 1, "control": control_id, "k": BenchJson.sig(best_k),
		"length_h": BenchJson.sig(best_h), "target_h": target_h, "reached": reached, "steps": steps}
	if not BenchJson.write_file(_out, result):
		return _fail("cannot write %s" % _out)
	print("fit %s: k = %s gives %.2f h (target %.2f h)%s" % [control_id, result["k"], best_h, target_h, "" if reached else "; the control does not reach the target"])
	return 0 if reached else 1


func _replay(ctx: Dictionary, k: Dictionary) -> int:
	var made: Dictionary = Runner.make_adapter(ctx)
	if not made["ok"]:
		return _fail("adapter error: " + str(made["error"]))
	var adapter: RefCounted = made["adapter"]
	if not adapter.has_method("state_hash"):
		return _fail("--replay needs a game with a core (the key \"core\" in bench.json and no key \"adapter\")")
	var data: Dictionary = Knobs.data_for(ctx, k)
	var player: Dictionary = {}
	var main_id: String = str(SelfCheck._main_player(ctx)["id"])
	for entry: Dictionary in Knobs.players(ctx, k):
		if str(entry.get("id", "")) == main_id:
			player = entry
	var first: Dictionary = Runner.run(adapter, ctx, data, player, {})
	if not (first["errors"] as Array).is_empty():
		return _fail("adapter error: " + "; ".join(PackedStringArray(first["errors"])))
	var hash_a: String = adapter.call("state_hash")
	var record: Array = first["record"]
	var marks: Array = _checkpoint_times(float(first["length_s"]))
	var second: Dictionary = Runner.run(adapter, ctx, data, player, {"replay": record, "replay_until": first["length_s"], "checkpoints": marks})
	var hash_b: String = adapter.call("state_hash")
	var same: bool = hash_a == hash_b and float(second["length_s"]) == float(first["length_s"])
	var result: Dictionary = {"format": "tuning-bench-replay", "version": 1, "ok": same, "player": main_id,
		"game": str((ctx["bench"] as Dictionary).get("game", "")),
		"actions": record.size(), "length_s": first["length_s"], "replay_length_s": second["length_s"],
		"hash": hash_a, "replay_hash": hash_b, "refused": second["refused"],
		"commands": record, "checkpoints": second["checkpoints"], "seed": adapter.get("bench_seed"),
		"decision_s": first["decision_s"], "active_share": first["active_share"], "k": Knobs.changed_k(ctx, k)}
	if not BenchJson.write_file(_out, result):
		return _fail("cannot write %s" % _out)
	print("replay of %d actions: %s" % [record.size(), "the 2 final states are equal" if same else "the 2 final states DIFFER"])
	return 0 if same else 1


static func _checkpoint_times(length_s: float) -> Array:
	var out: Array = []
	for index: int in range(1, REPLAY_CHECKPOINTS + 1):
		out.append(BenchJson.ms(length_s * float(index) / float(REPLAY_CHECKPOINTS + 1)))
	return out


func _replay_against(ctx: Dictionary, k: Dictionary, record_path: String, own_k: bool) -> int:
	var read: Dictionary = BenchJson.read_file(record_path)
	if not read["ok"] or not read["value"] is Dictionary:
		return _fail("the replay record %s does not load" % record_path)
	var record: Dictionary = read["value"]
	if str(record.get("format", "")) != "tuning-bench-replay" or not record.get("commands", null) is Array or not record.has("hash"):
		return _fail("%s is not a replay record with the key \"commands\" (write it with --replay --out <file>)" % record_path)
	var made: Dictionary = Runner.make_adapter(ctx)
	if not made["ok"]:
		return _fail("adapter error: " + str(made["error"]))
	var adapter: RefCounted = made["adapter"]
	if not adapter.has_method("state_hash"):
		return _fail("--replay needs a game with a core (the key \"core\" in bench.json and no key \"adapter\")")
	if not own_k and record.get("k", null) is Dictionary:
		for id: Variant in record["k"]:
			if not (ctx["by_id"] as Dictionary).has(id):
				return _fail("the control %s of the replay record is not in the bench file; give the values with --k or --set" % id)
			k[id] = float(record["k"][id])
	adapter.set("bench_seed", int(record.get("seed", adapter.get("bench_seed"))))
	var data: Dictionary = Knobs.data_for(ctx, k)
	var player: Dictionary = {"id": str(record.get("player", "active")), "decision_s": float(record.get("decision_s", 2.0)),
		"active_share": float(record.get("active_share", 0.0))}
	var marks: Array = []
	var old_marks: Array = record.get("checkpoints", []) if record.get("checkpoints", null) is Array else []
	for mark: Variant in old_marks:
		marks.append(float((mark as Array)[0]))
	var commands: Array = record["commands"]
	var trace: Dictionary = Runner.run(adapter, ctx, data, player, {"replay": commands, "replay_until": float(record.get("length_s", 0.0)), "checkpoints": marks})
	if not (trace["errors"] as Array).is_empty():
		return _fail("adapter error: " + "; ".join(PackedStringArray(trace["errors"])))
	var hash_now: String = adapter.call("state_hash")
	var same: bool = hash_now == str(record["hash"])
	var first_difference: Variant = null
	var new_marks: Array = trace["checkpoints"]
	var before_s: float = 0.0
	for index: int in range(old_marks.size()):
		var at_s: float = float((old_marks[index] as Array)[0])
		if index >= new_marks.size() or str((new_marks[index] as Array)[1]) != str((old_marks[index] as Array)[1]):
			first_difference = {"after_s": before_s, "at_s": at_s}
			break
		before_s = at_s
	if first_difference == null and not same:
		first_difference = {"after_s": before_s, "at_s": float(trace["length_s"])}
	var result: Dictionary = {"format": "tuning-bench-replay-check", "version": 1, "ok": same, "record": record_path,
		"player": player["id"], "actions": commands.size(), "refused": trace["refused"], "hash": hash_now,
		"record_hash": str(record["hash"]), "length_s": trace["length_s"], "record_length_s": record.get("length_s", 0.0),
		"first_difference": first_difference}
	if not BenchJson.write_file(_out, result):
		return _fail("cannot write %s" % _out)
	if same:
		print("replay of %d commands of %s: the end state is equal to the record" % [commands.size(), record_path])
	else:
		print("replay of %d commands of %s: the end state DIFFERS from the record" % [commands.size(), record_path])
		print("    the first difference is after %s s and at or before %s s of game time; the core of now refused %d commands" % [
			first_difference["after_s"], first_difference["at_s"], int(trace["refused"])])
	return 0 if same else 1


func _view_texts(ctx: Dictionary, k: Dictionary, scene_path: String, states: Array, frames: int, visible_only: bool) -> int:
	if scene_path == "":
		scene_path = str(ProjectSettings.get_setting("application/run/main_scene", ""))
	if scene_path == "" or not ResourceLoader.exists(scene_path):
		return _fail("--view-texts needs a scene: the project has no main scene, or \"%s\" does not exist (give --scene <res path>)" % scene_path)
	var packed: Variant = load(scene_path)
	if not packed is PackedScene:
		return _fail("%s is not a scene" % scene_path)
	if states.is_empty():
		states = ["start"]
	var session_name: String = str((ctx["bench"] as Dictionary).get("session", "Session"))
	var session: Node = root.get_node_or_null(NodePath(session_name))
	var made_states: Array = []
	for entry: Variant in states:
		var name: String = str(entry)
		if name == "start":
			if not made_states.is_empty():
				return _fail("give --state start before each other state: it is the state of the session at the start of the run")
			made_states.append({"name": name, "state": null})
			continue
		if session == null or not session.has_method("use_state"):
			return _fail("--state %s needs the session autoload \"%s\" with the method use_state(state: Dictionary)" % [name, session_name])
		if name.begins_with("bot:"):
			if not name.substr(4).is_valid_float() or name.substr(4).to_float() < 0.0:
				return _fail("--state bot:<hours> needs a number of hours, not \"%s\"" % name)
			var got: Dictionary = _bot_state(ctx, k, name.substr(4).to_float() * 3600.0)
			if not got["ok"]:
				return _fail(got["error"])
			made_states.append({"name": name, "state": got["state"]})
		else:
			var read: Dictionary = BenchJson.read_file(name)
			if not read["ok"] or not read["value"] is Dictionary:
				return _fail("the state file %s does not load as a JSON object" % name)
			made_states.append({"name": name.get_file(), "state": read["value"]})
	if DisplayServer.get_name() != "headless":
		print("tuning bench: --view-texts runs with a window. The session of the game must not load or write the save file in this run")
	_view_texts_run(packed, scene_path, session, made_states, frames, visible_only, str((ctx["bench"] as Dictionary).get("game", "")))
	return LATER


func _bot_state(ctx: Dictionary, k: Dictionary, seconds: float) -> Dictionary:
	var made: Dictionary = Runner.make_adapter(ctx)
	if not made["ok"]:
		return {"ok": false, "error": "adapter error: " + str(made["error"]), "state": {}}
	var adapter: RefCounted = made["adapter"]
	if not adapter.has_method("state_copy"):
		return {"ok": false, "error": "--state bot:<hours> needs a game with a core (the key \"core\" in bench.json)", "state": {}}
	var player: Dictionary = {}
	var main_id: String = str(SelfCheck._main_player(ctx)["id"])
	for entry: Dictionary in Knobs.players(ctx, k):
		if str(entry.get("id", "")) == main_id:
			player = entry
	var trace: Dictionary = Runner.run(adapter, ctx, Knobs.data_for(ctx, k), player, {"max_s": seconds})
	if not (trace["errors"] as Array).is_empty():
		return {"ok": false, "error": "adapter error: " + "; ".join(PackedStringArray(trace["errors"])), "state": {}}
	return {"ok": true, "error": "", "state": adapter.call("state_copy")}


func _view_texts_run(packed: PackedScene, scene_path: String, session: Node, states: Array, frames: int, visible_only: bool, game: String) -> void:
	var started: int = Time.get_ticks_msec()
	var files_before: Dictionary = SelfCheck.list_user_files()
	var lines: PackedStringArray = PackedStringArray()
	lines.append("# tuning bench view texts: %s, scene %s" % [game, scene_path])
	lines.append("# <node path> TAB <property> TAB <text as JSON> [TAB hidden]")
	var count: int = 0
	for entry: Dictionary in states:
		if entry["state"] != null:
			session.call("use_state", entry["state"])
		var view: Node = packed.instantiate()
		root.add_child(view)
		current_scene = view
		for frame: int in range(frames):
			await process_frame
		var found: Array = []
		_collect_texts(view, str(view.name), found, visible_only)
		found.sort()
		lines.append("== state %s" % entry["name"])
		for line: Variant in found:
			lines.append(str(line))
		count += found.size()
		current_scene = null
		root.remove_child(view)
		view.free()
		await process_frame
	var file: FileAccess = FileAccess.open(_out, FileAccess.WRITE)
	if file == null:
		quit(_fail("cannot write %s" % _out))
		return
	file.store_string("\n".join(lines) + "\n")
	file.close()
	print("view texts: %d texts in %d states of %s; result: %s" % [count, states.size(), scene_path, _out])
	var written: Array = SelfCheck.changed_files(files_before, SelfCheck.list_user_files())
	for line: Variant in written:
		printerr("tuning bench: " + str(line).replace("the simulation", "the run of --view-texts"))
	print("tuning bench: --view-texts took %.2f s" % (float(Time.get_ticks_msec() - started) / 1000.0))
	quit(1 if not written.is_empty() else 0)


static func _path_name(node: Node) -> String:
	var name: String = str(node.name)
	if name.contains("@"):
		return "%s#%03d" % [node.get_class(), node.get_index()]
	return name


static func _text_line(path: String, property: String, text: String, hidden: bool) -> String:
	return "%s\t%s\t%s%s" % [path, property, JSON.stringify(text), "\thidden" if hidden else ""]


static func _collect_texts(node: Node, path: String, out: Array, visible_only: bool) -> void:
	var hidden: bool = false
	if node is CanvasItem:
		hidden = not (node as CanvasItem).is_visible_in_tree()
	elif node is Window:
		hidden = not (node as Window).visible
	elif node is Node3D:
		hidden = not (node as Node3D).is_visible_in_tree()
	if not (hidden and visible_only):
		for property: String in ["text", "tooltip_text", "placeholder_text", "title", "dialog_text"]:
			if property in node and node.get(property) is String and str(node.get(property)) != "":
				out.append(_text_line(path, property, str(node.get(property)), hidden))
		if node is ItemList:
			for index: int in range((node as ItemList).item_count):
				out.append(_text_line(path, "item[%03d]" % index, (node as ItemList).get_item_text(index), hidden))
		elif node is OptionButton:
			for index: int in range((node as OptionButton).item_count):
				out.append(_text_line(path, "item[%03d]" % index, (node as OptionButton).get_item_text(index), hidden))
		elif node is PopupMenu:
			for index: int in range((node as PopupMenu).item_count):
				out.append(_text_line(path, "item[%03d]" % index, (node as PopupMenu).get_item_text(index), hidden))
		elif node is TabContainer:
			for index: int in range((node as TabContainer).get_tab_count()):
				out.append(_text_line(path, "tab[%03d]" % index, (node as TabContainer).get_tab_title(index), hidden))
		elif node is TabBar:
			for index: int in range((node as TabBar).tab_count):
				out.append(_text_line(path, "tab[%03d]" % index, (node as TabBar).get_tab_title(index), hidden))
		elif node is Tree:
			var row: int = 0
			var item: TreeItem = (node as Tree).get_root()
			while item != null:
				for column: int in range((node as Tree).columns):
					if item.get_text(column) != "":
						out.append(_text_line(path, "cell[%04d,%d]" % [row, column], item.get_text(column), hidden))
				row += 1
				item = item.get_next_in_tree()
	for child: Node in node.get_children():
		_collect_texts(child, path + "/" + _path_name(child), out, visible_only)
