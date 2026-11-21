extends RefCounted

const BenchJson := preload("res://addons/tuning_bench/bench_json.gd")

const DEFAULT_BENCH := "res://bench/bench.json"
const OPS: Array[String] = ["mul", "growth", "add", "set", "role"]
const FACTOR_ROLE := "effect_factor"
const GROWTH_SUFFIX := "_growth"
const ALL_NUMBERS := "All numbers"
const PLAYER_FIELDS: Array[String] = ["decision_s", "active_share", "reset_gain", "stuck_s", "away_every_min", "away_min"]
const DEFAULT_SELFCHECK := "res://bench/selfcheck.json"


static func is_growth_role(role: String) -> bool:
	return role == FACTOR_ROLE or role.ends_with(GROWTH_SUFFIX)


static func role_op(role: String) -> String:
	return "growth" if is_growth_role(role) else "mul"


static func sense_of(number: Dictionary) -> int:
	var raw: Variant = number.get("sense", 1)
	if (raw is int or raw is float) and float(raw) < 0.0:
		return -1
	return 1


static func ledger_hash(ctx: Dictionary) -> String:
	var base: Dictionary = ctx["base"]
	var lines: PackedStringArray = PackedStringArray()
	for id: Variant in ctx["tunable"]:
		lines.append("%s=%s" % [id, JSON.stringify(base[id])])
	return ("\n".join(lines) + "\n").sha256_text()


static func build(bench_path: String = DEFAULT_BENCH) -> Dictionary:
	var ctx: Dictionary = {"ok": false, "error": "", "path": bench_path, "bench": {}, "data": {}, "base": {},
		"numbers": {}, "tunable": [], "controls": [], "by_id": {}, "missing": [], "foreign": []}
	var read: Dictionary = BenchJson.read_file(bench_path)
	if not read["ok"]:
		ctx["error"] = read["error"]
		return ctx
	if not read["value"] is Dictionary:
		ctx["error"] = "%s: the content is not a JSON object" % bench_path
		return ctx
	var bench: Dictionary = read["value"]
	if str(bench.get("format", "")) != "tuning-bench":
		ctx["error"] = "%s: the key \"format\" is not \"tuning-bench\"" % bench_path
		return ctx
	for key: String in ["sources", "numbers", "phases", "controls", "players"]:
		if not bench.get(key, []) is Array:
			ctx["error"] = "%s: the key \"%s\" is not a list" % [bench_path, key]
			return ctx
	if (bench.get("players", []) as Array).is_empty():
		ctx["error"] = "%s: the list \"players\" is empty" % bench_path
		return ctx
	if (bench.get("phases", []) as Array).is_empty():
		ctx["error"] = "%s: the list \"phases\" is empty" % bench_path
		return ctx
	ctx["bench"] = bench

	var data: Dictionary = {}
	for source: Variant in bench.get("sources", []):
		var file: String = str(source)
		if not file.to_lower().ends_with(".json"):
			ctx["error"] = "%s: version 1 of the bench reads JSON sources only" % file
			return ctx
		var parsed: Dictionary = BenchJson.read_file(file)
		if not parsed["ok"]:
			ctx["error"] = parsed["error"]
			return ctx
		data[file] = parsed["value"]
	ctx["data"] = data

	var numbers: Dictionary = {}
	var base: Dictionary = {}
	var tunable: Array = []
	var missing: Array = []
	var foreign: Array = []
	for entry: Variant in bench.get("numbers", []):
		if not entry is Dictionary:
			continue
		var number: Dictionary = entry
		var id: String = str(number.get("id", ""))
		if id == "":
			continue
		numbers[id] = number
		if not is_tunable(number):
			continue
		var file: String = str(number.get("file", ""))
		if not data.has(file):
			foreign.append(id)
			continue
		var found: Array = BenchJson.pointer_get(data[file], str(number.get("pointer", "")))
		if not found[0] or not (found[1] is int or found[1] is float):
			missing.append(id)
			continue
		base[id] = found[1]
		tunable.append(id)
	ctx["numbers"] = numbers
	ctx["base"] = base
	ctx["tunable"] = tunable
	ctx["missing"] = missing
	ctx["foreign"] = foreign

	var controls: Array = []
	var by_id: Dictionary = {}
	var own: Dictionary = {}
	for entry: Variant in bench.get("controls", []):
		if not entry is Dictionary:
			continue
		var control: Dictionary = _control(entry, ctx)
		if control.is_empty():
			ctx["error"] = "%s: a control has no id or an op that is not known (%s)" % [bench_path, JSON.stringify(entry)]
			return ctx
		if by_id.has(control["id"]):
			ctx["error"] = "%s: 2 controls have the id %s" % [bench_path, control["id"]]
			return ctx
		controls.append(control)
		by_id[control["id"]] = control
		var targets: Array = control["targets"]
		if control["op"] == "set" and targets.size() == 1:
			own[targets[0]] = true
	for id: Variant in tunable:
		if own.has(id):
			continue
		var number: Dictionary = numbers[id]
		var made: Dictionary = _control({"id": "n:" + str(id), "group": ALL_NUMBERS,
			"name": str(number.get("label", id)), "op": "set", "select": {"id": [id]}}, ctx)
		made["generated"] = true
		controls.append(made)
		by_id[made["id"]] = made
	ctx["controls"] = controls
	ctx["by_id"] = by_id
	ctx["ok"] = true
	return ctx


