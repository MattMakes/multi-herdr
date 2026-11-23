class_name GameCore
extends RefCounted

const GENERATORS_FILE := "res://data/generators.json"
const UPGRADES_FILE := "res://data/upgrades.json"
const STAGES_FILE := "res://data/stages.json"
const ECONOMY_FILE := "res://data/economy.json"

const STAGE_INCOME_BONUS := 0.1
const EMBER_START_LIGHT := 5.0
const CLICKS_PER_S := 3.0
const AUTO_BUYS_PER_STEP := 1000

const NEVER := 1.0e300
const AUTO_ID := "lamplighter"

const RNG_MOD := 2147483648


static func _generators(data: Dictionary) -> Array:
	return data[GENERATORS_FILE]["generators"]


static func _upgrades(data: Dictionary) -> Array:
	return data[UPGRADES_FILE]["upgrades"]


static func _stages(data: Dictionary) -> Array:
	return data[STAGES_FILE]["stages"]


static func _economy(data: Dictionary) -> Dictionary:
	return data[ECONOMY_FILE]


static func new_state(data: Dictionary, seed: int = 1) -> Dictionary:
	var state: Dictionary = {"seed": seed, "rng": absi(seed) % RNG_MOD, "light": 0.0, "run_earned": 0.0,
		"lifetime": 0.0, "levels": {}, "upgrades": {}, "stage": 0, "embers": 0, "resets": 0, "seen": {},
		"mile_mark": 0.0, "watch": 0.0, "auto_off": false}
	for generator: Dictionary in _generators(data):
		state["levels"][generator["id"]] = 0
	_unlocks(state, data)
	return state


static func refresh(state: Dictionary, data: Dictionary) -> Array:
	for generator: Dictionary in _generators(data):
		if not (state["levels"] as Dictionary).has(generator["id"]):
			state["levels"][generator["id"]] = 0
	state["watch"] = 0.0
	return _unlocks(state, data)


static func step(state: Dictionary, data: Dictionary, dt: float) -> Array:
	var events: Array = []
	var left: float = dt
	var target: Dictionary = _auto_target(state, data)
	var bought: int = 0
	while not target.is_empty() and bought < AUTO_BUYS_PER_STEP:
		var id: String = target["id"]
		var level: int = int(state["levels"].get(id, 0))
		var cost: float = generator_cost(target, level)
		var short: float = cost - float(state["light"])
		if short > 0.0:
			var income: float = rate(state, data)
			if income <= 0.0 or short > income * left:
				break
			_earn(state, short)
			left -= short / income
		state["light"] = maxf(float(state["light"]) - cost, 0.0)
		state["levels"][id] = level + 1
		bought += 1
		events.append({"kind": "bought", "id": id, "label": str(target["name"]), "cost": cost, "level": level + 1, "auto": true})
	_earn(state, rate(state, data) * maxf(left, 0.0))
	if float(state["lifetime"]) >= float(state["watch"]):
		events.append_array(_unlocks(state, data))
	return events


static func away(state: Dictionary, data: Dictionary, seconds: float) -> Array:
	var offline: Dictionary = _economy(data)["offline"]
	var counted: float = clampf(seconds, 0.0, float(offline["cap_minutes"]) * 60.0)
	var amount: float = rate(state, data) * float(offline["rate"]) * counted
	_earn(state, amount)
	var events: Array = [{"kind": "reward", "id": "offline", "label": "Light from the time away", "amount": amount}]
	if float(state["lifetime"]) >= float(state["watch"]):
		events.append_array(_unlocks(state, data))
	return events


static func _auto_target(state: Dictionary, data: Dictionary) -> Dictionary:
	var auto: Dictionary = _economy(data)["auto_buy"]
	if not (state["seen"] as Dictionary).has(AUTO_ID) or state.get("auto_off", false):
		return {}
	for generator: Dictionary in _generators(data):
		if generator["id"] == auto["target"]:
			return generator
	return {}


static func active(state: Dictionary, data: Dictionary, seconds: float) -> Array:
	_earn(state, click_value(state, data) * CLICKS_PER_S * seconds)
	if float(state["lifetime"]) >= float(state["watch"]):
		return _unlocks(state, data)
	return []


static func click_value(state: Dictionary, data: Dictionary) -> float:
	var click: Dictionary = _economy(data)["click"]
	var factor: float = 1.0
	for upgrade: Dictionary in _upgrades(data):
		if upgrade["target"] == "click" and (state["upgrades"] as Dictionary).has(upgrade["id"]):
			factor *= float(upgrade["mult"])
	return float(click["power"]) * factor + float(click["rate_seconds"]) * rate(state, data)


