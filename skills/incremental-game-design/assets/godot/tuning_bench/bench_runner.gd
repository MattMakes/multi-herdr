extends RefCounted
## The run of the tuning bench: 1 player plays 1 game through the adapter. The output is a trace.
##
## The runner has no game math. It moves the clock, asks the adapter for the actions, and lets the bot decide.

const BenchJson := preload("res://addons/tuning_bench/bench_json.gd")
const Knobs := preload("res://addons/tuning_bench/bench_knobs.gd")
const Player := preload("res://addons/tuning_bench/bench_player.gd")
const Meters := preload("res://addons/tuning_bench/bench_meters.gd")
const AdapterBase := preload("res://addons/tuning_bench/bench_adapter.gd")

const CORE_ADAPTER := "res://addons/tuning_bench/core_adapter.gd"
const KINDS: Array[String] = ["buy", "once", "advance", "reset", "other"]
const MAX_ACTIONS_PER_DECISION := 5000
const NO_RATE_BEFORE := 999.0
## A player that is not the main player stops at the upper limit of the target of meter 6 times this value.
const OTHER_PLAYER_MARGIN := 1.25
## A player that cannot move stops after this game time (the key `sim.stall_hours`; 0 = the rule is off).
const DEFAULT_STALL_HOURS := 2.0
const PROFILE_ADAPTER := "res://addons/tuning_bench/bench_profile.gd"


## Make the adapter of the game. Return {"ok", "error", "adapter"}.
## `bench.json` names a script with "adapter". With "core" and no "adapter" the bench uses the core adapter.
static func make_adapter(ctx: Dictionary) -> Dictionary:
	var bench: Dictionary = ctx["bench"]
	var path: String = str(bench.get("adapter", ""))
	if path == "":
		if str(bench.get("core", "")) == "":
			return {"ok": false, "error": "bench.json has no key \"adapter\" and no key \"core\"", "adapter": null}
		path = CORE_ADAPTER
	if not ResourceLoader.exists(path):
		return {"ok": false, "error": "the adapter %s does not exist" % path, "adapter": null}
	var loaded: Variant = load(path)
	if not loaded is GDScript or not (loaded as GDScript).can_instantiate():
		return {"ok": false, "error": "the adapter %s does not load (see the errors above)" % path, "adapter": null}
	var made: Variant = (loaded as GDScript).new()
	if not made is AdapterBase:
		return {"ok": false, "error": "the adapter %s does not extend bench_adapter.gd" % path, "adapter": null}
	var adapter: AdapterBase = made
	adapter.bench_config = bench
	var sim: Variant = bench.get("sim", {})
	if sim is Dictionary:
		adapter.bench_seed = int((sim as Dictionary).get("seed", 1))
	var problem: String = adapter.bench_prepare()
	if problem != "":
		return {"ok": false, "error": problem, "adapter": null}
	return {"ok": true, "error": "", "adapter": adapter}


static func _sim(ctx: Dictionary, key: String, default: float) -> float:
	var sim: Variant = (ctx["bench"] as Dictionary).get("sim", {})
	if sim is Dictionary and (sim as Dictionary).has(key):
		return float((sim as Dictionary)[key])
	return default


