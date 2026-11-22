extends Node
## Data-driven progression state, purchases, stage simulation, and Regrow.

signal purchased(id: String, level: int)
signal stage_cleared(stage: int, reward: float)
signal upgrade_unlocked(id: String)
signal specimen_collected(id: String, stage: int)
signal network_tier_unlocked(id: String)
signal garden_unlocked_changed(unlocked: bool)
signal regrow_unlocked_changed(unlocked: bool)
signal prestiged(gain: int)
signal time_scale_changed(scale: float)

## Amount value for the bulk purchase functions: buy as many levels as the wallet pays for.
const BUY_MAX: int = -1
## Safety limit on the BUY_MAX loop when data/stages.json has no bulk_buy_max_levels key.
const DEFAULT_BULK_LIMIT: int = 10000

var spores: float = 0.0
var run_spores: float = 0.0
var stage: int = 0
var run_elapsed_seconds: float = 0.0
var highest_stage_reached: int = 0
var mycelium: int = 0
var lifetime_mycelium: int = 0
var levels: Dictionary = {}
var specimens_collected: Array[String] = []
var garden_unlocked: bool = false
var regrow_unlocked: bool = false
var time_scale: float = 1.0
var upgrades: Array = []
var rules: Dictionary = {}


func _ready() -> void:
	reset_all()


func reset_all() -> void:
	var parsed_upgrades: Variant = JSON.parse_string(FileAccess.get_file_as_string("res://data/upgrades.json"))
	var parsed_rules: Variant = JSON.parse_string(FileAccess.get_file_as_string("res://data/stages.json"))
	upgrades = parsed_upgrades as Array
	rules = parsed_rules as Dictionary
	spores = 0.0
	run_spores = 0.0
	stage = 0
	run_elapsed_seconds = 0.0
	highest_stage_reached = 0
	mycelium = 0
	lifetime_mycelium = 0
	levels.clear()
	for upgrade_value: Variant in upgrades:
		var upgrade: Dictionary = upgrade_value as Dictionary
		levels[String(upgrade["id"])] = 0
	specimens_collected.clear()
	garden_unlocked = false
	regrow_unlocked = false
	time_scale = 1.0


func step(delta: float) -> void:
	if delta <= 0.0:
		return
	var gain: float = spores_per_sec() * delta
	spores += gain
	run_spores += gain
	run_elapsed_seconds += delta
	_try_clear_stage()


func step_all(delta: float) -> void:
	var remaining: float = maxf(delta, 0.0)
	while remaining > 0.0:
		var chunk: float = minf(remaining, 1.0)
		Garden.step(chunk)
		step(chunk)
		remaining -= chunk


func set_time_scale(scale: float) -> void:
	var next_scale: float = maxf(scale, 0.0)
	if is_equal_approx(time_scale, next_scale):
		return
	time_scale = next_scale
	time_scale_changed.emit(time_scale)


func fast_forward(seconds: float) -> void:
	step_all(seconds)


func _process(delta: float) -> void:
	step_all(delta * time_scale)


func _try_clear_stage() -> void:
	while stage < max_stage() and grow_power() * float(rules["clear_seconds"]) >= stage_hp(stage + 1):
		stage += 1
		var reward: float = stage_reward(stage)
		spores += reward
		run_spores += reward
		var previous_highest: int = highest_stage_reached
		highest_stage_reached = maxi(highest_stage_reached, stage)
		if highest_stage_reached > previous_highest:
			for upgrade_value: Variant in upgrades:
				var upgrade: Dictionary = upgrade_value as Dictionary
				if String(upgrade["system"]) == "jar" and int(upgrade["unlock_stage"]) == highest_stage_reached:
					upgrade_unlocked.emit(String(upgrade["id"]))
			sync_permanent_unlocks()
		stage_cleared.emit(stage, reward)


