extends RefCounted
## The self-check of the tuning bench. It proves that the bench tells the truth about the game.
##
## The checks: repeat, clean start, dead number, unknown source, unclassified, contract, no file written.

const BenchJson := preload("res://addons/tuning_bench/bench_json.gd")
const Knobs := preload("res://addons/tuning_bench/bench_knobs.gd")
const Runner := preload("res://addons/tuning_bench/bench_runner.gd")
const Meters := preload("res://addons/tuning_bench/bench_meters.gd")

const CLI_SCRIPT := "res://addons/tuning_bench/bench_cli.gd"
## The bench prints 1 line of progress for each count of numbers.
const PROGRESS_EVERY := 10
## The folder of the bench in `user://`. The check "No file written" does not look at it.
const OWN_DIR := "user://tuning_bench/"
## A file above this size is compared by its size and its time only, not by its content.
const HASH_LIMIT := 4 * 1024 * 1024
const POLL_MS := 50
const NOT_CHECKED := "not checked: the repeat check or the clean start check failed"
## The largest count of child processes that the dead-number check starts by itself.
const MAX_DEFAULT_JOBS := 4
## With no progress line for this time, the check prints a line "still running".
const HEARTBEAT_MS := 30000


## Return the count of processes of the dead-number check when the caller gives none:
## min(4, count of processors - 1), and at least 1.
static func default_jobs() -> int:
	return clampi(OS.get_processor_count() - 1, 1, MAX_DEFAULT_JOBS)


static func _main_player(ctx: Dictionary) -> Dictionary:
	var players: Array = Knobs.players(ctx, {})
	for player: Dictionary in players:
		if str(player.get("id", "")) == "active":
			return player
	return players[0]


## Return the value that the dead-number check gives to a number: the value x 1.5.
## A growth role changes only the part above 1. An integer stays an integer and always changes.
static func moved_value(number: Dictionary, base: Variant) -> Variant:
	var value: float = float(base)
	var moved: float = value * 1.5
	if Knobs.is_growth_role(str(number.get("role", ""))) and value > 1.0:
		moved = 1.0 + (value - 1.0) * 1.5
	if value == 0.0:
		moved = 1.0
	if base is int:
		var whole: int = roundi(moved)
		if whole == int(base):
			whole += 1
		return whole
	return moved


## Return the tunable numbers that the globs of `only` select, in the order of the ledger.
## A glob is a pattern on the number id with `*` and `?`. An empty list selects each number.
static func select_numbers(ctx: Dictionary, only: Array) -> Array:
	var out: Array = []
	for id: Variant in ctx["tunable"]:
		if only.is_empty():
			out.append(id)
			continue
		for glob: Variant in only:
			if str(id).match(str(glob)):
				out.append(id)
				break
	return out


## Return each file below `user://`: path -> [size, modified time, MD5 of the content].
## The folder of the bench and the folder of the engine log are not in the list.
static func list_user_files() -> Dictionary:
	var skip: Array = [OWN_DIR]
	var log_path: String = str(ProjectSettings.get_setting("debug/file_logging/log_path", "user://logs/godot.log"))
	if log_path.begins_with("user://"):
		skip.append(log_path.get_base_dir().trim_suffix("/") + "/")
	var out: Dictionary = {}
	_list_dir("user://", skip, out)
	return out


static func _list_dir(folder: String, skip: Array, out: Dictionary) -> void:
	var dir: DirAccess = DirAccess.open(folder)
	if dir == null:
		return
	dir.include_hidden = true
	for name: String in dir.get_directories():
		var sub: String = folder.path_join(name) + "/"
		if not skip.has(sub):
			_list_dir(sub, skip, out)
	for name: String in dir.get_files():
		var path: String = folder.path_join(name)
		var size: int = 0
		var file: FileAccess = FileAccess.open(path, FileAccess.READ)
		if file != null:
			size = file.get_length()
			file.close()
		var content: String = FileAccess.get_md5(path) if size <= HASH_LIMIT else ""
		out[path] = [size, FileAccess.get_modified_time(path), content]


