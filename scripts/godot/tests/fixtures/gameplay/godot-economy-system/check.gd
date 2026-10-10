extends SceneTree
## godot-economy-system scenarios: "Prove it" in SKILL.md and the "Checks"
## lists of references/shops-and-loot.md and references/balancing.md, on the
## skill's own blocks in res://skill/.

const Expect = preload("res://expect.gd")

var e := Expect.new()


func _initialize() -> void:
	e.watch(self)
	# The root enters the tree after _initialize starts; _ready needs it.
	await process_frame
	_wallet()
	_save_round_trip()
	_currency_format()
	_shop()
	_loot()
	_report_and_curve()
	e.finish(self)


func _new_wallet() -> Wallet:
	var w := Wallet.new()
	w.caps = {&"gold": 1000} as Dictionary[StringName, int]
	root.add_child(w)
	return w


func _wallet() -> void:
	var w := _new_wallet()
	w.ledger_size = 2
	var refused: Array[StringName] = []
	w.transaction_refused.connect(func(reason: StringName) -> void: refused.append(reason))
	w.grant(&"gold", 500, &"loot")
	var cost: Dictionary[StringName, int] = {&"gold": 300, &"gems": 5}
	e.check(not w.spend(cost, &"shop_buy") and w.balance(&"gold") == 500 and w.balance(&"gems") == 0
		and refused == [&"insufficient_funds"],
		"a spend over two currencies with one short leaves both balances unchanged")
	var negative: Dictionary[StringName, int] = {&"gold": -50}
	e.check(not w.spend(negative, &"bad_data") and w.balance(&"gold") == 500, "a negative cost is refused")
	var added: int = w.grant(&"gold", 800, &"loot")
	e.check(added == 500 and w.balance(&"gold") == 1000, "a grant past the cap returns the clamped amount")
	e.check(w.grant(&"gold", 0, &"loot") == 0 and w.grant(&"gems", -3, &"loot") == 0, "a grant of 0 or less adds 0")
	e.check(w.grant(&"gems", 7, &"quest") == 7, "a currency with no cap takes the whole grant")
	w.grant(&"gems", 1, &"quest")
	w.grant(&"gems", 1, &"quest")
	var ledger: Array[Dictionary] = w.recent_ledger()
	e.check(ledger.size() == 2 and ledger[-1]["reason"] == &"quest", "the ledger keeps the last ledger_size entries")
	w.queue_free()


func _save_round_trip() -> void:
	var w := _new_wallet()
	w.grant(&"gold", 250, &"loot")
	w.grant(&"gems", 3, &"quest")
	var text: String = JSON.stringify(w.to_save())
	var loaded := _new_wallet()
	loaded.from_save(JSON.parse_string(text))
	e.check(loaded.balance(&"gold") == 250 and loaded.balance(&"gems") == 3
		and typeof(loaded.to_save()["gold"]) == TYPE_INT,
		"a save and load through JSON keeps the balances as int")
	loaded.from_save({"gold": 5000.0, "gems": -4.0})
	e.check(loaded.balance(&"gold") == 1000 and loaded.balance(&"gems") == 0,
		"from_save clamps to the cap and drops negative values")
	w.queue_free()
	loaded.queue_free()


func _currency_format() -> void:
	var cents := CurrencyDef.new()
	cents.decimals = 2
	var plain := CurrencyDef.new()
	e.check(cents.format(12345) == "123.45" and cents.format(-5) == "-0.05" and cents.format(7) == "0.07"
		and plain.format(42) == "42", "CurrencyDef.format places the decimals")


func _entry(id: StringName, price: int, ratio: float, max_stock: int, restock: int = 0) -> ShopEntry:
	var entry := ShopEntry.new()
	entry.item_id = id
	entry.base_price = price
	entry.sell_back_ratio = ratio
	entry.max_stock = max_stock
	entry.restock_seconds = restock
	return entry


func _shop() -> void:
	var shop := Shop.new()
	shop.entries = [
		_entry(&"sword", 100, 0.5, 1, 60),
		_entry(&"potion", 30, 0.95, -1),
		_entry(&"pebble", 1, 0.95, -1),
	] as Array[ShopEntry]
	root.add_child(shop)
	var w := _new_wallet()
	w.grant(&"gold", 50, &"loot")
	e.check(not shop.try_buy(w, &"sword") and w.balance(&"gold") == 50 and shop.stock(&"sword") == 1,
		"try_buy with too little money: balance and stock unchanged")
	w.grant(&"gold", 500, &"loot")
	var first: bool = shop.try_buy(w, &"sword")
	var second: bool = shop.try_buy(w, &"sword")
	e.check(first and not second and w.balance(&"gold") == 450 and shop.stock(&"sword") == 0,
		"stock 1: the second try_buy returns false")
	e.check(shop.try_buy(w, &"potion") and shop.try_buy(w, &"potion") and shop.stock(&"potion") == -1,
		"an unlimited entry keeps stock -1")
	var spread_ok: bool = true
	for step: int in range(1, 21):
		shop.price_multiplier = step / 10.0
		for entry: ShopEntry in shop.entries:
			if shop.sell_price(entry) >= shop.buy_price(entry):
				spread_ok = false
	e.check(spread_ok, "sell_price < buy_price for every entry at every multiplier 0.1 to 2.0")
	shop.price_multiplier = 1.0
	shop.restock(1000)
	var early: int = shop.stock(&"sword")
	shop.restock(1059)
	var still: int = shop.stock(&"sword")
	shop.restock(1060)
	e.check(early == 0 and still == 0 and shop.stock(&"sword") == 1, "restock refills after restock_seconds")
	shop.queue_free()
	w.queue_free()


func _loot() -> void:
	var table := LootTable.new()
	table.item_ids = [&"common", &"rare"] as Array[StringName]
	table.weights = PackedFloat32Array([3.0, 1.0])
	var rng := RandomNumberGenerator.new()
	rng.seed = 12345
	var counts: Dictionary = {}
	for i: int in 10000:
		var id: StringName = table.roll(rng)
		counts[id] = counts.get(id, 0) + 1
	e.check(absi(counts.get(&"rare", 0) - 2500) < 250 and absi(counts.get(&"common", 0) - 7500) < 250
		and not counts.has(&""), "10,000 rolls with weights 3, 1 and nothing_weight 0: about 75% and 25%")
	table.nothing_weight = 4.0
	var nothing: int = 0
	for i: int in 1000:
		if table.roll(rng) == &"":
			nothing += 1
	e.check(nothing > 400 and nothing < 600, "nothing_weight 4 of 8: about half the rolls drop nothing")


func _report_and_curve() -> void:
	var w := _new_wallet()
	w.grant(&"gold", 600, &"loot")
	w.grant(&"gold", 200, &"quest")
	w.spend({&"gold": 100} as Dictionary[StringName, int], &"repair")
	var r: Dictionary = EconomyReport.per_minute(w.recent_ledger(), &"gold", 120000)
	e.check(is_equal_approx(r["income_per_min"], 400.0) and is_equal_approx(r["spent_per_min"], 50.0)
		and is_equal_approx(r["by_reason"][&"repair"], -50.0), "EconomyReport.per_minute over 2 minutes")
	w.queue_free()
	var rising: bool = true
	var last: int = 0
	for level: int in 101:
		var cost: int = roundi(100 * pow(1.15, level))
		if cost <= last:
			rising = false
		last = cost
	e.check(rising, "the exponential cost formula for levels 0 to 100 is positive and increasing")