func sync_permanent_unlocks() -> void:
	var should_unlock_garden: bool = highest_stage_reached >= int(rules["garden_unlock_stage"])
	if should_unlock_garden != garden_unlocked:
		garden_unlocked = should_unlock_garden
		garden_unlocked_changed.emit(garden_unlocked)
	var should_unlock_regrow: bool = highest_stage_reached >= int(rules["prestige_unlock_stage"])
	if should_unlock_regrow != regrow_unlocked:
		regrow_unlocked = should_unlock_regrow
		regrow_unlocked_changed.emit(regrow_unlocked)
	for specimen_value: Variant in get_specimens():
		var specimen: Dictionary = specimen_value as Dictionary
		var specimen_id: String = String(specimen["id"])
		var specimen_stage: int = int(specimen["unlock_stage"])
		if highest_stage_reached >= specimen_stage and not is_specimen_collected(specimen_id):
			specimens_collected.append(specimen_id)
			specimen_collected.emit(specimen_id, specimen_stage)


func get_upgrade(id: String) -> Dictionary:
	for upgrade_value: Variant in upgrades:
		var upgrade: Dictionary = upgrade_value as Dictionary
		if String(upgrade["id"]) == id:
			return upgrade
	return {}


func get_upgrades_for_system(system_id: String) -> Array:
	var result: Array = []
	for upgrade_value: Variant in upgrades:
		var upgrade: Dictionary = upgrade_value as Dictionary
		if String(upgrade["system"]) == system_id:
			result.append(upgrade)
	return result


func level(id: String) -> int:
	return int(levels.get(id, 0))


func effective_max_level(id: String) -> int:
	var upgrade: Dictionary = get_upgrade(id)
	if upgrade.is_empty() or upgrade.get("max_level") == null:
		return -1
	var maximum: int = int(upgrade["max_level"])
	for specimen_value: Variant in get_specimens():
		var specimen: Dictionary = specimen_value as Dictionary
		if not is_specimen_collected(String(specimen["id"])):
			continue
		for effect_value: Variant in specimen["effects"] as Array:
			var effect: Dictionary = effect_value as Dictionary
			if String(effect["target"]) == id and String(effect["kind"]) == "cap":
				maximum += int(effect["value"])
	if id == "compost":
		maximum += Garden.get_compost_cap_bonus()
	for modifier_value: Variant in upgrades:
		var modifier: Dictionary = modifier_value as Dictionary
		var modifier_effect: Dictionary = modifier["effect"] as Dictionary
		if String(modifier_effect["stat"]) == id and modifier_effect.has("cap"):
			maximum += int(modifier_effect["cap"]) * level(String(modifier["id"]))
	return maximum


func is_capped(id: String) -> bool:
	var maximum: int = effective_max_level(id)
	return maximum >= 0 and level(id) >= maximum


func unlock_stage(id: String) -> int:
	var upgrade: Dictionary = get_upgrade(id)
	return int(upgrade.get("unlock_stage", 0))


func unlock_lifetime_mycelium(id: String) -> int:
	var upgrade: Dictionary = get_upgrade(id)
	return int(upgrade.get("unlock_lifetime_mycelium", 0))


func is_unlocked(id: Variant) -> bool:
	var upgrade: Dictionary = get_upgrade(String(id)) if id is String else id as Dictionary
	if upgrade.is_empty():
		return false
	return highest_stage_reached >= int(upgrade.get("unlock_stage", 0)) \
		and lifetime_mycelium >= int(upgrade.get("unlock_lifetime_mycelium", 0))


func price(id: Variant) -> float:
	var upgrade: Dictionary = get_upgrade(String(id)) if id is String else id as Dictionary
	if upgrade.is_empty():
		return 0.0
	var upgrade_id: String = String(upgrade["id"])
	return ceilf(float(upgrade["cost"]) * pow(float(upgrade["growth"]), float(level(upgrade_id))))


func can_buy(id: String) -> bool:
	var upgrade: Dictionary = get_upgrade(id)
	if upgrade.is_empty() or not is_unlocked(upgrade) or is_capped(id):
		return false
	var cost: float = price(upgrade)
	return float(mycelium) >= cost if String(upgrade["currency"]) == "mycelium" else spores >= cost


func buy(id: String) -> bool:
	if not can_buy(id):
		return false
	var upgrade: Dictionary = get_upgrade(id)
	var cost: float = price(upgrade)
	if String(upgrade["currency"]) == "mycelium":
		mycelium -= int(cost)
	else:
		spores -= cost
	levels[id] = level(id) + 1
	purchased.emit(id, level(id))
	_try_clear_stage()
	return true


func bulk_limit() -> int:
	return maxi(int(rules.get("bulk_buy_max_levels", DEFAULT_BULK_LIMIT)), 0)