## Return 1 line for each file that is new, changed or gone between 2 lists of `list_user_files`.
static func changed_files(before: Dictionary, after: Dictionary) -> Array:
	var out: Array = []
	var paths: Array = after.keys()
	paths.sort()
	for path: Variant in paths:
		if not before.has(path):
			out.append("%s: the simulation made this file (%d bytes)" % [path, int((after[path] as Array)[0])])
		elif before[path] != after[path]:
			out.append("%s: the simulation changed this file (%d bytes before, %d bytes after)" % [path,
				int((before[path] as Array)[0]), int((after[path] as Array)[0])])
	var gone: Array = before.keys()
	gone.sort()
	for path: Variant in gone:
		if not after.has(path):
			out.append("%s: the simulation removed this file" % path)
	return out


## Try each number of `ids`: a run with the moved value must differ from the reference run.
## Return the list of the numbers with no effect. `on_done` is called 1 time after each number.
## A number with the role `offline` that changes nothing for the main player is tried again with each player
## that goes away (`away_every_min` and `away_min` above 0).
static func probe(ctx: Dictionary, adapter: RefCounted, player: Dictionary, reference: Array, ids: Array, on_done: Callable = Callable()) -> Array:
	var numbers: Dictionary = ctx["numbers"]
	var base: Dictionary = ctx["base"]
	var base_data: Dictionary = Knobs.data_for(ctx, {})
	var away_players: Array = []
	for other: Dictionary in Knobs.players(ctx, {}):
		if str(other.get("id", "")) != str(player.get("id", "")) and float(other.get("away_every_min", 0.0)) > 0.0 and float(other.get("away_min", 0.0)) > 0.0:
			away_players.append(other)
	var away_reference: Dictionary = {}
	var dead: Array = []
	for id: Variant in ids:
		var number: Dictionary = numbers[id]
		var vals: Dictionary = base.duplicate()
		vals[id] = moved_value(number, base[id])
		var moved_data: Dictionary = Knobs.apply(ctx, vals)
		var alive: bool = Runner.run(adapter, ctx, moved_data, player, {"reference": reference})["diverged"]
		if not alive and str(number.get("role", "")) == "offline":
			for other: Dictionary in away_players:
				var other_id: String = str(other.get("id", ""))
				if not away_reference.has(other_id):
					away_reference[other_id] = Runner.run(adapter, ctx, base_data, other, {})["log"]
				if Runner.run(adapter, ctx, moved_data, other, {"reference": away_reference[other_id]})["diverged"]:
					alive = true
					break
		if not alive:
			var item: Dictionary = {"id": id, "label": str(number.get("label", id)), "value": base[id], "tried": vals[id]}
			if number.has("dead_ok"):
				item["dead_ok"] = str(number["dead_ok"])
			dead.append(item)
		if on_done.is_valid():
			on_done.call(id)
	return dead


