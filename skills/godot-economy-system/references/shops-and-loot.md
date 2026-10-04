# Shops and loot

Both read numbers from resources and change balances only through the
`Wallet` in `SKILL.md`. Items themselves (stacks, slots) are in
`godot-inventory-system`; here an item is an id.

## Shop data

```gdscript
class_name ShopEntry
extends Resource

@export var item_id: StringName
@export var currency: StringName = &"gold"
@export var base_price: int = 100
## The shop buys back at this share of the price it sells at.
@export_range(0.0, 0.95) var sell_back_ratio: float = 0.5
## -1 means unlimited stock.
@export var max_stock: int = -1
@export var restock_seconds: int = 0
```

## The shop

```gdscript
class_name Shop
extends Node

signal stock_changed(item_id: StringName, stock: int)

@export var entries: Array[ShopEntry] = []
## Multiplies every price the shop sells at: 0.9 is a 10% discount.
@export var price_multiplier: float = 1.0

var _stock: Dictionary[StringName, int] = {}
var _last_restock: Dictionary[StringName, int] = {}


func _ready() -> void:
	for entry: ShopEntry in entries:
		_stock[entry.item_id] = entry.max_stock


func buy_price(entry: ShopEntry) -> int:
	return maxi(roundi(entry.base_price * price_multiplier), 1)


## The price the shop pays the player. Always below buy_price.
func sell_price(entry: ShopEntry) -> int:
	var offer: int = floori(entry.base_price * entry.sell_back_ratio)
	return clampi(offer, 0, buy_price(entry) - 1)


func stock(item_id: StringName) -> int:
	return _stock.get(item_id, 0)


func try_buy(wallet: Wallet, item_id: StringName) -> bool:
	var entry: ShopEntry = _find(item_id)
	if entry == null or stock(item_id) == 0:
		return false
	var cost: Dictionary[StringName, int] = {entry.currency: buy_price(entry)}
	if not wallet.spend(cost, &"shop_buy"):
		return false
	if entry.max_stock >= 0:
		_stock[item_id] -= 1
		stock_changed.emit(item_id, _stock[item_id])
	return true


func sell_to_shop(wallet: Wallet, item_id: StringName) -> int:
	var entry: ShopEntry = _find(item_id)
	if entry == null:
		return 0
	return wallet.grant(entry.currency, sell_price(entry), &"shop_sell")


func restock(now_unix: int) -> void:
	for entry: ShopEntry in entries:
		if entry.max_stock < 0 or entry.restock_seconds <= 0:
			continue
		var last: int = _last_restock.get(entry.item_id, now_unix)
		if now_unix - last >= entry.restock_seconds:
			_stock[entry.item_id] = entry.max_stock
			_last_restock[entry.item_id] = now_unix
			stock_changed.emit(entry.item_id, entry.max_stock)
		elif not _last_restock.has(entry.item_id):
			_last_restock[entry.item_id] = now_unix


func _find(item_id: StringName) -> ShopEntry:
	for entry: ShopEntry in entries:
		if entry.item_id == item_id:
			return entry
	return null
```

The order in `try_buy` matters: the spend runs first and the stock changes
only after it succeeds. The inventory grant goes after `try_buy` returns
`true`. If the inventory is full, check that before the spend, or refund
with `wallet.grant(..., &"refund")`.

Notes:

- `sell_price` clamps to one below `buy_price`. A discount (a low
  `price_multiplier`) can never make "buy, then sell back" pay.
- Use the restock time in Unix seconds from `Time.get_unix_time_from_system()`
  if restock must pass while the game is closed. Store `_stock` and
  `_last_restock` in the save data.
- Dynamic prices (reputation, demand, a sale event) are more multipliers on
  `buy_price`. Keep every multiplier in one function, so the clamp in
  `sell_price` sees the final price.

## Barter

A trade is "these items for those items". Check every required item before
you remove any, then remove and add as one step. The inventory owns the item
checks (`godot-inventory-system`); the trade is data: two
`Dictionary[StringName, int]` (item id to count), one for each side.

## Loot tables

```gdscript
class_name LootTable
extends Resource

@export var item_ids: Array[StringName] = []
@export var weights := PackedFloat32Array()
## Weight of "no drop". 0 means a roll always drops something.
@export var nothing_weight: float = 0.0


## Returns an item id, or &"" for no drop.
func roll(rng: RandomNumberGenerator) -> StringName:
	var all := PackedFloat32Array(weights)
	all.append(nothing_weight)
	var index: int = rng.rand_weighted(all)
	if index < 0 or index >= item_ids.size():
		return &""
	return item_ids[index]
```

- `RandomNumberGenerator.rand_weighted` (4.3 and later) returns an index
  picked by weight, or -1 when every weight is 0.
- Keep `item_ids` and `weights` the same length; check it in a test, or in
  `_validate_property` for an editor warning.
- Seed the generator (`rng.seed = run_seed`) for daily runs, replays and
  tests. Use one generator per purpose (loot, map, combat), so a new roll in
  one place does not change the others.

### Pity counters

A rare drop with a 1% chance can miss for hundreds of rolls. A pity counter
counts misses and raises the weight, or forces the drop, at a limit. Store
the counter per player and per table in the save data. Tell players about
it if the drop costs real money; some regions require published odds.

### Currency drops

A drop that grants currency calls `wallet.grant(currency, amount, &"loot")`
when the player picks it up, not when the enemy dies; an unpicked coin
is not income. Pickups follow the "commit, then show, then free" rule in
`godot-gameplay-loops`.

## Checks

- `try_buy` with too little money: balance and stock unchanged.
- Stock 1: the second `try_buy` returns `false`.
- For every entry and `price_multiplier` in 0.1 to 2.0: `sell_price < buy_price`.
- `LootTable.roll` 10,000 times with weights 1, 3 and `nothing_weight` 0:
  about 25% and 75%.