func level_price(id: String, at_level: int) -> float:
	var upgrade: Dictionary = get_upgrade(id)
	if upgrade.is_empty():
		return 0.0
	return ceilf(float(upgrade["cost"]) * pow(float(upgrade["growth"]), float(at_level)))


func remaining_levels(id: String) -> int:
	var maximum: int = effective_max_level(id)
	if maximum < 0:
		return bulk_limit()
	return mini(maxi(maximum - level(id), 0), bulk_limit())


func wallet_for(id: String) -> float:
	var upgrade: Dictionary = get_upgrade(id)
	if upgrade.is_empty():
		return 0.0
	return float(mycelium) if String(upgrade["currency"]) == "mycelium" else spores


func bulk_count(id: String, amount: int) -> int:
	var upgrade: Dictionary = get_upgrade(id)
	if upgrade.is_empty() or not is_unlocked(upgrade):
		return 0
	var remaining: int = remaining_levels(id)
	var wallet: float = wallet_for(id)
	if amount == BUY_MAX:
		var start: int = level(id)
		var total: float = 0.0
		var count: int = 0
		while count < remaining:
			var next_total: float = total + level_price(id, start + count)
			if next_total > wallet:
				break
			total = next_total
			count += 1
		return count
	if amount <= 0:
		return 0
	var fixed: int = mini(amount, remaining)
	if fixed <= 0 or bulk_price(id, fixed) > wallet:
		return 0
	return fixed


func bulk_target(id: String, amount: int) -> int:
	if amount == BUY_MAX:
		if remaining_levels(id) <= 0:
			return 0
		return maxi(1, bulk_count(id, BUY_MAX))
	if amount <= 0:
		return 0
	return mini(amount, remaining_levels(id))


func bulk_price(id: String, count: int) -> float:
	var start: int = level(id)
	var total: float = 0.0
	for offset: int in range(maxi(count, 0)):
		total += level_price(id, start + offset)
	return total


func buy_many(id: String, amount: int) -> int:
	var count: int = bulk_count(id, amount)
	if count <= 0:
		return 0
	var upgrade: Dictionary = get_upgrade(id)
	var cost: float = bulk_price(id, count)
	if String(upgrade["currency"]) == "mycelium":
		mycelium -= int(cost)
	else:
		spores -= cost
	levels[id] = level(id) + count
	purchased.emit(id, level(id))
	_try_clear_stage()
	return count


func spores_per_sec() -> float:
	return _stat("spores_per_sec", float(rules["base_spores_per_sec"])) * Garden.get_spores_mult() * specimen_spores_mult()


func grow_power() -> float:
	return _stat("grow_power", float(rules["base_grow_power"])) * Garden.get_grow_mult()


func max_stage() -> int:
	return int(rules["count"])


func stage_hp(n: int) -> float:
	return float(rules["hp_base"]) * pow(float(rules["hp_growth"]), float(n - 1))


func stage_reward(n: int) -> float:
	return float(rules["reward_base"]) * pow(float(rules["reward_growth"]), float(n - 1))


func next_stage_requirement() -> float:
	if stage >= max_stage():
		return 0.0
	return stage_hp(stage + 1) / float(rules["clear_seconds"])


func stage_progress() -> float:
	if stage >= max_stage():
		return 1.0
	return clampf(grow_power() / next_stage_requirement(), 0.0, 1.0)


func next_landmark_text() -> String:
	var candidates: Array[Dictionary] = []
	for upgrade_value: Variant in get_upgrades_for_system("jar"):
		var upgrade: Dictionary = upgrade_value as Dictionary
		var upgrade_stage: int = int(upgrade["unlock_stage"])
		if upgrade_stage > highest_stage_reached:
			candidates.append({"stage": upgrade_stage, "name": String(upgrade["name"])})
	var garden_stage: int = int(rules["garden_unlock_stage"])
	if not garden_unlocked:
		candidates.append({"stage": garden_stage, "name": "Dew Garden"})
	var prestige_stage: int = int(rules["prestige_unlock_stage"])
	if not regrow_unlocked:
		candidates.append({"stage": prestige_stage, "name": "Regrow"})
	for specimen_value: Variant in get_specimens():
		var specimen: Dictionary = specimen_value as Dictionary
		if not is_specimen_collected(String(specimen["id"])):
			candidates.append({"stage": int(specimen["unlock_stage"]), "name": "Pressed Specimen"})
	if candidates.is_empty():
		return "All landmarks reached"
	candidates.sort_custom(func(a: Dictionary, b: Dictionary) -> bool: return int(a["stage"]) < int(b["stage"]))
	var next_landmark: Dictionary = candidates[0]
	return "Stage %d: %s" % [int(next_landmark["stage"]), String(next_landmark["name"])]