static func is_tunable(number: Dictionary) -> bool:
	if str(number.get("class", "")) != "balance":
		return false
	if number.get("tunable", true) == false:
		return false
	return number.has("pointer")


static func _control(entry: Dictionary, ctx: Dictionary) -> Dictionary:
	var id: String = str(entry.get("id", ""))
	var op: String = str(entry.get("op", "mul"))
	if id == "" or not OPS.has(op):
		return {}
	var control: Dictionary = {"id": id, "group": str(entry.get("group", "Other")), "name": str(entry.get("name", id)),
		"op": op, "targets": [], "kind": "numbers", "generated": false, "element": "", "select": {}}
	var low: float = 0.25
	var high: float = 4.0
	var start: float = 1.0
	if entry.get("player", null) is Dictionary:
		control["kind"] = "player"
		control["player"] = entry["player"]
		if op == "add":
			low = -1.0
			high = 1.0
			start = 0.0
	else:
		var select: Dictionary = {}
		if entry.get("select", null) is Dictionary:
			select = entry["select"]
		control["select"] = select
		var numbers: Dictionary = ctx["numbers"]
		var targets: Array = []
		for number_id: Variant in ctx["tunable"]:
			if _matches(numbers[number_id], select):
				targets.append(number_id)
		control["targets"] = targets
		if op == "add":
			low = -1.0
			high = 1.0
			start = 0.0
		elif op == "set":
			var value: float = 0.0
			var growth: bool = false
			if not targets.is_empty():
				var base: Dictionary = ctx["base"]
				value = float(base[targets[0]])
				var first: Dictionary = numbers[targets[0]]
				growth = is_growth_role(str(first.get("role", "")))
				control["element"] = str(first.get("element", ""))
			start = value
			if growth and value > 1.0:
				low = 1.0 + (value - 1.0) * 0.25
				high = 1.0 + (value - 1.0) * 4.0
			elif value > 0.0:
				low = value * 0.25
				high = value * 4.0
			elif value < 0.0:
				low = value * 4.0
				high = value * 0.25
			else:
				low = 0.0
				high = 1.0
	if entry.has("min"):
		low = float(entry["min"])
	if entry.has("max"):
		high = float(entry["max"])
	control["min"] = low
	control["max"] = high
	control["base"] = start
	return control


static func _matches(number: Dictionary, select: Dictionary) -> bool:
	if select.is_empty():
		return false
	for key: String in ["role", "phase", "element", "id"]:
		if not select.has(key):
			continue
		var allowed: Variant = select[key]
		var value: String = str(number.get(key, ""))
		if allowed is Array:
			if not (allowed as Array).has(value):
				return false
		elif str(allowed) != value:
			return false
	return true


static func base_k(ctx: Dictionary) -> Dictionary:
	var out: Dictionary = {}
	for control: Dictionary in ctx["controls"]:
		out[control["id"]] = control["base"]
	return out


static func changed_k(ctx: Dictionary, k: Dictionary) -> Dictionary:
	var out: Dictionary = {}
	var by_id: Dictionary = ctx["by_id"]
	for id: Variant in k:
		if by_id.has(id) and float(k[id]) != float((by_id[id] as Dictionary)["base"]):
			out[id] = float(k[id])
	return out


static func report_k(ctx: Dictionary, k: Dictionary) -> Dictionary:
	var out: Dictionary = {}
	for control: Dictionary in ctx["controls"]:
		var value: float = float(k.get(control["id"], control["base"]))
		if not control["generated"] or value != float(control["base"]):
			out[control["id"]] = value
	return out


static func _apply_op(op: String, value: float, k: float) -> float:
	match op:
		"mul":
			return value * k
		"growth":
			return 1.0 + (value - 1.0) * k
		"add":
			return value + k
		"set":
			return k
	return value


static func apply_to_number(op: String, number: Dictionary, value: float, k: float) -> float:
	var real_op: String = role_op(str(number.get("role", ""))) if op == "role" else op
	if sense_of(number) < 0:
		match real_op:
			"mul", "growth":
				if k <= 0.0:
					return value
				return _apply_op(real_op, value, 1.0 / k)
			"add":
				return _apply_op(real_op, value, -k)
	return _apply_op(real_op, value, k)


static func values(ctx: Dictionary, k: Dictionary) -> Dictionary:
	var base: Dictionary = ctx["base"]
	var numbers: Dictionary = ctx["numbers"]
	var work: Dictionary = {}
	for control: Dictionary in ctx["controls"]:
		if control["kind"] != "numbers":
			continue
		var kk: float = float(k.get(control["id"], control["base"]))
		if kk == float(control["base"]):
			continue
		for id: Variant in control["targets"]:
			var now: float = float(work.get(id, base[id]))
			work[id] = apply_to_number(control["op"], numbers[id], now, kk)
	var out: Dictionary = base.duplicate()
	for id: Variant in work:
		var value: float = work[id]
		if base[id] is int:
			var whole: int = roundi(value)
			if str((numbers[id] as Dictionary).get("role", "")) == "cap":
				whole = maxi(whole, 1)
			out[id] = whole
		else:
			out[id] = value
	return out


