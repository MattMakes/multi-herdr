extends RefCounted


static func choose(acts: Array, player: Dictionary, since_purchase_s: float, refused: Dictionary) -> Dictionary:
	var need: float = float(player.get("reset_gain", 1.0))
	var stuck: bool = since_purchase_s >= float(player.get("stuck_s", 600.0))
	var best: Dictionary = {}
	var best_cost: float = 0.0
	var best_currency: String = ""
	var reset: Dictionary = {}
	for entry: Variant in acts:
		if not entry is Dictionary:
			continue
		var act: Dictionary = entry
		if not act.get("affordable", false) or refused.has(act.get("id", "")):
			continue
		if act.get("bot", true) is bool and not act.get("bot", true):
			continue
		if str(act.get("kind", "")) == "reset":
			if reset.is_empty() and (stuck or float(act.get("gain", 0.0)) >= need):
				reset = act
			continue
		var cost: float = float(act.get("cost", 0.0))
		var currency: String = str(act.get("currency", ""))
		if best.is_empty():
			best = act
			best_cost = cost
			best_currency = currency
		elif currency == best_currency and cost < best_cost:
			best = act
			best_cost = cost
	if not reset.is_empty():
		return reset
	return best
