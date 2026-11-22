extends Node
## Persistent Dew Garden simulation and balance queries.

signal dew_collected(amount: int)
signal bed_purchased(beds: int)
signal tonic_unlocked(id: String)
signal tonic_purchased(id: String, level: int)

var dew: int = 0
var lifetime_dew: int = 0
var beds: int = 1
var drip_timer: float = 0.0
var tonic_levels: Dictionary = {}
var unlocked_tonics: Dictionary = {}
var config: Dictionary = {}


func _ready() -> void:
	reset_all()


func reset_all() -> void:
	var parsed: Variant = JSON.parse_string(FileAccess.get_file_as_string("res://data/garden.json"))
	config = parsed as Dictionary
	dew = 0
	lifetime_dew = 0
	beds = int(config.get("initial_beds", 0))
	drip_timer = 0.0
	tonic_levels.clear()
	unlocked_tonics.clear()
	for tonic_value: Variant in get_tonics():
		var tonic: Dictionary = tonic_value as Dictionary
		tonic_levels[String(tonic["id"])] = 0


func is_open() -> bool:
	return Economy.garden_unlocked


func step(delta: float) -> void:
	if not is_open() or delta <= 0.0:
		return
	drip_timer += delta
	var interval: float = drip_seconds()
	while drip_timer >= interval:
		drip_timer -= interval
		var gain: int = dew_per_drip()
		dew += gain
		lifetime_dew += gain
		sync_tonic_milestones()
		dew_collected.emit(gain)


func sync_tonic_milestones() -> void:
	var grow_changed: bool = false
	for tonic_value: Variant in get_tonics():
		var tonic: Dictionary = tonic_value as Dictionary
		var id: String = String(tonic["id"])
		if lifetime_dew < int(tonic["unlock_lifetime_dew"]) or is_tonic_unlocked(id):
			continue
		unlocked_tonics[id] = true
		if not bool(tonic["purchasable"]):
			tonic_levels[id] = 1
			var effect: Dictionary = tonic["effect"] as Dictionary
			grow_changed = grow_changed or String(effect["stat"]) == "grow_power"
		tonic_unlocked.emit(id)
	if grow_changed:
		Economy._try_clear_stage()


func drip_seconds() -> float:
	return float(config["drip_seconds"])


func dew_per_bed() -> int:
	return int(config["dew_per_bed"])


func dew_per_drip() -> int:
	return beds * dew_per_bed()


func dew_per_minute() -> float:
	return float(dew_per_drip()) * 60.0 / drip_seconds()


func drip_progress() -> float:
	return clampf(drip_timer / drip_seconds(), 0.0, 1.0)


func seconds_to_next_drip() -> float:
	return maxf(drip_seconds() - drip_timer, 0.0)


func bed_price() -> int:
	var bed: Dictionary = config["bed"] as Dictionary
	var initial_beds: int = int(config["initial_beds"])
	var bought_levels: int = beds - initial_beds
	return int(ceilf(float(bed["cost"]) * pow(float(bed["growth"]), float(bought_levels))))


func can_buy_bed() -> bool:
	return is_open() and dew >= bed_price()


func buy_bed() -> bool:
	if not can_buy_bed():
		return false
	dew -= bed_price()
	beds += 1
	bed_purchased.emit(beds)
	return true


func get_tonics() -> Array:
	return config.get("tonics", []) as Array


func get_tonic(id: String) -> Dictionary:
	for tonic_value: Variant in get_tonics():
		var tonic: Dictionary = tonic_value as Dictionary
		if String(tonic["id"]) == id:
			return tonic
	return {}


func tonic_level(id: String) -> int:
	return int(tonic_levels.get(id, 0))


func is_tonic_unlocked(id: String) -> bool:
	return bool(unlocked_tonics.get(id, false))


func tonic_price(id: String) -> int:
	var tonic: Dictionary = get_tonic(id)
	if tonic.is_empty() or not bool(tonic.get("purchasable", false)):
		return 0
	return int(ceilf(float(tonic["cost"]) * pow(float(tonic["growth"]), float(tonic_level(id)))))


func can_buy_tonic(id: String) -> bool:
	var tonic: Dictionary = get_tonic(id)
	if tonic.is_empty() or not is_open() or not is_tonic_unlocked(id):
		return false
	if not bool(tonic.get("purchasable", false)):
		return false
	var maximum: Variant = tonic.get("max_level")
	if maximum != null and tonic_level(id) >= int(maximum):
		return false
	return dew >= tonic_price(id)


func buy_tonic(id: String) -> bool:
	if not can_buy_tonic(id):
		return false
	dew -= tonic_price(id)
	tonic_levels[id] = tonic_level(id) + 1
	tonic_purchased.emit(id, tonic_level(id))
	Economy._try_clear_stage()
	return true