static func generator_output(generator: Dictionary, level: int) -> float:
	if level <= 0:
		return 0.0
	var growth: float = float(generator["level_growth"])
	if absf(growth - 1.0) < 1.0e-9:
		return float(generator["income"]) * float(level)
	return float(generator["income"]) * (pow(growth, float(level)) - 1.0) / (growth - 1.0)


static func generator_cost(generator: Dictionary, level: int) -> float:
	return float(generator["cost"]) * pow(float(generator["cost_growth"]), float(level))


static func rate(state: Dictionary, data: Dictionary) -> float:
	var economy: Dictionary = _economy(data)
	var owned: Dictionary = state["upgrades"]
	var factors: Dictionary = {}
	var all: float = 1.0
	for upgrade: Dictionary in _upgrades(data):
		if not owned.has(upgrade["id"]):
			continue
		var target: String = upgrade["target"]
		if target == "all":
			all *= float(upgrade["mult"])
		elif target != "click":
			factors[target] = float(factors.get(target, 1.0)) * float(upgrade["mult"])
	var total: float = float(economy["base_income"])
	var levels: Dictionary = state["levels"]
	for generator: Dictionary in _generators(data):
		var level: int = int(levels.get(generator["id"], 0))
		if level > 0:
			total += generator_output(generator, level) * float(factors.get(generator["id"], 1.0))
	total *= all
	total *= 1.0 + float(int(state["embers"])) * float(economy["reset"]["bonus_per_ember"])
	total *= 1.0 + STAGE_INCOME_BONUS * float(int(state["stage"]))
	return total


static func progress(state: Dictionary, data: Dictionary) -> float:
	var count: int = _stages(data).size()
	if count == 0:
		return 1.0
	return clampf(float(int(state["stage"])) / float(count), 0.0, 1.0)


static func wealth(state: Dictionary, _data: Dictionary) -> float:
	return float(state["light"])


static func max_step(_data: Dictionary) -> float:
	return 3600.0


static func reset_embers(state: Dictionary, data: Dictionary) -> int:
	var reset: Dictionary = _economy(data)["reset"]
	if float(state["run_earned"]) < float(reset["need_light"]):
		return 0
	return int(floor(pow(float(state["run_earned"]) / float(reset["gain_div"]), float(reset["gain_exp"]))))


static func actions(state: Dictionary, data: Dictionary) -> Array:
	var out: Array = []
	var seen: Dictionary = state["seen"]
	var light: float = state["light"]
	var levels: Dictionary = state["levels"]
	for generator: Dictionary in _generators(data):
		var id: String = generator["id"]
		if not seen.has(id):
			continue
		var level: int = int(levels.get(id, 0))
		var cost: float = generator_cost(generator, level)
		out.append({"id": id, "kind": "buy", "label": str(generator["name"]), "cost": cost,
			"affordable": light >= cost, "element": id, "group": "Lamps", "level": level})
	var owned: Dictionary = state["upgrades"]
	for upgrade: Dictionary in _upgrades(data):
		var id: String = upgrade["id"]
		if not seen.has(id) or owned.has(id):
			continue
		var cost: float = float(upgrade["cost"])
		out.append({"id": id, "kind": "once", "label": str(upgrade["name"]), "cost": cost, "affordable": light >= cost,
			"element": id, "group": "Upgrades"})
	var stages: Array = _stages(data)
	var at: int = int(state["stage"])
	if at < stages.size():
		var stage: Dictionary = stages[at]
		if seen.has(stage["id"]):
			var need: float = float(stage["need"])
			out.append({"id": stage["id"], "kind": "advance", "label": "Light %s" % stage["name"], "cost": need,
				"affordable": light >= need, "element": stage["id"], "group": "Road"})
	if seen.has("reset"):
		var gives: int = reset_embers(state, data)
		out.append({"id": "reset", "kind": "reset", "label": "New night", "cost": 0.0,
			"affordable": gives >= 1, "element": "reset", "group": "Road", "embers": gives,
			"gain": float(gives) / float(maxi(int(state["embers"]), 1))})
	if seen.has(AUTO_ID):
		var on: bool = not state.get("auto_off", false)
		out.append({"id": AUTO_ID, "kind": "other", "label": "Lamplighter: %s" % ("on" if on else "off"), "cost": 0.0,
			"affordable": true, "element": AUTO_ID, "group": "Road", "bot": false, "on": on})
	return out