static func apply(ctx: Dictionary, vals: Dictionary) -> Dictionary:
	var data: Dictionary = (ctx["data"] as Dictionary).duplicate(true)
	var base: Dictionary = ctx["base"]
	var numbers: Dictionary = ctx["numbers"]
	for id: Variant in vals:
		if not base.has(id) or (vals[id] == base[id] and typeof(vals[id]) == typeof(base[id])):
			continue
		var number: Dictionary = numbers[id]
		BenchJson.pointer_set(data[str(number["file"])], str(number["pointer"]), vals[id])
	return data


static func data_for(ctx: Dictionary, k: Dictionary) -> Dictionary:
	return apply(ctx, values(ctx, k))


static func players(ctx: Dictionary, k: Dictionary) -> Array:
	var out: Array = []
	for entry: Variant in (ctx["bench"] as Dictionary).get("players", []):
		if entry is Dictionary:
			out.append((entry as Dictionary).duplicate(true))
	for control: Dictionary in ctx["controls"]:
		if control["kind"] != "player":
			continue
		var kk: float = float(k.get(control["id"], control["base"]))
		if kk == float(control["base"]):
			continue
		var spec: Dictionary = control["player"]
		var field: String = str(spec.get("field", ""))
		if not PLAYER_FIELDS.has(field):
			continue
		for player: Dictionary in out:
			if spec.has("ids") and not (spec["ids"] as Array).has(player.get("id", "")):
				continue
			var player_op: String = "mul" if control["op"] == "role" else str(control["op"])
			var value: float = _apply_op(player_op, float(player.get(field, 0.0)), kk)
			if field == "active_share":
				value = clampf(value, 0.0, 1.0)
			elif field == "decision_s":
				value = maxf(value, 0.05)
			player[field] = value
	return out


static func targets(ctx: Dictionary, changes: Dictionary) -> Dictionary:
	var out: Dictionary = {}
	var source: Variant = (ctx["bench"] as Dictionary).get("targets", {})
	if source is Dictionary:
		out = (source as Dictionary).duplicate(true)
	for key: Variant in changes:
		if changes[key] is Dictionary and out.get(key, null) is Dictionary:
			for sub: Variant in changes[key]:
				(out[key] as Dictionary)[sub] = changes[key][sub]
		else:
			out[key] = changes[key]
	return out


static func hint(ctx: Dictionary, roles: Array) -> Array:
	var out: Array = []
	for control: Dictionary in ctx["controls"]:
		if control["kind"] != "numbers" or control["generated"]:
			continue
		var select: Dictionary = control["select"]
		if not select.has("role") or select.has("phase") or select.has("element") or select.has("id"):
			continue
		if (control["targets"] as Array).is_empty():
			continue
		for role: Variant in select["role"]:
			if roles.has(role):
				out.append(control["id"])
				break
	return out


static func player_hint(ctx: Dictionary, fields: Array) -> Array:
	var out: Array = []
	for control: Dictionary in ctx["controls"]:
		if control["kind"] == "player" and fields.has(str((control["player"] as Dictionary).get("field", ""))):
			out.append(control["id"])
	return out


static func read_setting(ctx: Dictionary, path: String) -> Dictionary:
	var read: Dictionary = BenchJson.read_file(path)
	if not read["ok"]:
		return {"ok": false, "error": read["error"], "k": {}, "targets": {}}
	if not read["value"] is Dictionary:
		return {"ok": false, "error": "%s: the content is not a JSON object" % path, "k": {}, "targets": {}}
	var setting: Dictionary = read["value"]
	var k: Dictionary = {}
	var raw: Variant = setting.get("k", {})
	if raw is Dictionary:
		for id: Variant in raw:
			if not (ctx["by_id"] as Dictionary).has(id):
				return {"ok": false, "error": "%s: the control %s is not in the bench file" % [path, id], "k": {}, "targets": {}}
			k[id] = float(raw[id])
	var changes: Dictionary = {}
	if setting.get("targets", null) is Dictionary:
		changes = setting["targets"]
	return {"ok": true, "error": "", "k": k, "targets": changes}


static func setting(ctx: Dictionary, k: Dictionary, target_changes: Dictionary) -> Dictionary:
	return {"format": "tuning-bench-setting", "version": 1, "game": str((ctx["bench"] as Dictionary).get("game", "")),
		"k": changed_k(ctx, k), "targets": target_changes}


static func control_of_number(ctx: Dictionary, number_id: String) -> String:
	for control: Dictionary in ctx["controls"]:
		if control["op"] == "set" and control["kind"] == "numbers":
			var targets: Array = control["targets"]
			if targets.size() == 1 and str(targets[0]) == number_id:
				return control["id"]
	return ""