func bed_level_price(bought_levels: int) -> int:
	var bed: Dictionary = config["bed"] as Dictionary
	return int(ceilf(float(bed["cost"]) * pow(float(bed["growth"]), float(bought_levels))))


func bed_bulk_price(count: int) -> int:
	var start: int = beds - int(config["initial_beds"])
	var total: int = 0
	for offset: int in range(maxi(count, 0)):
		total += bed_level_price(start + offset)
	return total


func bed_bulk_count(amount: int) -> int:
	if not is_open():
		return 0
	var start: int = beds - int(config["initial_beds"])
	return _bulk_count(amount, Economy.bulk_limit(), func(offset: int) -> int: return bed_level_price(start + offset))


func bed_bulk_target(amount: int) -> int:
	return _bulk_target(amount, Economy.bulk_limit(), bed_bulk_count(Economy.BUY_MAX))


func buy_beds(amount: int) -> int:
	var count: int = bed_bulk_count(amount)
	if count <= 0:
		return 0
	dew -= bed_bulk_price(count)
	beds += count
	bed_purchased.emit(beds)
	return count


func tonic_level_price(id: String, at_level: int) -> int:
	var tonic: Dictionary = get_tonic(id)
	if tonic.is_empty() or not bool(tonic.get("purchasable", false)):
		return 0
	return int(ceilf(float(tonic["cost"]) * pow(float(tonic["growth"]), float(at_level))))


func tonic_remaining_levels(id: String) -> int:
	var tonic: Dictionary = get_tonic(id)
	if tonic.is_empty() or not bool(tonic.get("purchasable", false)):
		return 0
	var maximum: Variant = tonic.get("max_level")
	if maximum == null:
		return Economy.bulk_limit()
	return mini(maxi(int(maximum) - tonic_level(id), 0), Economy.bulk_limit())


func tonic_bulk_price(id: String, count: int) -> int:
	var start: int = tonic_level(id)
	var total: int = 0
	for offset: int in range(maxi(count, 0)):
		total += tonic_level_price(id, start + offset)
	return total


func tonic_bulk_count(id: String, amount: int) -> int:
	if not is_open() or not is_tonic_unlocked(id):
		return 0
	var start: int = tonic_level(id)
	return _bulk_count(amount, tonic_remaining_levels(id), func(offset: int) -> int: return tonic_level_price(id, start + offset))


func tonic_bulk_target(id: String, amount: int) -> int:
	return _bulk_target(amount, tonic_remaining_levels(id), tonic_bulk_count(id, Economy.BUY_MAX))


func buy_tonics(id: String, amount: int) -> int:
	var count: int = tonic_bulk_count(id, amount)
	if count <= 0:
		return 0
	dew -= tonic_bulk_price(id, count)
	tonic_levels[id] = tonic_level(id) + count
	tonic_purchased.emit(id, tonic_level(id))
	Economy._try_clear_stage()
	return count


## Returns the levels that 1 press buys. price_at gives the price of the level at an offset from the current level.
func _bulk_count(amount: int, remaining: int, price_at: Callable) -> int:
	var count: int = 0
	var total: int = 0
	if amount == Economy.BUY_MAX:
		while count < remaining:
			var next_total: int = total + int(price_at.call(count))
			if next_total > dew:
				break
			total = next_total
			count += 1
		return count
	if amount <= 0:
		return 0
	var fixed: int = mini(amount, remaining)
	for offset: int in range(fixed):
		total += int(price_at.call(offset))
	return fixed if total <= dew else 0


func _bulk_target(amount: int, remaining: int, max_count: int) -> int:
	if amount == Economy.BUY_MAX:
		return 0 if remaining <= 0 else maxi(1, max_count)
	if amount <= 0:
		return 0
	return mini(amount, remaining)


func get_grow_mult() -> float:
	return _effect_multiplier("grow_power")


func get_spores_mult() -> float:
	return _effect_multiplier("spores_per_sec")


func get_compost_cap_bonus() -> int:
	var bonus: int = 0
	for tonic_value: Variant in get_tonics():
		var tonic: Dictionary = tonic_value as Dictionary
		var effect: Dictionary = tonic["effect"] as Dictionary
		if String(effect["stat"]) == "compost" and effect.has("cap"):
			bonus += int(effect["cap"]) * tonic_level(String(tonic["id"]))
	return bonus


func _effect_multiplier(stat: String) -> float:
	var multiplier: float = 1.0
	for tonic_value: Variant in get_tonics():
		var tonic: Dictionary = tonic_value as Dictionary
		var effect: Dictionary = tonic["effect"] as Dictionary
		if String(effect["stat"]) == stat and effect.has("mult"):
			multiplier *= pow(float(effect["mult"]), float(tonic_level(String(tonic["id"]))))
	return multiplier
