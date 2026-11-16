extends RefCounted
## The meters of the tuning bench: the traces of the players go in, the 8 meters come out.
##
## A target is a range. A value inside the target has the status "pass". A value past the target and inside
## the tolerance has the status "pass (in range)". Only a value outside the range has the status "fail".

const BenchJson := preload("res://addons/tuning_bench/bench_json.gd")
const Knobs := preload("res://addons/tuning_bench/bench_knobs.gd")

const PASS := "pass"
const IN_RANGE := "pass (in range)"
const FAIL := "fail"
const NA := "n/a"
const DEFAULT_TOLERANCE := 0.2
const PURCHASE_KINDS: Array[String] = ["buy", "once", "other"]
## Meter 8 names at most this count of numbers with no effect.
const MAX_DEAD_LINES := 12


static func _rank(status: String) -> int:
	match status:
		FAIL:
			return 3
		IN_RANGE:
			return 2
		PASS:
			return 1
	return 0


## Return the worst status of a list.
static func worst(statuses: Array) -> String:
	var out: String = NA
	for status: Variant in statuses:
		if _rank(str(status)) > _rank(out):
			out = str(status)
	return out


## A target "about x": pass inside half of the tolerance, in range inside the tolerance.
static func about(value: float, target: float, tolerance: float) -> String:
	var miss: float = absf(value - target)
	if miss <= absf(target) * tolerance * 0.5 + 1.0e-12:
		return PASS
	if miss <= absf(target) * tolerance + 1.0e-12:
		return IN_RANGE
	return FAIL


static func at_most(value: float, limit: float, tolerance: float) -> String:
	if value <= limit + 1.0e-12:
		return PASS
	if value <= limit * (1.0 + tolerance) + 1.0e-12:
		return IN_RANGE
	return FAIL


static func at_least(value: float, limit: float, tolerance: float) -> String:
	if value >= limit - 1.0e-12:
		return PASS
	if value >= limit * (1.0 - tolerance) - 1.0e-12:
		return IN_RANGE
	return FAIL


static func median(values: Array) -> float:
	return percentile(values, 0.5)


## Return a percentile of a list. The list does not need an order.
static func percentile(values: Array, share: float) -> float:
	if values.is_empty():
		return 0.0
	var sorted: Array = values.duplicate()
	sorted.sort()
	var at: float = share * float(sorted.size() - 1)
	var low: int = int(floor(at))
	var high: int = mini(low + 1, sorted.size() - 1)
	return lerpf(float(sorted[low]), float(sorted[high]), at - float(low))


static func _f(value: float, digits: int = 2) -> String:
	var text: String = ("%." + str(digits) + "f") % value
	if text.contains("."):
		text = text.rstrip("0").rstrip(".")
	return text


static func _pct(value: float) -> String:
	return "%d%%" % roundi(value * 100.0)


static func _target(targets: Dictionary, key: String) -> Dictionary:
	var entry: Variant = targets.get(key, {})
	if entry is Dictionary:
		return entry
	if entry is int or entry is float:
		return {"about": entry}
	return {}


static func _tol(target: Dictionary) -> float:
	return float(target.get("tolerance", DEFAULT_TOLERANCE))


static func _phase_of(phases: Array, t: float) -> String:
	for phase: Array in phases:
		if t <= float(phase[2]):
			return str(phase[0])
	if phases.is_empty():
		return ""
	return str((phases[phases.size() - 1] as Array)[0])


static func _phase_seconds(trace: Dictionary) -> Dictionary:
	var out: Dictionary = {}
	for phase: Array in trace["phases"]:
		out[phase[0]] = float(phase[2]) - float(phase[1])
	return out


## Return the times at which 1 or more purchases came, in order. The purchases of the bot count, and with
## `with_auto` also the purchases that the game made by itself (the list "auto" of the trace).
static func purchase_moments(trace: Dictionary, with_auto: bool = true) -> Array:
	var seen: Dictionary = {}
	for purchase: Array in trace["purchases"]:
		seen[float(purchase[0])] = true
	if with_auto:
		for purchase: Array in trace.get("auto", []):
			seen[float(purchase[0])] = true
	var out: Array = seen.keys()
	out.sort()
	return out


## Return the count of the purchases that the game made by itself.
static func auto_count(trace: Dictionary) -> int:
	var count: int = 0
	for purchase: Array in trace.get("auto", []):
		count += int(purchase[2])
	return count


