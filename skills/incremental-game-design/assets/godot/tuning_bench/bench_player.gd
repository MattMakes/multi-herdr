extends RefCounted
## The bot of the tuning bench. It decides what a player does at 1 decision.
##
## The rule is simple and the same for each game. The bot does the reset when a reset is affordable and
## its gain is large enough, or when no purchase came for a long time. If not, it buys the affordable action
## with the lowest cost.
##
## A cost is a plain number. An action can name its currency with the optional key "currency". The bot compares
## 2 costs only inside 1 currency: it takes the currency of the first affordable action of the list and buys the
## lowest cost of that currency. An action with no key is in the main currency ("").
##
## An action with `"bot": false` is a free choice of the player (a selector, a switch that goes on and off).
## The bot never does it: a bot would repeat a free action with no end. The view shows it.


## Return the action that the player does now, or an empty dictionary when the player does nothing.
## `refused` has the ids that the game refused at this decision.
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