## Try the numbers in `jobs` child processes of Godot. Return {"ok", "error", "dead"}.
## Each child makes its own adapter and its own reference run, and tries each number whose place in `ids`
## has the rest `part` after a division by `jobs`.
static func probe_in_children(ctx: Dictionary, ids: Array, jobs: int, only: Array, talk: bool) -> Dictionary:
	var folder: String = OS.get_cache_dir().path_join("tuning_bench_probe_%d" % OS.get_process_id())
	DirAccess.make_dir_recursive_absolute(folder)
	var children: Array = []
	for part: int in range(jobs):
		var out_path: String = folder.path_join("part_%d.json" % part)
		var args: PackedStringArray = PackedStringArray(["--headless", "--quiet", "--path", ProjectSettings.globalize_path("res://"),
			"-s", CLI_SCRIPT, "--", "--probe", "--part", "%d/%d" % [part, jobs], "--bench", str(ctx["path"]), "--out", out_path])
		for glob: Variant in only:
			args.append_array(PackedStringArray(["--only", str(glob)]))
		var pid: int = OS.create_process(OS.get_executable_path(), args)
		if pid <= 0:
			_remove_folder(folder)
			return {"ok": false, "error": "cannot start a child process of Godot for the dead-number check", "dead": []}
		children.append({"pid": pid, "out": out_path, "progress": out_path + ".progress", "done": false})
	var dead: Array = []
	var problem: String = ""
	var shown: int = 0
	var left: int = children.size()
	var started_ms: int = Time.get_ticks_msec()
	var said_ms: int = started_ms
	while left > 0:
		OS.delay_msec(POLL_MS)
		for child: Dictionary in children:
			if child["done"]:
				continue
			var finished: bool = FileAccess.file_exists(str(child["out"]))
			if not finished and OS.is_process_running(int(child["pid"])):
				continue
			child["done"] = true
			left -= 1
			var read: Dictionary = BenchJson.read_file(str(child["out"]))
			if not read["ok"] or not read["value"] is Dictionary:
				problem = "a child process of the dead-number check stopped with no result"
			elif str((read["value"] as Dictionary).get("format", "")) != "tuning-bench-probe":
				problem = str((read["value"] as Dictionary).get("error", "a child process of the dead-number check failed"))
			else:
				dead.append_array((read["value"] as Dictionary).get("dead", []))
		var count: int = 0
		for child: Dictionary in children:
			count += _line_count(str(child["progress"]))
		while talk and shown + PROGRESS_EVERY <= count:
			shown += PROGRESS_EVERY
			said_ms = Time.get_ticks_msec()
			print("  dead-number check: %d of %d" % [shown, ids.size()])
		if talk and Time.get_ticks_msec() - said_ms >= HEARTBEAT_MS:
			# A run of a large game is long. This line says that the check still works.
			said_ms = Time.get_ticks_msec()
			print("  still running: %d of %d numbers tried, %d s" % [count, ids.size(), (said_ms - started_ms) / 1000])
	for child: Dictionary in children:
		if OS.is_process_running(int(child["pid"])):
			OS.kill(int(child["pid"]))
	_remove_folder(folder)
	if problem != "":
		return {"ok": false, "error": problem, "dead": []}
	var place: Dictionary = {}
	for index: int in range(ids.size()):
		place[ids[index]] = index
	dead.sort_custom(func(a: Dictionary, b: Dictionary) -> bool: return int(place.get(a["id"], 0)) < int(place.get(b["id"], 0)))
	return {"ok": true, "error": "", "dead": dead}


static func _line_count(path: String) -> int:
	if not FileAccess.file_exists(path):
		return 0
	return FileAccess.get_file_as_string(path).count("\n")


static func _remove_folder(folder: String) -> void:
	var dir: DirAccess = DirAccess.open(folder)
	if dir == null:
		return
	for name: String in dir.get_files():
		dir.remove(name)
	DirAccess.remove_absolute(folder)


## The work of 1 child process of `probe_in_children`. Return {"ok", "error", "result"}.
static func run_probe_part(ctx: Dictionary, only: Array, part: int, jobs: int, progress_path: String) -> Dictionary:
	var made: Dictionary = Runner.make_adapter(ctx)
	if not made["ok"]:
		return {"ok": false, "error": "adapter error: " + str(made["error"]), "result": {}}
	var adapter: RefCounted = made["adapter"]
	var player: Dictionary = _main_player(ctx)
	var first: Dictionary = Runner.run(adapter, ctx, Knobs.data_for(ctx, {}), player, {})
	if not (first["errors"] as Array).is_empty():
		return {"ok": false, "error": "adapter error: " + "; ".join(PackedStringArray(first["errors"])), "result": {}}
	var all: Array = select_numbers(ctx, only)
	var mine: Array = []
	for index: int in range(all.size()):
		if index % jobs == part:
			mine.append(all[index])
	var progress: FileAccess = FileAccess.open(progress_path, FileAccess.WRITE)
	var note: Callable = func(id: Variant) -> void:
		if progress != null:
			progress.store_line(str(id))
			progress.flush()
	var dead: Array = probe(ctx, adapter, player, first["log"], mine, note)
	if progress != null:
		progress.close()
	return {"ok": true, "error": "", "result": {"format": "tuning-bench-probe", "version": 1, "dead": dead, "tried": mine.size()}}