static func apply(state: Dictionary, data: Dictionary, action_id: String) -> Array:
	var seen: Dictionary = state["seen"]
	if not seen.has(action_id):
		return [_refused(action_id, "not unlocked")]
	var light: float = state["light"]
	if action_id == AUTO_ID:
		state["auto_off"] = not state.get("auto_off", false)
		return [{"kind": "switch", "id": AUTO_ID, "label": "The lamplighter %s" % ("rests" if state["auto_off"] else "works"),
			"on": not state["auto_off"]}]
	if action_id == "reset":
		var gives: int = reset_embers(state, data)
		if gives < 1:
			return [_refused(action_id, "not enough light in this night")]
		var events: Array = [{"kind": "reset", "id": "reset", "label": "New night", "embers": gives}]
		if int(state["embers"]) == 0:
			events.append({"kind": "unlock", "id": "embers", "label": "Embers"})
		state["embers"] = int(state["embers"]) + gives
		state["resets"] = int(state["resets"]) + 1
		state["light"] = EMBER_START_LIGHT * float(int(state["embers"]))
		state["run_earned"] = 0.0
		state["upgrades"] = {}
		for id: Variant in state["levels"]:
			state["levels"][id] = 0
		return events
	for generator: Dictionary in _generators(data):
		if generator["id"] != action_id:
			continue
		var level: int = int(state["levels"].get(action_id, 0))
		var cost: float = generator_cost(generator, level)
		if light < cost:
			return [_refused(action_id, "not enough light")]
		state["light"] = light - cost
		state["levels"][action_id] = level + 1
		return [{"kind": "bought", "id": action_id, "label": str(generator["name"]), "cost": cost, "level": level + 1}]
	for upgrade: Dictionary in _upgrades(data):
		if upgrade["id"] != action_id:
			continue
		if (state["upgrades"] as Dictionary).has(action_id):
			return [_refused(action_id, "already bought")]
		var cost: float = float(upgrade["cost"])
		if light < cost:
			return [_refused(action_id, "not enough light")]
		state["light"] = light - cost
		state["upgrades"][action_id] = true
		return [{"kind": "bought", "id": action_id, "label": str(upgrade["name"]), "cost": cost, "level": 1}]
	var stages: Array = _stages(data)
	var at: int = int(state["stage"])
	if at < stages.size() and stages[at]["id"] == action_id:
		var stage: Dictionary = stages[at]
		var need: float = float(stage["need"])
		if light < need:
			return [_refused(action_id, "not enough light")]
		var spread: float = float(data[STAGES_FILE]["reward_spread"])
		var amount: float = float(stage["reward"]) * (1.0 - spread + 2.0 * spread * _random(state))
		state["light"] = light - need
		state["stage"] = at + 1
		var events: Array = [{"kind": "advance", "id": action_id, "label": str(stage["name"]), "cost": need},
			{"kind": "reward", "id": action_id, "label": "Gift of %s" % stage["name"], "amount": amount}]
		_earn(state, amount)
		state["mile_mark"] = float(state["lifetime"])
		state["watch"] = 0.0
		events.append_array(_unlocks(state, data))
		return events
	return [_refused(action_id, "not an action")]


static func _refused(id: String, reason: String) -> Dictionary:
	return {"kind": "refused", "id": id, "label": reason}


static func _earn(state: Dictionary, amount: float) -> void:
	state["light"] = float(state["light"]) + amount
	state["run_earned"] = float(state["run_earned"]) + amount
	state["lifetime"] = float(state["lifetime"]) + amount


static func _random(state: Dictionary) -> float:
	var next: int = (int(state["rng"]) * 1103515245 + 12345) % RNG_MOD
	state["rng"] = next
	return float(next) / float(RNG_MOD)


static func _unlocks(state: Dictionary, data: Dictionary) -> Array:
	var events: Array = []
	var seen: Dictionary = state["seen"]
	var lifetime: float = state["lifetime"]
	var watch: float = NEVER
	for generator: Dictionary in _generators(data):
		watch = _unlock_one(seen, events, generator["id"], str(generator["name"]), float(generator["unlock_at"]), lifetime, watch)
	for upgrade: Dictionary in _upgrades(data):
		watch = _unlock_one(seen, events, upgrade["id"], str(upgrade["name"]), float(upgrade["unlock_at"]), lifetime, watch)
	var stages: Array = _stages(data)
	var at: int = int(state["stage"])
	if at < stages.size():
		var stage: Dictionary = stages[at]
		var need: float = float(state["mile_mark"]) + float(stage["unlock_at"])
		watch = _unlock_one(seen, events, stage["id"], str(stage["name"]), need, lifetime, watch)
	var reset: Dictionary = _economy(data)["reset"]
	watch = _unlock_one(seen, events, "reset", "New night", float(reset["unlock_at"]), lifetime, watch)
	var auto: Dictionary = _economy(data)["auto_buy"]
	watch = _unlock_one(seen, events, AUTO_ID, "Lamplighter", float(auto["unlock_at"]), lifetime, watch)
	state["watch"] = watch
	return events


static func _unlock_one(seen: Dictionary, events: Array, id: String, label: String, need: float, lifetime: float, watch: float) -> float:
	if seen.has(id):
		return watch
	if lifetime >= need:
		seen[id] = true
		events.append({"kind": "unlock", "id": id, "label": label})
		return watch
	return minf(watch, need)
