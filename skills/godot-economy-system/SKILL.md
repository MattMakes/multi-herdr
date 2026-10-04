---
name: godot-economy-system
description: "Use when building a game economy in Godot 4.7 with GDScript: currencies held as integers, a wallet autoload with caps and change signals, all-or-nothing transactions over several currencies with a ledger, shops with buy and sell spread, stock, restock and price modifiers, weighted loot tables with seeded rolls, currency sinks and faucets, gold-per-minute telemetry, very large idle-game numbers, wallet save data, and server-side spends online. Items and stacks are in godot-inventory-system; save files are in godot-save-load."
---

# Godot economy system

Target engine: **Godot 4.7**. Every code block is typed GDScript that parses
on 4.7.2. Every engine API it names is in the 4.7.2 `--doctool` dump.

## Rules

- **Money is an `int` in the smallest unit.** Store cents, not dollars:
  `1250` shows as `12.50`. A float drifts (`0.1 + 0.2 != 0.3`) and a save
  then holds a coin count no one can explain.
- **GDScript `int` is 64-bit.** It holds up to about 9.2 × 10^18. Plain games
  never need more. Idle games that pass that need a big-number type
  ([references/balancing.md](references/balancing.md)).
- **One wallet owns the balances.** An autoload holds every balance. UI,
  shops and pickups ask it; they never write a balance.
- **Check, then change.** A spend tests `balance >= cost` first. A balance
  never goes below 0.
- **All or nothing.** A price in two currencies (100 gold and 2 gems) takes
  both or neither.
- **Clamp at the cap.** A grant that passes the cap clamps, and reports how
  much it really added.
- **Every change has a reason.** The wallet logs `(currency, delta, reason)`.
  The log finds lost coins and feeds balancing.
- **Sell for less than you buy.** Equal buy and sell prices, or a price
  modifier that can flip them, make money from nothing.
- **Online: the server decides.** A client asks to buy; the server checks
  and applies the spend, then sends the new balance.

## The wallet

```gdscript
class_name Wallet
extends Node

signal balance_changed(currency: StringName, balance: int, delta: int)
signal transaction_refused(reason: StringName)

## currency id -> the most a player can hold. A missing id has no cap.
@export var caps: Dictionary[StringName, int] = {}
@export var ledger_size: int = 200

var _balances: Dictionary[StringName, int] = {}
var _ledger: Array[Dictionary] = []


func balance(currency: StringName) -> int:
	return _balances.get(currency, 0)


func can_afford(cost: Dictionary[StringName, int]) -> bool:
	for currency: StringName in cost:
		if cost[currency] < 0 or balance(currency) < cost[currency]:
			return false
	return true


## Takes the whole cost or nothing. Returns false when it takes nothing.
func spend(cost: Dictionary[StringName, int], reason: StringName) -> bool:
	if not can_afford(cost):
		transaction_refused.emit(&"insufficient_funds")
		return false
	for currency: StringName in cost:
		_apply(currency, -cost[currency], reason)
	return true


## Adds an amount up to the cap. Returns the amount really added.
func grant(currency: StringName, amount: int, reason: StringName) -> int:
	if amount <= 0:
		return 0
	var room: int = caps.get(currency, 9223372036854775807) - balance(currency)
	var added: int = mini(amount, maxi(room, 0))
	if added > 0:
		_apply(currency, added, reason)
	return added


func recent_ledger() -> Array[Dictionary]:
	return _ledger.duplicate()


func to_save() -> Dictionary:
	var out: Dictionary = {}
	for currency: StringName in _balances:
		out[String(currency)] = _balances[currency]
	return out


func from_save(data: Dictionary) -> void:
	_balances.clear()
	for key: Variant in data:
		var currency := StringName(key)
		var value: int = maxi(int(data[key]), 0)
		_balances[currency] = mini(value, caps.get(currency, value))
		balance_changed.emit(currency, _balances[currency], 0)


func _apply(currency: StringName, delta: int, reason: StringName) -> void:
	_balances[currency] = balance(currency) + delta
	_ledger.append({"currency": currency, "delta": delta, "reason": reason,
		"msec": Time.get_ticks_msec()})
	if _ledger.size() > ledger_size:
		_ledger.pop_front()
	balance_changed.emit(currency, _balances[currency], delta)
```

Register it as an autoload (for example `Wallet`). Notes:

- `cost` is a typed dictionary: `{&"gold": 100, &"gems": 2}`. A negative cost
  is refused, so a bad data file cannot turn a spend into a grant.
- `from_save` converts the JSON floats back to `int`, drops negative values,
  and clamps to the caps. `godot-save-load` writes the file. Save currency
  ids as strings.
- The ledger is a ring of recent entries. For an audit trail across
  sessions, append entries to a file in batches, not one write per change.
- A balance label connects to `balance_changed` and formats the number. It
  never polls in `_process`.

## Currencies as data

Describe each currency in a resource, so the HUD and shops read its name,
icon and decimals from data (`godot-resource-pattern`):

```gdscript
class_name CurrencyDef
extends Resource

@export var id: StringName
@export var display_name: String
@export var icon: Texture2D
## 2 for a currency shown with cents. The wallet stores the smallest unit.
@export_range(0, 4) var decimals: int = 0
@export var cap: int = 0  # 0 means no cap


func format(amount: int) -> String:
	if decimals == 0:
		return str(amount)
	var unit: int = 10 ** decimals
	var sign_text: String = "-" if amount < 0 else ""
	var whole: int = absi(amount) / unit
	var part: int = absi(amount) % unit
	return "%s%d.%0*d" % [sign_text, whole, decimals, part]
```

`format(1250)` with 2 decimals gives `12.50`. For thousands separators and
locales, see `godot-localization`.

## Shops, loot, balance

| Task | Read |
| --- | --- |
| Shop stock, buy and sell prices, restock, discounts, barter | [references/shops-and-loot.md](references/shops-and-loot.md) |
| Loot tables, drop rolls, pity counters, currency pickups | [references/shops-and-loot.md](references/shops-and-loot.md) |
| Sinks and faucets, gold-per-minute, inflation checks, big numbers | [references/balancing.md](references/balancing.md) |

## Online

- The server runs the wallet; each client has a read-only copy.
- A client calls an RPC "buy item X from shop Y". The server checks stock,
  price and balance, runs `spend`, changes the inventory, and sends both
  results. The client shows a spinner until then, not a guessed balance.
- Never accept a price or an amount from a client. The client sends ids; the
  server looks up the numbers.
- `godot-multiplayer-sync` covers RPC and authority.

## Other skills own these parts

| Need | Skill |
| --- | --- |
| Items, stacks, slots and item data | `godot-inventory-system` |
| Writing the wallet's save data to disk | `godot-save-load` |
| Currency and price data in `.tres` files | `godot-resource-pattern` |
| Wallet change events across the game | `godot-event-bus` |
| Balance labels and shop screens | `godot-hud-system`, `godot-ui` |
| Rewards for quests | `godot-quest-system` |
| Harvest and idle income loops | `godot-gameplay-loops` |
| Number formats per language | `godot-localization` |

## Prove it

Test the wallet headless in a `SceneTree` script: a spend over two
currencies with one short leaves both balances unchanged; a grant past the
cap returns the clamped amount; a sell price is below the buy price for every
item at every modifier; a save and load through `JSON` keeps the balances as
`int`. `godot-build-verify` runs the script.