## Play 1 game for 1 player. Return the trace.
## Options: "strict" (check the contract), "reference" (the log of another run: stop at the first difference),
## "cancel" (a dictionary; the run stops when its key "cancel" is true), "record" (keep the list of actions),
## "replay" (a list of [time, action id]: do these actions and let no bot decide),
## "max_s" (stop the run at this game time; the default is `sim.max_hours`),
## "checkpoints" (a list of game times in order: the trace gets [time, hash of the state] at the first decision
## at or after each time, before the actions of that decision; the adapter needs `state_hash`).
##
## An action with `"bot": false` is a free choice of the player. The bot never does it, and it does not count
## in the wake rule. It is in the table of the actions, so the view rule "each action id has a control" sees it.
##
## The stall rule: the run stops when, for `sim.stall_hours` of game time (default 2; 0 = off), the bot did no
## action, the game reported no purchase of its own (an event `bought`), and `progress()` and `rate()` did not
## change. The trace then has "stalled": true and "stall_from_s" (the time of the last of these changes).
## A replay has no stall rule: it runs to the time of its record.
##
## The order of 1 decision at the time t:
## 1. the bot looks at `actions()` and does actions until nothing is affordable;
## 2. the runner takes `events()`, `rate()` and `progress()`;
## 3. the runner calls `step()` for `decision_s` seconds, in parts of at most `max_step()`;
## 4. the runner calls `active(decision_s x active_share)` 1 time, when `active_share` is above 0;
## 5. when the player goes away now, the runner calls `away(away_min x 60)`; the clock moves by that time.
## The next decision is at t + decision_s (plus the time away).
static func run(adapter: AdapterBase, ctx: Dictionary, data: Dictionary, player: Dictionary, opts: Dictionary = {}) -> Dictionary:
	var bench: Dictionary = ctx["bench"]
	var strict: bool = opts.get("strict", false)
	var has_reference: bool = opts.has("reference")
	var reference: Array = opts.get("reference", [])
	var cancel: Dictionary = opts.get("cancel", {})
	var has_replay: bool = opts.has("replay")
	var replay: Array = opts.get("replay", [])
	var replay_at: int = 0
	var record: Array = []

	var decision_s: float = maxf(float(player.get("decision_s", 2.0)), 0.05)
	var active_share: float = clampf(float(player.get("active_share", 0.0)), 0.0, 1.0)
	var max_t: float = _sim(ctx, "max_hours", 18.0) * 3600.0
	if opts.has("max_s"):
		max_t = minf(max_t, float(opts["max_s"]))
	var scan_s: float = _sim(ctx, "scan_s", 10.0)
	var away_every_s: float = maxf(float(player.get("away_every_min", 0.0)), 0.0) * 60.0
	var away_s: float = maxf(float(player.get("away_min", 0.0)), 0.0) * 60.0
	var goes_away: bool = away_every_s > 0.0 and away_s > 0.0 and not has_replay
	var stall_s: float = 0.0 if has_replay else maxf(_sim(ctx, "stall_hours", DEFAULT_STALL_HOURS), 0.0) * 3600.0
	var checkpoints: Array = opts.get("checkpoints", [])
	var checkpoint_at: int = 0
	var can_hash: bool = not checkpoints.is_empty() and adapter.has_method("state_hash")

	var trace: Dictionary = {"player": str(player.get("id", "")), "ended": false, "length_s": 0.0, "purchases": [],
		"advances": [], "resets": [], "content": [], "events": [], "phases": [], "rate": [], "actions": {},
		"errors": [], "contract": [], "log": [], "diverged": false, "cancelled": false, "decision_s": decision_s,
		"record": record, "scans": 0, "steps": 0, "auto": [], "aways": [], "limit_s": max_t,
		"away": "never", "active_share": active_share, "stalled": false, "stall_from_s": 0.0, "stall_s": stall_s,
		"stop": "", "checkpoints": [], "refused": 0}
	var errors: Array = trace["errors"]
	var contract: Array = trace["contract"]
	var log: Array = trace["log"]
	var table: Dictionary = trace["actions"]
	var purchases: Array = trace["purchases"]
	var auto: Array = trace["auto"]
	var aways: Array = trace["aways"]
	var advances: Array = trace["advances"]
	var content: Array = trace["content"]
	var content_seen: Dictionary = {}
	var happened: Dictionary = {}
	var rate_series: Array = trace["rate"]

	adapter.bench_errors.clear()
	adapter.bench_away_missing = false
	adapter.setup(data)
	if not adapter.bench_errors.is_empty():
		errors.append_array(adapter.bench_errors)
		return trace
	var step_max: float = adapter.max_step()
	if step_max <= 0.0:
		errors.append("max_step() of the adapter is not above 0")
		return trace
	adapter.events()

	var phases: Array = bench.get("phases", [])
	var phase_at: int = 0
	var phase_start: float = 0.0
	var phase_out: Array = trace["phases"]

	var t: float = 0.0
	var tick: int = 0
	var first_scan: bool = true
	var last_buy_t: float = 0.0
	var last_scan_t: float = -1.0e18
	var wake: float = 0.0
	var resets: int = 0
	var last_rate: float = -1.0
	var stall_rate: float = -1.0
	var diverged: bool = false
	var done: bool = false
	var away_total: float = 0.0
	var last_away_play: float = 0.0
	var away_missing: bool = false
	var scans: int = 0
	var steps: int = 0
	var last_move_t: float = 0.0
	var last_progress: float = -1.0
	var stalled: bool = false
	var limit_hit: bool = false

	while true:
		if cancel.get("cancel", false):
			trace["cancelled"] = true
			break
		while can_hash and checkpoint_at < checkpoints.size() and t >= float(checkpoints[checkpoint_at]) - 1.0e-9:
			(trace["checkpoints"] as Array).append([float(checkpoints[checkpoint_at]), str(adapter.call("state_hash"))])
			checkpoint_at += 1
		var acted: bool = false
		if has_replay:
			while replay_at < replay.size() and float((replay[replay_at] as Array)[0]) <= t:
				if not adapter.do_action(str((replay[replay_at] as Array)[1])):
					trace["refused"] = int(trace["refused"]) + 1
				replay_at += 1
				acted = true
		else:
			var scan: bool = true
			if not first_scan and t - last_scan_t < scan_s:
				var have: float = adapter.wealth()
				if have >= 0.0 and have < wake:
					scan = false
			if scan:
				last_scan_t = t
				var refused: Dictionary = {}
				var guard: int = 0
				while guard < MAX_ACTIONS_PER_DECISION:
					guard += 1
					scans += 1
					var raw: Variant = adapter.actions()
					if not raw is Array:
						errors.append("actions() of the adapter did not return a list")
						break
					var acts: Array = raw
					for entry: Variant in acts:
						if not entry is Dictionary:
							if strict:
								_note(contract, "an action is not a dictionary")
							continue
						var act: Dictionary = entry
						var act_id: String = str(act.get("id", ""))
						if strict:
							_check_action(act, contract)
						if table.has(act_id) and not (table[act_id] as Dictionary).has("from_event"):
							continue
						# A row that an auto purchase made keeps its auto purchases. The action shows now.
						var auto_times: Array = []
						var auto_count: int = 0
						if table.has(act_id):
							auto_times = (table[act_id] as Dictionary)["auto_times"]
							auto_count = int((table[act_id] as Dictionary)["auto_count"])
						table[act_id] = {"kind": str(act.get("kind", "other")), "label": str(act.get("label", act_id)),
							"element": str(act.get("element", "")), "seen_s": t, "times": [], "gains": [],
							"auto_times": auto_times, "auto_count": auto_count}
						if act.get("bot", true) is bool and not act.get("bot", true):
							# A free choice of the player. The bot does not use it, and meter 8 does not count it.
							(table[act_id] as Dictionary)["bot"] = false
						if not first_scan and not content_seen.has(act_id):
							content_seen[act_id] = true
							content.append([t, act_id])
							happened[act_id] = true
							diverged = _log(log, [t, "c", act_id], reference, has_reference) or diverged
					first_scan = false
					var pick: Dictionary = Player.choose(acts, player, t - last_buy_t, refused)
					if pick.is_empty():
						# `wealth()` is the main currency. A cost in another currency says nothing about the
						# time of the next purchase: with such an action that is not affordable, the bot looks
						# at each decision.
						var have_now: float = adapter.wealth()
						wake = INF
						for entry: Variant in acts:
							if not entry is Dictionary:
								continue
							var waiting: Dictionary = entry
							if waiting.get("bot", true) is bool and not waiting.get("bot", true):
								continue
							if str(waiting.get("currency", "")) != "":
								if not waiting.get("affordable", false):
									wake = -INF
									break
								continue
							var cost: float = float(waiting.get("cost", 0.0))
							if cost > have_now and cost < wake:
								wake = cost
						break
					var id: String = str(pick.get("id", ""))
					var kind: String = str(pick.get("kind", "other"))
					var before: float = adapter.rate()
					if not adapter.do_action(id):
						refused[id] = true
						if strict:
							_note(contract, "do_action(\"%s\") returned false for an affordable action" % id)
						continue
					var after: float = adapter.rate()
					acted = true
					happened[id] = true
					record.append([t, id])
					var row: Dictionary = table[id]
					(row["times"] as Array).append(t)
					if kind == "reset":
						resets += 1
						(trace["resets"] as Array).append(t)
						last_buy_t = t
						diverged = _log(log, [t, "r", id, BenchJson.sig(after)], reference, has_reference) or diverged
					elif kind == "advance":
						advances.append([t, id])
						diverged = _log(log, [t, "a", id, BenchJson.sig(after)], reference, has_reference) or diverged
					else:
						var gain: float = 0.0
						if before > 0.0:
							gain = after / before - 1.0
						elif after > 0.0:
							gain = NO_RATE_BEFORE
						gain = BenchJson.sig(gain)
						(row["gains"] as Array).append(gain)
						purchases.append([t, id, gain])
						last_buy_t = t
						diverged = _log(log, [t, "b", id, BenchJson.sig(after)], reference, has_reference) or diverged
					if diverged:
						break
				if guard >= MAX_ACTIONS_PER_DECISION:
					errors.append("the bot did %d actions at 1 decision; an action that is free can repeat with no end (the last action: \"%s\"). Give a free choice of the player the key \"bot\": false" % [guard, str(record[record.size() - 1][1]) if not record.is_empty() else ""])
		if not adapter.bench_errors.is_empty():
			errors.append_array(adapter.bench_errors)
		if not errors.is_empty():
			break

		# Events that the adapter reports, and the income after this decision.
		var moved: bool = acted
		var reported: Variant = adapter.events()
		if reported is Array:
			for entry: Variant in reported:
				if not entry is Dictionary:
					continue
				var event: Dictionary = entry
				var event_id: String = str(event.get("id", ""))
				var event_kind: String = str(event.get("kind", ""))
				happened[event_id] = true
				if event_kind == "unlock":
					if not content_seen.has(event_id) and not (table.has(event_id) and float((table[event_id] as Dictionary)["seen_s"]) == 0.0):
						content_seen[event_id] = true
						content.append([t, event_id])
						diverged = _log(log, [t, "c", event_id], reference, has_reference) or diverged
				elif event_kind == "bought":
					# A purchase that the game made by itself (an auto-buy). The bot did not ask for it.
					var bought: int = maxi(int(event.get("count", 1)), 1)
					if not table.has(event_id):
						table[event_id] = {"kind": "buy", "label": str(event.get("label", event_id)), "element": "",
							"seen_s": t, "times": [], "gains": [], "auto_times": [], "auto_count": 0, "from_event": true}
					var auto_row: Dictionary = table[event_id]
					(auto_row["auto_times"] as Array).append(t)
					auto_row["auto_count"] = int(auto_row["auto_count"]) + bought
					auto.append([t, event_id, bought])
					moved = true
					diverged = _log(log, [t, "ab", event_id, bought], reference, has_reference) or diverged
				else:
					(trace["events"] as Array).append([t, event_kind, event_id])
					if event_kind == "advance" and not advances.has([t, event_id]):
						# A wall that the game passed by itself. The timeline shows it in the row "walls".
						advances.append([t, event_id])
		var now_rate: float = adapter.rate()
		var progress: float = adapter.progress()
		if strict:
			if now_rate < 0.0:
				_note(contract, "rate() is below 0")
			if progress < 0.0 or progress > 1.0:
				_note(contract, "progress() is outside 0 to 1")
		if now_rate != last_rate:
			rate_series.append([t, BenchJson.sig(now_rate)])
			last_rate = now_rate

		# The end of a phase.
		while phase_at < phases.size() - 1 and _phase_ends(phases[phase_at], progress, happened, resets):
			var phase_id: String = str((phases[phase_at] as Dictionary).get("id", ""))
			phase_out.append([phase_id, phase_start, t])
			diverged = _log(log, [t, "p", phase_id], reference, has_reference) or diverged
			phase_start = t
			phase_at += 1

		if progress >= 1.0:
			done = true
			break
		if diverged:
			break
		if t >= max_t:
			limit_hit = true
			break
		if has_replay and replay_at >= replay.size() and t >= float(opts.get("replay_until", 0.0)):
			break
		# The stall rule: no purchase, and no change of the progress and the income, for `stall_s`.
		if moved or progress != last_progress or now_rate != stall_rate:
			last_move_t = t
			last_progress = progress
			stall_rate = now_rate
		elif stall_s > 0.0 and t - last_move_t >= stall_s - 1.0e-9:
			stalled = true
			break

		var left: float = decision_s
		while left > 1.0e-9:
			var dt: float = minf(left, step_max)
			adapter.step(dt)
			left -= dt
			steps += 1
		if active_share > 0.0:
			adapter.active(decision_s * active_share)
		tick += 1
		var play_t: float = float(tick) * decision_s
		t = BenchJson.ms(play_t + away_total)
		if goes_away and not away_missing and play_t - last_away_play >= away_every_s - 1.0e-9:
			# The player closes the game. The game applies its offline rule.
			adapter.away(away_s)
			if adapter.bench_away_missing:
				away_missing = true
			else:
				last_away_play = play_t
				aways.append([t, away_s])
				away_total += away_s
				var after_away: Array = [BenchJson.ms(play_t + away_total), "w", away_s]
				var have_after: float = adapter.wealth()
				if have_after >= 0.0:
					after_away.append(BenchJson.sig(have_after))
				t = after_away[0]
				diverged = _log(log, after_away, reference, has_reference) or diverged

	if phase_at < phases.size():
		phase_out.append([str((phases[phase_at] as Dictionary).get("id", "")), phase_start, t])
	if goes_away:
		trace["away"] = "not supported" if away_missing else "ok"
	trace["ended"] = done
	trace["length_s"] = t
	trace["scans"] = scans
	trace["steps"] = steps
	trace["decisions"] = tick + 1
	trace["stalled"] = stalled
	trace["stall_from_s"] = last_move_t if stalled else 0.0
	trace["stop"] = "end" if done else ("stalled" if stalled else ("limit" if limit_hit else ""))
	trace["resets_count"] = resets
	_log(log, [t, "e", done], reference, has_reference)
	if has_reference and log.size() != reference.size():
		diverged = true
	trace["diverged"] = diverged
	return trace