## Return why the run of a player stopped: "end" (progress 1), "stalled" (the stall rule of the runner),
## "m6" (the limit of the range of meter 6) or "max_hours".
static func stop_of(trace: Dictionary) -> String:
	if trace["ended"]:
		return "end"
	if trace.get("stalled", false):
		return "stalled"
	return str(trace.get("limit_kind", "max_hours"))


## Return the count of the purchases of a run: by the bot and by the game.
static func purchase_count(trace: Dictionary) -> int:
	return (trace["purchases"] as Array).size() + auto_count(trace)


## Return the text for a player that cannot move, or "" for a player that ended or that still moved at its limit.
## 0 purchases in the whole run: "the idle player cannot start: the game needs the manual action" (a player with
## `active_share` 0), or "... cannot start: no purchase in <h> h" (a player that uses the manual action).
## A stall after 1 or more purchases: "the idle player stalls at <h> h: no purchase and no progress for <n> h".
static func stuck_text(id: String, trace: Dictionary) -> String:
	if trace["ended"]:
		return ""
	if purchase_count(trace) == 0:
		if float(trace.get("active_share", 0.0)) <= 0.0:
			return "the %s player cannot start: the game needs the manual action" % id
		return "the %s player cannot start: no purchase in %s h" % [id, _f(float(trace["length_s"]) / 3600.0)]
	if trace.get("stalled", false):
		return "the %s player stalls at %s h: no purchase and no progress for %s h" % [id,
			_f(float(trace.get("stall_from_s", 0.0)) / 3600.0), _f(float(trace.get("stall_s", 0.0)) / 3600.0)]
	return ""


static func _gaps_min(moments: Array) -> Array:
	var gaps: Array = []
	for index: int in range(1, moments.size()):
		gaps.append((float(moments[index]) - float(moments[index - 1])) / 60.0)
	return gaps


## Return the class of a gain: "none", "low", "medium" or "high".
static func impact_class(gain: float, target: Dictionary) -> String:
	if gain == 0.0:
		return "none"
	if gain < float(target.get("low_below", 0.03)):
		return "low"
	if gain >= float(target.get("high_from", 0.25)):
		return "high"
	return "medium"


static func _meter(id: String, name: String, unit: String) -> Dictionary:
	return {"id": id, "name": name, "value": 0.0, "unit": unit, "text": "", "target": "no target", "status": NA,
		"phases": {}, "hint": [], "lines": []}