## Run the self-check. Return {"ok", "error", "result"}. `error` is not empty for bad input or an adapter error.
## Options: "child" (false: do not start a second process for the repeat check), "print" (print the progress),
## "only" (a list of globs on the number id: the dead-number check tries only these numbers),
## "jobs" (the count of child processes of the dead-number check; 1 = in this process).
static func run_check(ctx: Dictionary, opts: Dictionary = {}) -> Dictionary:
	var bench: Dictionary = ctx["bench"]
	var numbers: Dictionary = ctx["numbers"]
	var checks: Array = []
	var talk: bool = opts.get("print", false)
	var only: Array = opts.get("only", [])
	var jobs: int = maxi(int(opts.get("jobs", 1)), 1)
	var files_before: Dictionary = list_user_files()

	# Counts of the ledger.
	var by_class: Dictionary = {}
	var balance: int = 0
	var not_tunable: Array = []
	var unclassified: Array = []
	var stale: Array = []
	for id: Variant in numbers:
		var number: Dictionary = numbers[id]
		var kind: String = str(number.get("class", "unclassified"))
		by_class[kind] = int(by_class.get(kind, 0)) + 1
		if kind == "unclassified":
			unclassified.append(id)
		if kind == "balance":
			balance += 1
			if not Knobs.is_tunable(number):
				not_tunable.append(id)
	var base: Dictionary = ctx["base"]
	for id: Variant in ctx["tunable"]:
		var number: Dictionary = numbers[id]
		if number.has("value") and float(number["value"]) != float(base[id]):
			stale.append("%s: the ledger has %s, the file has %s" % [id, number["value"], base[id]])

	var made: Dictionary = Runner.make_adapter(ctx)
	if not made["ok"]:
		return {"ok": false, "error": "adapter error: " + str(made["error"]), "result": {}}
	var adapter: RefCounted = made["adapter"]
	var player: Dictionary = _main_player(ctx)
	var data: Dictionary = Knobs.data_for(ctx, {})

	# First run: the contract.
	if talk:
		print("  self-check: run 1 of the main player (the contract)")
	var first: Dictionary = Runner.run(adapter, ctx, data, player, {"strict": true})
	if not (first["errors"] as Array).is_empty():
		return {"ok": false, "error": "adapter error: " + "; ".join(PackedStringArray(first["errors"])), "result": {}}
	var reference: Array = first["log"]

	# Clean start: the second setup in this process must give the same run.
	if talk:
		print("  self-check: run 2 in the same process (clean start); run 1 took %d steps" % int(first["steps"]))
	var second: Dictionary = Runner.run(adapter, ctx, data, player, {})
	var clean: bool = second["log"] == reference
	var clean_detail: Array = []
	if not clean:
		clean_detail.append("the run after a second setup() differs from the first run: state leaks through setup()")

	# Repeat: a run in a new process must give the same result file.
	var repeat_ok: bool = true
	var repeat_detail: Array = []
	var traces: Dictionary = {player["id"]: first}
	first["limit_kind"] = "max_hours"
	var mine: Dictionary = Meters.compute(ctx, Knobs.targets(ctx, {}), traces, [player["id"]], Knobs.report_k(ctx, {}), {})
	var mine_text: String = JSON.stringify(mine, "  ", true) + "\n"
	var used_child: bool = false
	if opts.get("child", true):
		if talk:
			print("  self-check: run 3 in a second process (repeat)")
		var out_path: String = OS.get_cache_dir().path_join("tuning_bench_repeat_%d.json" % OS.get_process_id())
		var args: PackedStringArray = PackedStringArray(["--headless", "--path", ProjectSettings.globalize_path("res://"),
			"-s", CLI_SCRIPT, "--", "--run", "--player", str(player["id"]), "--no-check",
			"--bench", str(ctx["path"]), "--out", out_path])
		var output: Array = []
		var code: int = OS.execute(OS.get_executable_path(), args, output, true)
		if code >= 0 and code <= 1 and FileAccess.file_exists(out_path):
			used_child = true
			var theirs: String = FileAccess.get_file_as_string(out_path)
			if theirs != mine_text:
				repeat_ok = false
				repeat_detail.append("a run in a second process gives another result: the game uses a clock or a random value with no seed")
		DirAccess.remove_absolute(out_path)
	if not used_child:
		var again: Dictionary = Runner.make_adapter(ctx)
		var third: Dictionary = Runner.run(again["adapter"], ctx, data, player, {})
		if third["log"] != reference:
			repeat_ok = false
			repeat_detail.append("2 runs with the same input differ")
		repeat_detail.append("the check ran in 1 process only")
	checks.append({"id": "repeat", "name": "Repeat", "ok": repeat_ok, "detail": repeat_detail})
	checks.append({"id": "clean_start", "name": "Clean start", "ok": clean, "detail": clean_detail})

	# Dead number: each tunable number must change the run.
	var dead: Array = []
	var dead_fail: Array = []
	var tunable: Array = ctx["tunable"]
	var tried: Array = select_numbers(ctx, only)
	if clean and repeat_ok:
		if talk:
			print("  self-check: %d tunable numbers, 1 run for each; processes: %d" % [tried.size(), mini(jobs, maxi(tried.size(), 1))])
		if jobs > 1 and tried.size() > 1:
			var parts: Dictionary = probe_in_children(ctx, tried, mini(jobs, tried.size()), only, talk)
			if not parts["ok"]:
				return {"ok": false, "error": str(parts["error"]), "result": {}}
			dead = parts["dead"]
		else:
			var count: Array = [0, Time.get_ticks_msec(), Time.get_ticks_msec()]
			var note: Callable = func(_id: Variant) -> void:
				count[0] = int(count[0]) + 1
				if talk and int(count[0]) % PROGRESS_EVERY == 0:
					count[2] = Time.get_ticks_msec()
					print("  dead-number check: %d of %d" % [count[0], tried.size()])
				elif talk and Time.get_ticks_msec() - int(count[2]) >= HEARTBEAT_MS:
					count[2] = Time.get_ticks_msec()
					print("  still running: %d of %d numbers tried, %d s" % [count[0], tried.size(), (int(count[2]) - int(count[1])) / 1000])
			dead = probe(ctx, adapter, player, reference, tried, note)
		dead_fail = dead_lines(dead)
	else:
		tried = []
		dead_fail.append(NOT_CHECKED)
	checks.append({"id": "dead_number", "name": "Dead number", "ok": dead_fail.is_empty(), "detail": dead_fail})

	# Unknown source.
	var unknown: Array = []
	for id: Variant in ctx["foreign"]:
		unknown.append("%s: the file %s is not in \"sources\"" % [id, (numbers[id] as Dictionary).get("file", "")])
	for id: Variant in ctx["missing"]:
		unknown.append("%s: the pointer %s gives no number in %s" % [id, (numbers[id] as Dictionary).get("pointer", ""), (numbers[id] as Dictionary).get("file", "")])
	checks.append({"id": "unknown_source", "name": "Unknown source", "ok": unknown.is_empty(), "detail": unknown})

	# Unclassified.
	var unclassified_detail: Array = []
	for id: Variant in unclassified:
		unclassified_detail.append("%s has the class \"unclassified\"" % id)
	checks.append({"id": "unclassified", "name": "Unclassified", "ok": unclassified.is_empty(), "detail": unclassified_detail})

	# Contract.
	var contract: Array = first["contract"]
	checks.append({"id": "contract", "name": "Contract", "ok": contract.is_empty(), "detail": contract})

	# No file written: the simulation must not make or change a file of the player in `user://`.
	var written: Array = changed_files(files_before, list_user_files())
	checks.append({"id": "no_file_written", "name": "No file written", "ok": written.is_empty(), "detail": written})

	var ok: bool = true
	for check: Dictionary in checks:
		if not check["ok"]:
			ok = false
	var result: Dictionary = {"format": "tuning-bench-check", "version": 1, "game": str(bench.get("game", "")), "ok": ok,
		"checks": checks, "dead": dead, "stale": stale, "ledger_hash": Knobs.ledger_hash(ctx),
		"partial": tried.size() < tunable.size(), "checked": tried,
		"numbers": {"total": numbers.size(), "balance": balance, "tunable": tunable.size(),
			"not_tunable": not_tunable, "by_class": by_class},
		"run": {"player": player["id"], "ended": first["ended"], "length_s": first["length_s"], "stop": Meters.stop_of(first)}}
	return {"ok": true, "error": "", "result": result}