static func _log(log: Array, entry: Array, reference: Array, has_reference: bool) -> bool:
	log.append(entry)
	if not has_reference:
		return false
	var at: int = log.size() - 1
	return at >= reference.size() or reference[at] != entry


static func _note(list: Array, line: String) -> void:
	if list.size() < 20 and not list.has(line):
		list.append(line)


static func _check_action(act: Dictionary, contract: Array) -> void:
	var id: String = str(act.get("id", ""))
	if not act.has("id") or id == "":
		_note(contract, "an action has no id")
	for key: String in ["kind", "cost", "affordable"]:
		if not act.has(key):
			_note(contract, "the action \"%s\" has no key \"%s\"" % [id, key])
	if act.has("kind") and not KINDS.has(str(act["kind"])):
		_note(contract, "the action \"%s\" has the kind \"%s\", which is not known" % [id, act["kind"]])
	if act.has("cost") and not (act["cost"] is int or act["cost"] is float):
		_note(contract, "the cost of the action \"%s\" is not a number" % id)
	if act.has("bot") and not act["bot"] is bool:
		_note(contract, "the key \"bot\" of the action \"%s\" is not true or false" % id)


static func _phase_ends(phase: Variant, progress: float, happened: Dictionary, resets: int) -> bool:
	if not phase is Dictionary:
		return false
	var until: Variant = (phase as Dictionary).get("until", {})
	if not until is Dictionary:
		return false
	var rule: Dictionary = until
	if rule.has("progress") and progress >= float(rule["progress"]) - 1.0e-9:
		return true
	if rule.has("event") and happened.has(str(rule["event"])):
		return true
	if rule.has("resets") and resets >= int(rule["resets"]):
		return true
	return false