## Compute the result of a run. `traces` maps a player id to its trace. `order` is the order of the players.
## `pending` lists the players that did not run yet: the result is then the result of the main player alone.
## A meter has the key "lines": more lines of text that go below "text" (for example "by hand: ..." of M2).
static func compute(ctx: Dictionary, targets: Dictionary, traces: Dictionary, order: Array, k: Dictionary, check: Dictionary, pending: Array = []) -> Dictionary:
	var bench: Dictionary = ctx["bench"]
	var main_id: String = ""
	var idle_id: String = ""
	for id: Variant in order:
		if traces.has(id) and (main_id == "" or str(id) == "active"):
			main_id = str(id)
	for id: Variant in order:
		if traces.has(id) and str(id) != main_id:
			idle_id = str(id)
	if traces.has("idle") and main_id != "idle":
		idle_id = "idle"
	var main: Dictionary = traces[main_id]
	var phases: Array = main["phases"]
	var length_s: float = maxf(float(main["length_s"]), 1.0e-9)
	var max_hours: float = 18.0
	if bench.get("sim", null) is Dictionary:
		max_hours = float((bench["sim"] as Dictionary).get("max_hours", 18.0))

	var players_out: Dictionary = {}
	for id: Variant in traces:
		var trace: Dictionary = traces[id]
		var by_game: int = auto_count(trace)
		players_out[id] = {"ended": trace["ended"], "length_s": trace["length_s"],
			"purchases": (trace["purchases"] as Array).size() + by_game, "auto_purchases": by_game,
			"resets": (trace["resets"] as Array).size(), "away": str(trace.get("away", "never")),
			"away_count": (trace.get("aways", []) as Array).size(),
			"limit_s": trace.get("limit_s", max_hours * 3600.0), "limit_kind": str(trace.get("limit_kind", "max_hours")),
			"stop": stop_of(trace)}
		if trace.get("stalled", false):
			players_out[id]["stall_from_s"] = trace.get("stall_from_s", 0.0)

	var meters: Array = []

	# M1 Game length.
	var m1: Dictionary = _meter("M1", "Game length", "h")
	var t1: Dictionary = _target(targets, "length_h")
	var hours: float = length_s / 3600.0
	m1["value"] = BenchJson.sig(hours)
	var others: PackedStringArray = PackedStringArray()
	for id: Variant in order:
		if traces.has(id) and str(id) != main_id:
			var other: Dictionary = traces[id]
			if other["ended"]:
				others.append("%s %s h" % [id, _f(float(other["length_s"]) / 3600.0)])
			elif stuck_text(str(id), other) != "":
				others.append(stuck_text(str(id), other))
			else:
				others.append("%s does not end" % id)
	if not pending.is_empty():
		others.append("more players run…")
	var tail: String = "" if others.is_empty() else " (" + ", ".join(others) + ")"
	if t1.has("about"):
		var want: float = float(t1["about"])
		m1["target"] = "about %s h (%s to %s)" % [_f(want), _f(want * (1.0 - _tol(t1))), _f(want * (1.0 + _tol(t1)))]
	if main["ended"]:
		m1["text"] = "%s h%s" % [_f(hours), tail]
		m1["status"] = about(hours, float(t1["about"]), _tol(t1)) if t1.has("about") else PASS
	elif stuck_text(main_id, main) != "":
		# The main player cannot move. This is a finding about the game, not about the limit of the run.
		m1["text"] = stuck_text(main_id, main) + tail
		m1["status"] = FAIL
	else:
		m1["text"] = "does not end in %s h%s" % [_f(max_hours), tail]
		m1["status"] = FAIL
	var seconds: Dictionary = _phase_seconds(main)
	for id: Variant in seconds:
		(m1["phases"] as Dictionary)[id] = BenchJson.sig(float(seconds[id]) / 3600.0)
	m1["hint"] = Knobs.hint(ctx, ["cost_base", "wall", "effect", "effect_factor", "cost_growth"])
	meters.append(m1)

	# M2 Time between upgrades.
	var m2: Dictionary = _meter("M2", "Time between upgrades", "min")
	var t2: Dictionary = _target(targets, "upgrade_gap_min")
	# The purchases of the bot and the purchases that the game made by itself (auto-buy) both count.
	var moments: Array = purchase_moments(main)
	var main_auto: int = auto_count(main)
	var gaps: Array = _gaps_min(moments)
	var gaps_by_phase: Dictionary = {}
	for index: int in range(1, moments.size()):
		var gap: float = gaps[index - 1]
		var phase_id: String = _phase_of(phases, float(moments[index]))
		if not gaps_by_phase.has(phase_id):
			gaps_by_phase[phase_id] = []
		(gaps_by_phase[phase_id] as Array).append(gap)
	if gaps.is_empty():
		m2["text"] = "fewer than 2 purchases"
		m2["status"] = FAIL
	else:
		var mid: float = median(gaps)
		var p90: float = percentile(gaps, 0.9)
		var longest: float = float(gaps.max())
		m2["value"] = BenchJson.sig(mid)
		m2["text"] = "median %s min, 90th percentile %s min, longest %s min" % [_f(mid), _f(p90), _f(longest)]
		var parts: PackedStringArray = PackedStringArray()
		var states: Array = []
		if t2.has("median_about"):
			var want_mid: float = float(t2["median_about"])
			parts.append("median about %s min (%s to %s)" % [_f(want_mid), _f(want_mid * (1.0 - _tol(t2))), _f(want_mid * (1.0 + _tol(t2)))])
			states.append(about(mid, want_mid, _tol(t2)))
		if t2.has("longest_max"):
			var limit: float = float(t2["longest_max"])
			parts.append("longest at most %s min (range to %s)" % [_f(limit), _f(limit * (1.0 + _tol(t2)))])
			states.append(at_most(longest, limit, _tol(t2)))
		if not parts.is_empty():
			m2["target"] = "; ".join(parts)
		m2["status"] = PASS if states.is_empty() else worst(states)
	if main_auto > 0:
		var hand_gaps: Array = _gaps_min(purchase_moments(main, false))
		if hand_gaps.is_empty():
			(m2["lines"] as Array).append("by hand: fewer than 2 purchases")
		else:
			(m2["lines"] as Array).append("by hand: median %s min, 90th percentile %s min, longest %s min" % [
				_f(median(hand_gaps)), _f(percentile(hand_gaps, 0.9)), _f(float(hand_gaps.max()))])
			m2["by_hand"] = {"median": BenchJson.sig(median(hand_gaps)), "p90": BenchJson.sig(percentile(hand_gaps, 0.9)),
				"longest": BenchJson.sig(float(hand_gaps.max()))}
	for phase: Array in phases:
		if gaps_by_phase.has(phase[0]):
			(m2["phases"] as Dictionary)[phase[0]] = BenchJson.sig(median(gaps_by_phase[phase[0]]))
	m2["hint"] = Knobs.hint(ctx, ["cost_growth", "cost_base", "effect", "effect_factor", "auto_at"])
	meters.append(m2)

	# M3 Time between new things.
	var m3: Dictionary = _meter("M3", "Time between new things", "min")
	var t3: Dictionary = _target(targets, "content_gap_min")
	var content_times: Array = []
	var first_hour: int = 0
	for item: Array in main["content"]:
		content_times.append(float(item[0]))
		if float(item[0]) <= 3600.0:
			first_hour += 1
	var longest_content: float = _longest_gap(content_times, 0.0, length_s) / 60.0
	m3["value"] = BenchJson.sig(longest_content)
	m3["text"] = "longest %s min with no new thing; %d new things in the first hour; %d in the game" % [_f(longest_content), first_hour, content_times.size()]
	var parts3: PackedStringArray = PackedStringArray()
	var states3: Array = []
	if t3.has("longest_max"):
		var limit3: float = float(t3["longest_max"])
		parts3.append("longest at most %s min (range to %s)" % [_f(limit3), _f(limit3 * (1.0 + _tol(t3)))])
		states3.append(at_most(longest_content, limit3, _tol(t3)))
	if t3.has("first_hour_min_count"):
		var floor3: float = float(t3["first_hour_min_count"])
		parts3.append("first hour at least %s (range from %s)" % [_f(floor3), _f(floor3 * (1.0 - _tol(t3)))])
		states3.append(at_least(float(first_hour), floor3, _tol(t3)))
	if not parts3.is_empty():
		m3["target"] = "; ".join(parts3)
	m3["status"] = PASS if states3.is_empty() else worst(states3)
	for phase: Array in phases:
		var inside: Array = []
		for t: Variant in content_times:
			if float(t) > float(phase[1]) and float(t) <= float(phase[2]):
				inside.append(t)
		(m3["phases"] as Dictionary)[phase[0]] = BenchJson.sig(_longest_gap(inside, float(phase[1]), float(phase[2])) / 60.0)
	m3["hint"] = Knobs.hint(ctx, ["unlock_at", "wall", "cost_base"])
	meters.append(m3)

	# M4 Impact of an upgrade.
	var m4: Dictionary = _meter("M4", "Impact of an upgrade", "gain")
	var t4: Dictionary = _target(targets, "impact")
	var counts: Dictionary = {"none": 0, "low": 0, "medium": 0, "high": 0}
	var gains: Array = []
	var gains_by_phase: Dictionary = {}
	for purchase: Array in main["purchases"]:
		var gain: float = purchase[2]
		var kind: String = impact_class(gain, t4)
		counts[kind] = int(counts[kind]) + 1
		if kind == "none":
			continue
		gains.append(gain)
		var phase_id: String = _phase_of(phases, float(purchase[0]))
		if not gains_by_phase.has(phase_id):
			gains_by_phase[phase_id] = []
		(gains_by_phase[phase_id] as Array).append(gain)
	# An auto purchase has no measured impact: the bench does not see the income before and after it.
	var not_measured: String = ""
	if main_auto > 0:
		not_measured = "; %d auto purchases not measured" % main_auto
	if gains.is_empty():
		m4["text"] = "no purchase changes the income" + not_measured
		m4["status"] = FAIL
	else:
		var total: float = float(gains.size())
		var low_share: float = float(counts["low"]) / total
		var mid_share: float = float(counts["medium"]) / total
		var high_share: float = float(counts["high"]) / total
		m4["value"] = BenchJson.sig(median(gains))
		m4["text"] = "low %s, medium %s, high %s; median gain %s; %d with no rate change" % [_pct(low_share), _pct(mid_share), _pct(high_share), _pct(median(gains)), int(counts["none"])] + not_measured
		m4["shares"] = {"low": BenchJson.sig(low_share), "medium": BenchJson.sig(mid_share), "high": BenchJson.sig(high_share),
			"none_count": counts["none"], "auto_count": main_auto}
		var parts4: PackedStringArray = PackedStringArray()
		var states4: Array = []
		if t4.has("low_share_max"):
			var limit_low: float = float(t4["low_share_max"])
			parts4.append("low at most %s (range to %s)" % [_pct(limit_low), _pct(limit_low * (1.0 + _tol(t4)))])
			states4.append(at_most(low_share, limit_low, _tol(t4)))
		if t4.has("high_share_min"):
			var floor_high: float = float(t4["high_share_min"])
			parts4.append("high at least %s (range from %s)" % [_pct(floor_high), _pct(floor_high * (1.0 - _tol(t4)))])
			states4.append(at_least(high_share, floor_high, _tol(t4)))
		if not parts4.is_empty():
			m4["target"] = "; ".join(parts4)
		m4["status"] = PASS if states4.is_empty() else worst(states4)
	for phase: Array in phases:
		if gains_by_phase.has(phase[0]):
			(m4["phases"] as Dictionary)[phase[0]] = BenchJson.sig(median(gains_by_phase[phase[0]]))
	m4["hint"] = Knobs.hint(ctx, ["effect", "effect_factor", "effect_growth", "cost_growth"])
	meters.append(m4)

	# M5 Phase balance.
	var m5: Dictionary = _meter("M5", "Phase balance", "share")
	var t5: Dictionary = _target(targets, "phase_share")
	var parts5: PackedStringArray = PackedStringArray()
	var wants5: PackedStringArray = PackedStringArray()
	var states5: Array = []
	var worst_miss: float = 0.0
	var hint5: Array = []
	for entry: Variant in bench.get("phases", []):
		if not entry is Dictionary:
			continue
		var phase: Dictionary = entry
		var phase_id: String = str(phase.get("id", ""))
		var name: String = str(phase.get("name", phase_id))
		var share: float = float(seconds.get(phase_id, 0.0)) / length_s
		(m5["phases"] as Dictionary)[phase_id] = BenchJson.sig(share)
		if not phase.has("share"):
			parts5.append("%s %s" % [name, _pct(share)])
			continue
		var want_share: float = float(phase["share"])
		parts5.append("%s %s" % [name, _pct(share)])
		wants5.append("%s about %s" % [name, _pct(want_share)])
		var miss: float = absf(share - want_share)
		var state: String = FAIL
		if miss <= maxf(want_share * _tol(t5) * 0.5, 0.025):
			state = PASS
		elif miss <= maxf(want_share * _tol(t5), 0.05):
			state = IN_RANGE
		if not main["ended"] and not seconds.has(phase_id):
			state = FAIL
		states5.append(state)
		if miss > worst_miss:
			worst_miss = miss
			hint5 = []
			for control: Dictionary in ctx["controls"]:
				var select: Dictionary = control["select"]
				if not control["generated"] and select.get("phase", null) is Array and (select["phase"] as Array).has(phase_id):
					hint5.append(control["id"])
	m5["value"] = BenchJson.sig(worst_miss)
	m5["text"] = ", ".join(parts5) + "; largest miss %s points" % _f(worst_miss * 100.0, 1)
	if not wants5.is_empty():
		m5["target"] = ", ".join(wants5)
	m5["status"] = PASS if states5.is_empty() else worst(states5)
	m5["hint"] = hint5
	meters.append(m5)

	# M6 Active against idle.
	var m6: Dictionary = _meter("M6", "Active against idle", "ratio")
	var t6: Dictionary = _target(targets, "idle_ratio")
	if t6.has("min") or t6.has("max"):
		m6["target"] = "%s to %s (range %s to %s)" % [_f(float(t6.get("min", 0.0))), _f(float(t6.get("max", 0.0))),
			_f(float(t6.get("min", 0.0)) * (1.0 - _tol(t6))), _f(float(t6.get("max", 0.0)) * (1.0 + _tol(t6)))]
	if not pending.is_empty():
		m6["text"] = "more players run…"
	elif idle_id == "":
		m6["text"] = "the run has 1 player only"
	else:
		var idle: Dictionary = traces[idle_id]
		if not idle["ended"] and purchase_count(idle) == 0:
			# The player bought nothing in the whole run. The ratio has no meaning: the meter is not a failure
			# of the balance. Meter 1 has the same text.
			m6["text"] = stuck_text(idle_id, idle)
		elif not idle["ended"] or not main["ended"]:
			if not idle["ended"] and idle.get("stalled", false):
				m6["text"] = stuck_text(idle_id, idle)
			elif not idle["ended"] and str(idle.get("limit_kind", "")) == "m6":
				# The run stopped early: a longer run cannot come back into the range of this meter.
				m6["text"] = "the %s player does not end inside %s h (limit of the M6 range)" % [idle_id, _f(float(idle["limit_s"]) / 3600.0)]
			else:
				m6["text"] = "the %s player does not end in %s h" % [idle_id if not idle["ended"] else main_id, _f(max_hours)]
			m6["value"] = BenchJson.sig(float(idle["length_s"]) / length_s)
			m6["status"] = FAIL
		else:
			var ratio: float = float(idle["length_s"]) / length_s
			m6["value"] = BenchJson.sig(ratio)
			m6["text"] = "the %s player needs %s x the time of the %s player" % [idle_id, _f(ratio), main_id]
			var states6: Array = [PASS]
			if t6.has("min"):
				states6.append(at_least(ratio, float(t6["min"]), _tol(t6)))
			if t6.has("max"):
				states6.append(at_most(ratio, float(t6["max"]), _tol(t6)))
			m6["status"] = worst(states6)
		var idle_seconds: Dictionary = _phase_seconds(idle)
		if not idle["ended"] and purchase_count(idle) == 0:
			idle_seconds = {}
		for id: Variant in seconds:
			if idle_seconds.has(id) and float(seconds[id]) > 0.0:
				(m6["phases"] as Dictionary)[id] = BenchJson.sig(float(idle_seconds[id]) / float(seconds[id]))
	var hint6: Array = Knobs.hint(ctx, ["active_power", "active_time", "offline"])
	hint6.append_array(Knobs.player_hint(ctx, ["active_share", "decision_s"]))
	m6["hint"] = hint6
	meters.append(m6)

	# M7 Reset rhythm.
	var m7: Dictionary = _meter("M7", "Reset rhythm", "resets")
	var t7: Dictionary = _target(targets, "resets")
	var has_reset: bool = false
	for id: Variant in main["actions"]:
		if str((main["actions"][id] as Dictionary)["kind"]) == "reset":
			has_reset = true
	var reset_times: Array = main["resets"]
	if not has_reset:
		m7["text"] = "the game showed no reset action"
	else:
		m7["value"] = reset_times.size()
		if reset_times.is_empty():
			m7["text"] = "0 resets"
		else:
			var runs: PackedStringArray = PackedStringArray()
			var last: float = 0.0
			for t: Variant in reset_times:
				runs.append(_f((float(t) - last) / 60.0, 0))
				last = float(t)
			runs.append(_f((length_s - last) / 60.0, 0))
			m7["text"] = "%d resets; first at minute %s; runs of %s min" % [reset_times.size(), _f(float(reset_times[0]) / 60.0, 1), ", ".join(runs)]
		var parts7: PackedStringArray = PackedStringArray()
		var states7: Array = [PASS]
		if t7.has("count_min"):
			parts7.append("at least %s resets" % _f(float(t7["count_min"])))
			states7.append(at_least(float(reset_times.size()), float(t7["count_min"]), _tol(t7)))
		if t7.has("count_max"):
			parts7.append("at most %s resets" % _f(float(t7["count_max"])))
			states7.append(at_most(float(reset_times.size()), float(t7["count_max"]), _tol(t7)))
		if t7.has("first_min_max"):
			parts7.append("first reset before minute %s" % _f(float(t7["first_min_max"])))
			if reset_times.is_empty():
				states7.append(FAIL)
			else:
				states7.append(at_most(float(reset_times[0]) / 60.0, float(t7["first_min_max"]), _tol(t7)))
		if not parts7.is_empty():
			m7["target"] = "; ".join(parts7)
		m7["status"] = worst(states7)
		for phase: Array in phases:
			var count: int = 0
			for t: Variant in reset_times:
				if float(t) > float(phase[1]) and float(t) <= float(phase[2]):
					count += 1
			(m7["phases"] as Dictionary)[phase[0]] = count
	var hint7: Array = Knobs.hint(ctx, ["reset_need", "reset_gain", "reset_bonus"])
	hint7.append_array(Knobs.player_hint(ctx, ["reset_gain"]))
	m7["hint"] = hint7
	meters.append(m7)

	# M8 Waits and dead content.
	var m8: Dictionary = _meter("M8", "Waits and dead content", "share")
	var t8: Dictionary = _target(targets, "wait_share_max")
	var longest_target: float = float(t2.get("longest_max", 2.0))
	var limit_s: float = 2.0 * longest_target * 60.0
	var edges: Array = [0.0]
	edges.append_array(moments)
	edges.append(length_s)
	var waited: float = 0.0
	var waited_by_phase: Dictionary = {}
	for index: int in range(1, edges.size()):
		var gap_s: float = float(edges[index]) - float(edges[index - 1])
		if gap_s > limit_s:
			waited += gap_s
			var phase_id: String = _phase_of(phases, float(edges[index]))
			waited_by_phase[phase_id] = float(waited_by_phase.get(phase_id, 0.0)) + gap_s
	var wait_share: float = waited / length_s
	var unused: Array = []
	for id: Variant in main["actions"]:
		var used: Dictionary = main["actions"][id]
		# A purchase that the game made by itself is a use of the action. A free choice of the player
		# (`"bot": false`) is not for the bot: it is never "never used".
		if (used["times"] as Array).is_empty() and int(used.get("auto_count", 0)) == 0 and used.get("bot", true):
			unused.append(id)
	unused.sort()
	var dead_text: String = "numbers with no effect: not checked"
	var dead_count: int = -1
	var dead_ids: Array = []
	var check_state: String = "not checked"
	if check.get("dead", null) is Array:
		if check.has("ledger_hash") and str(check["ledger_hash"]) != Knobs.ledger_hash(ctx):
			# The numbers of the ledger changed after the self-check ran.
			check_state = "stale"
			dead_text = "numbers with no effect: self-check is stale"
		else:
			check_state = "partial" if check.get("partial", false) else "checked"
			dead_count = 0
			var ledger: Dictionary = ctx["numbers"]
			for entry: Variant in check["dead"]:
				if not entry is Dictionary:
					continue
				var dead_id: String = str((entry as Dictionary).get("id", ""))
				# The reason `dead_ok` of the bench file of now counts, not the one of the time of the check.
				var accepted: bool = (entry as Dictionary).has("dead_ok")
				if ledger.has(dead_id):
					accepted = (ledger[dead_id] as Dictionary).has("dead_ok")
				if accepted:
					continue
				dead_count += 1
				dead_ids.append(dead_id)
				if dead_count <= MAX_DEAD_LINES:
					(m8["lines"] as Array).append("no effect: %s (%s)" % [(entry as Dictionary).get("label", dead_id), dead_id])
			if dead_count > MAX_DEAD_LINES:
				(m8["lines"] as Array).append("and %d more numbers with no effect" % (dead_count - MAX_DEAD_LINES))
			dead_text = "%d numbers with no effect" % dead_count
			if check_state == "partial":
				dead_text += " (self-check partial: %d of %d numbers checked)" % [(check.get("checked", []) as Array).size(), (ctx["tunable"] as Array).size()]
	m8["value"] = BenchJson.sig(wait_share)
	m8["text"] = "%s of the time in a wait above %s min; %d actions never used; %s" % [_pct(wait_share), _f(limit_s / 60.0), unused.size(), dead_text]
	m8["unused"] = unused
	m8["dead_numbers"] = dead_count
	m8["dead_ids"] = dead_ids
	m8["selfcheck"] = check_state
	var states8: Array = [PASS]
	var parts8: PackedStringArray = PackedStringArray()
	if t8.has("about"):
		var limit8: float = float(t8["about"])
		parts8.append("waits at most about %s (range to %s)" % [_pct(limit8), _pct(limit8 * (1.0 + _tol(t8)))])
		states8.append(at_most(wait_share, limit8, _tol(t8)))
	parts8.append("0 actions never used; 0 numbers with no effect")
	if not unused.is_empty() or dead_count > 0:
		states8.append(FAIL)
	m8["target"] = "; ".join(parts8)
	m8["status"] = worst(states8)
	for phase: Array in phases:
		var span: float = float(phase[2]) - float(phase[1])
		if span > 0.0:
			(m8["phases"] as Dictionary)[phase[0]] = BenchJson.sig(float(waited_by_phase.get(phase[0], 0.0)) / span)
	m8["hint"] = Knobs.hint(ctx, ["cost_growth", "wall", "unlock_at"])
	meters.append(m8)

	# The table of the actions.
	var actions_out: Array = []
	var ids: Array = (main["actions"] as Dictionary).keys()
	var all_times: Dictionary = {}
	for id: Variant in ids:
		# The times of the purchases of the bot and of the game together, in order, each time 1 time.
		var both: Dictionary = {}
		for t: Variant in main["actions"][id]["times"]:
			both[float(t)] = true
		for t: Variant in (main["actions"][id] as Dictionary).get("auto_times", []):
			both[float(t)] = true
		var merged: Array = both.keys()
		merged.sort()
		all_times[id] = merged
	ids.sort_custom(func(a: Variant, b: Variant) -> bool:
		var first_a: float = float((all_times[a] as Array)[0]) if not (all_times[a] as Array).is_empty() else 1.0e18
		var first_b: float = float((all_times[b] as Array)[0]) if not (all_times[b] as Array).is_empty() else 1.0e18
		if first_a != first_b:
			return first_a < first_b
		return str(a) < str(b))
	for id: Variant in ids:
		var row: Dictionary = main["actions"][id]
		var times: Array = all_times[id]
		var by_hand: int = (row["times"] as Array).size()
		var by_game: int = int(row.get("auto_count", 0))
		var waits: Array = []
		for index: int in range(1, times.size()):
			waits.append(float(times[index]) - float(times[index - 1]))
		var changing: Array = []
		for gain: Variant in row["gains"]:
			if float(gain) != 0.0:
				changing.append(gain)
		var impact: String = "not used"
		if not row.get("bot", true):
			impact = "choice"
		elif not times.is_empty():
			if str(row["kind"]) == "reset" or str(row["kind"]) == "advance":
				impact = str(row["kind"])
			elif by_hand == 0:
				impact = "auto (not measured)"
			elif changing.is_empty():
				impact = "no rate change"
			else:
				impact = impact_class(median(changing), t4)
		actions_out.append({"id": id, "kind": row["kind"], "label": row["label"], "element": row["element"],
			"seen_s": row["seen_s"],
			"first_s": times[0] if not times.is_empty() else null, "count": by_hand + by_game, "auto_count": by_game,
			"median_wait_s": BenchJson.ms(median(waits)) if not waits.is_empty() else null,
			"impact": impact, "median_gain": BenchJson.sig(median(changing)) if not changing.is_empty() else null})
		if not row.get("bot", true):
			(actions_out[actions_out.size() - 1] as Dictionary)["bot"] = false

	var fails: Array = []
	for meter: Dictionary in meters:
		if meter["status"] == FAIL:
			fails.append(meter["id"])
	var phase_names: Dictionary = {}
	for entry: Variant in bench.get("phases", []):
		if entry is Dictionary:
			phase_names[str((entry as Dictionary).get("id", ""))] = str((entry as Dictionary).get("name", ""))
	var result: Dictionary = {"format": "tuning-bench-result", "version": 1, "game": str(bench.get("game", "")), "k": k,
		"main_player": main_id, "players": players_out, "meters": meters, "failed": fails, "actions": actions_out,
		"phase_names": phase_names,
		"timeline": {"purchases": main["purchases"], "auto": main.get("auto", []), "content": main["content"],
			"phases": main["phases"], "rate": main["rate"], "advances": main["advances"], "resets": main["resets"],
			"events": main["events"], "aways": main.get("aways", []), "length_s": main["length_s"]}}
	if not pending.is_empty():
		result["pending_players"] = pending
	return result


static func _longest_gap(times: Array, start: float, end: float) -> float:
	var longest: float = 0.0
	var last: float = start
	for t: Variant in times:
		longest = maxf(longest, float(t) - last)
		last = float(t)
	return maxf(longest, end - last)