## Return 1 line for each number with no effect that has no `dead_ok`.
static func dead_lines(dead: Array) -> Array:
	var out: Array = []
	for item: Dictionary in dead:
		if not item.has("dead_ok"):
			out.append("%s (%s): the value %s and the value %s give the same run" % [item["id"], item["label"], item["value"], item["tried"]])
	return out


## Put the result of a self-check of some numbers (`--only`) into an older result of the same ledger.
## The numbers that the new check tried replace their old entries. Each other entry stays.
## An older result of another ledger (another `ledger_hash`) is not used: the new result stays as it is.
static func merge(ctx: Dictionary, old: Dictionary, new: Dictionary) -> Dictionary:
	if str(old.get("format", "")) != "tuning-bench-check" or str(old.get("ledger_hash", "")) != str(new.get("ledger_hash", "")):
		return new
	if not old.get("dead", null) is Array or not old.get("checked", null) is Array:
		return new
	var tried: Dictionary = {}
	for id: Variant in new["checked"]:
		tried[id] = true
	var seen: Dictionary = tried.duplicate()
	for id: Variant in old["checked"]:
		seen[id] = true
	var by_id: Dictionary = {}
	for item: Variant in old["dead"]:
		if item is Dictionary and not tried.has((item as Dictionary).get("id", "")):
			by_id[(item as Dictionary)["id"]] = item
	for item: Dictionary in new["dead"]:
		by_id[item["id"]] = item
	var dead: Array = []
	var checked: Array = []
	var numbers: Dictionary = ctx["numbers"]
	for id: Variant in ctx["tunable"]:
		if seen.has(id):
			checked.append(id)
		if by_id.has(id):
			var item: Dictionary = (by_id[id] as Dictionary).duplicate()
			# The reason of the bench file of now counts.
			item.erase("dead_ok")
			if (numbers[id] as Dictionary).has("dead_ok"):
				item["dead_ok"] = str((numbers[id] as Dictionary)["dead_ok"])
			dead.append(item)
	var out: Dictionary = new.duplicate(true)
	out["dead"] = dead
	out["checked"] = checked
	out["partial"] = checked.size() < (ctx["tunable"] as Array).size()
	var ok: bool = true
	for check: Dictionary in out["checks"]:
		if check["id"] == "dead_number" and not (check["detail"] as Array).has(NOT_CHECKED):
			check["detail"] = dead_lines(dead)
			check["ok"] = (check["detail"] as Array).is_empty()
		if not check["ok"]:
			ok = false
	out["ok"] = ok
	return out