## Return the id of the main player of a list: the player "active", or the first player.
## `only` is the id of the 1 player that runs, or "".
static func main_player_id(players: Array, only: String = "") -> String:
	var first: String = ""
	for player: Dictionary in players:
		var id: String = str(player.get("id", ""))
		if only != "" and id != only:
			continue
		if id == "active":
			return id
		if first == "":
			first = id
	return first


## Return the game time at which a player that is not the main player stops:
## min(max_hours, length of the main player x (upper limit of the target `idle_ratio`) x 1.25).
## A longer run cannot change the status of meter 6. Return {"limit_s", "kind"}; `kind` is "m6" or "max_hours".
static func other_player_limit(ctx: Dictionary, targets: Dictionary, main: Dictionary) -> Dictionary:
	var limit_s: float = _sim(ctx, "max_hours", 18.0) * 3600.0
	var out: Dictionary = {"limit_s": limit_s, "kind": "max_hours"}
	var ratio: Variant = targets.get("idle_ratio", {})
	if main.is_empty() or not main["ended"] or not ratio is Dictionary or not (ratio as Dictionary).has("max"):
		return out
	var by_ratio: float = float(main["length_s"]) * float((ratio as Dictionary)["max"]) * OTHER_PLAYER_MARGIN
	if by_ratio > 0.0 and by_ratio < limit_s:
		out = {"limit_s": by_ratio, "kind": "m6"}
	return out