func can_prestige() -> bool:
	return regrow_unlocked and prestige_gain() >= 1


func prestige_gain() -> int:
	var divisor: float = float(rules["prestige_divisor"])
	var exponent: float = float(rules["prestige_exponent"])
	return int(floor(pow(run_spores / divisor, exponent)))


func spores_for_mycelium(target_gain: int) -> float:
	if target_gain <= 0:
		return 0.0
	var exponent: float = float(rules["prestige_exponent"])
	return float(rules["prestige_divisor"]) * pow(float(target_gain), 1.0 / exponent)


func spores_to_next_mycelium() -> float:
	return maxf(spores_for_mycelium(prestige_gain() + 1) - run_spores, 0.0)


func prestige() -> bool:
	if not can_prestige():
		return false
	var gain: int = prestige_gain()
	var old_lifetime: int = lifetime_mycelium
	mycelium += gain
	lifetime_mycelium += gain
	for upgrade_value: Variant in upgrades:
		var upgrade: Dictionary = upgrade_value as Dictionary
		var threshold: int = int(upgrade.get("unlock_lifetime_mycelium", 0))
		if threshold > old_lifetime and threshold <= lifetime_mycelium:
			var milestone: Variant = upgrade.get("unlock_milestone")
			if milestone != null:
				network_tier_unlocked.emit(String(milestone))
	spores = 0.0
	run_spores = 0.0
	stage = 0
	run_elapsed_seconds = 0.0
	for upgrade_value: Variant in upgrades:
		var upgrade: Dictionary = upgrade_value as Dictionary
		if String(upgrade["system"]) == "jar":
			levels[String(upgrade["id"])] = 0
	_try_clear_stage()
	prestiged.emit(gain)
	return true


func get_specimens() -> Array:
	return rules.get("specimens", []) as Array


func is_specimen_collected(specimen_id: String) -> bool:
	return specimens_collected.has(specimen_id)


func specimen_count() -> int:
	return specimens_collected.size()


func specimen_trellis_cap_bonus() -> int:
	var bonus: int = 0
	for specimen_value: Variant in get_specimens():
		var specimen: Dictionary = specimen_value as Dictionary
		if not is_specimen_collected(String(specimen["id"])):
			continue
		for effect_value: Variant in specimen["effects"] as Array:
			var effect: Dictionary = effect_value as Dictionary
			if String(effect["target"]) == "trellis" and String(effect["kind"]) == "cap":
				bonus += int(effect["value"])
	return bonus


func specimen_spores_mult() -> float:
	var multiplier: float = 1.0
	for specimen_value: Variant in get_specimens():
		var specimen: Dictionary = specimen_value as Dictionary
		if not is_specimen_collected(String(specimen["id"])):
			continue
		for effect_value: Variant in specimen["effects"] as Array:
			var effect: Dictionary = effect_value as Dictionary
			if String(effect["target"]) == "spores_per_sec" and String(effect["kind"]) == "mult":
				multiplier *= float(effect["value"])
	return multiplier


func _stat(stat: String, base: float) -> float:
	var flat: float = base
	var bonus: float = 0.0
	var multiplier: float = 1.0
	for upgrade_value: Variant in upgrades:
		var upgrade: Dictionary = upgrade_value as Dictionary
		var effect: Dictionary = upgrade["effect"] as Dictionary
		if String(effect["stat"]) != stat:
			continue
		var upgrade_level: int = level(String(upgrade["id"]))
		flat += float(effect.get("add", 0.0)) * float(upgrade_level)
		bonus += float(effect.get("bonus_pct", 0.0)) * float(upgrade_level)
		if effect.has("mult"):
			multiplier *= pow(float(effect["mult"]), float(upgrade_level))
	return flat * (1.0 + bonus) * multiplier