## Run each player and compute the meters. Return {"ok", "error", "result", "traces"}.
## The main player runs first. Each other player stops at `other_player_limit`.
## Options: "player" (run only this player id), "cancel", "check" (the content of a self-check result),
## "on_partial" (a Callable; the runner calls it with the result of the main player alone before the other
## players run. That result has the key "pending_players"), "profile" (a list; the runner adds 1 entry for each
## player: {"player", "usec", "steps", "decisions", "scans", "spent"}; `spent` is the time of each contract
## function, see bench_profile.gd. The result does not change).
static func simulate(ctx: Dictionary, k: Dictionary, target_changes: Dictionary = {}, opts: Dictionary = {}) -> Dictionary:
	var made: Dictionary = opts.get("adapter_made", {})
	if made.is_empty():
		made = make_adapter(ctx)
	if not made["ok"]:
		return {"ok": false, "error": made["error"], "result": {}, "traces": {}}
	var adapter: AdapterBase = made["adapter"]
	var profile: Variant = opts.get("profile", null)
	if profile is Array:
		var proxy: AdapterBase = (load(PROFILE_ADAPTER) as GDScript).new()
		proxy.call("wrap", adapter)
		adapter = proxy
	var data: Dictionary = Knobs.data_for(ctx, k)
	var only: String = str(opts.get("player", ""))
	var targets: Dictionary = Knobs.targets(ctx, target_changes)
	var check: Dictionary = opts.get("check", {})
	var players: Array = Knobs.players(ctx, k)
	var main_id: String = main_player_id(players, only)
	if main_id == "":
		return {"ok": false, "error": "the player %s is not in the bench file" % only, "result": {}, "traces": {}}
	var order: Array = []
	var queue: Array = []
	for player: Dictionary in players:
		var id: String = str(player.get("id", ""))
		order.append(id)
		if only != "" and id != only:
			continue
		if id == main_id:
			queue.push_front(player)
		else:
			queue.append(player)
	var traces: Dictionary = {}
	var limit: Dictionary = {}
	for index: int in range(queue.size()):
		var player: Dictionary = queue[index]
		var id: String = str(player.get("id", ""))
		var run_opts: Dictionary = {"cancel": opts.get("cancel", {})}
		if index > 0:
			run_opts["max_s"] = limit["limit_s"]
		var started: int = Time.get_ticks_usec()
		var trace: Dictionary = run(adapter, ctx, data, player, run_opts)
		if profile is Array:
			(profile as Array).append({"player": id, "usec": Time.get_ticks_usec() - started, "steps": trace["steps"],
				"decisions": trace.get("decisions", 0), "scans": trace["scans"], "spent": adapter.call("take")})
		if not (trace["errors"] as Array).is_empty():
			return {"ok": false, "error": "adapter error: " + "; ".join(PackedStringArray(trace["errors"])), "result": {}, "traces": {}}
		if trace["cancelled"]:
			return {"ok": false, "error": "cancelled", "result": {}, "traces": {}}
		traces[id] = trace
		if index > 0:
			trace["limit_kind"] = limit["kind"]
			continue
		trace["limit_kind"] = "max_hours"
		limit = other_player_limit(ctx, targets, trace)
		if queue.size() > 1 and opts.get("on_partial", null) is Callable:
			var pending: Array = []
			for other: Variant in order:
				if str(other) != id and (only == "" or str(other) == only):
					pending.append(other)
			(opts["on_partial"] as Callable).call(Meters.compute(ctx, targets, traces, order, Knobs.report_k(ctx, k), check, pending))
	var result: Dictionary = Meters.compute(ctx, targets, traces, order, Knobs.report_k(ctx, k), check)
	return {"ok": true, "error": "", "result": result, "traces": traces}
