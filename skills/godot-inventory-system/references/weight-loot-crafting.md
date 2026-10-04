Adds weight limits, seeded weighted loot tables, atomic crafting, world pickups and per-instance item state to the slot inventory. Read it when items have weight, drop from tables, combine into other items, or need durability.

> ← Back to [SKILL.md](../SKILL.md)

# Weight, Loot, Crafting and Item Instances

These recipes build on `Inventory`, `InventorySlot` and `ItemData` from [core-classes.md](core-classes.md). The code here uses small stand-in classes so each block is complete. Rename them to your project's classes.

## Weight limits

Check capacity **before** the add, then add only what fits. A post-check that rolls back is harder to get right and emits signals for a change that did not happen.

```gdscript
# weighted_bag.gd
class_name WeightedBag
extends RefCounted

signal changed

var max_weight: float
var _counts: Dictionary[StringName, int] = {}
var _unit_weight: Dictionary[StringName, float] = {}


func _init(limit: float) -> void:
    max_weight = limit


func total_weight() -> float:
    var total := 0.0
    for id in _counts:
        total += _unit_weight[id] * _counts[id]
    return total


## Adds as many as the weight limit allows. Returns the leftover count.
func add(id: StringName, unit_weight: float, quantity: int) -> int:
    var fits := quantity
    if unit_weight > 0.0:
        var room := max_weight - total_weight()
        fits = clampi(floori(room / unit_weight), 0, quantity)
    if fits > 0:
        _unit_weight[id] = unit_weight
        _counts[id] = _counts.get(id, 0) + fits
        changed.emit()
    return quantity - fits
```

- Keep quantities `int`. Keep weights `float`, and compare with a small margin if a designer will hit the limit exactly (`room + 0.0001`).
- **Encumbrance instead of a hard cap** is a design choice: allow the add, and slow the player above the limit. Say which one the game uses before you build it.
- Combine with slots: run the weight check first, then the two-pass slot add from SKILL.md with the reduced quantity.

## Loot tables

A loot table is a `Resource`, so a designer assigns `boss_table.tres` or `trash_table.tres` in the Inspector. Roll it with a `RandomNumberGenerator` that the caller owns, so a seeded run gives the same drops.

```gdscript
# loot_entry.gd
class_name LootEntry
extends Resource

@export var item_id: StringName = &""
@export_range(0.0, 1000.0) var weight: float = 1.0
@export var min_count: int = 1
@export var max_count: int = 1
```

```gdscript
# loot_table.gd
class_name LootTable
extends Resource

@export var entries: Array[LootEntry] = []
## How many independent rolls one drop makes.
@export var rolls: int = 1
## Chance that one roll gives nothing at all.
@export_range(0.0, 1.0) var nothing_chance: float = 0.0


## Returns item id -> count. The same rng seed always gives the same result.
func roll(rng: RandomNumberGenerator) -> Dictionary[StringName, int]:
    var result: Dictionary[StringName, int] = {}
    if entries.is_empty():
        return result
    var weights := PackedFloat32Array()
    for entry in entries:
        weights.append(entry.weight)
    for i in rolls:
        if rng.randf() < nothing_chance:
            continue
        var index := rng.rand_weighted(weights)
        if index < 0:
            continue
        var entry := entries[index]
        var count := rng.randi_range(entry.min_count, entry.max_count)
        result[entry.item_id] = result.get(entry.item_id, 0) + count
    return result
```

- `RandomNumberGenerator.rand_weighted()` (4.3+) returns the index of one weight, or `-1` when all weights are zero. Do not hand-roll a cumulative sum.
- Weights are relative. `[70, 25, 5]` and `[0.7, 0.25, 0.05]` give the same odds, so designers can use whole numbers.
- Never call the global `randf()` in loot code. Pass the run's or the chest's `RandomNumberGenerator` in. A chest that must give the same loot after a reload stores its seed in the save file.
- A nested table ("roll the gem table") is an entry whose id names another table. Resolve it in the caller, and cap the depth to stop a table that contains itself.

## Crafting

Crafting must be **atomic**: either the ingredients go and the result arrives, or nothing changes. The common bug removes the ingredients, then finds no room for the result, and the player loses both.

```gdscript
# crafting_recipe.gd
class_name CraftingRecipe
extends Resource

## Ingredient id -> count needed.
@export var ingredients: Dictionary[StringName, int] = {}
@export var result_id: StringName = &""
@export var result_count: int = 1
```

```gdscript
# crafter.gd
class_name Crafter
extends RefCounted

## The bag interface the crafter needs. Your Inventory provides these three.
var count_of: Callable      # func(id: StringName) -> int
var room_for: Callable      # func(id: StringName, count: int) -> bool
var apply_delta: Callable   # func(removed: Dictionary, added_id: StringName, added_count: int) -> void


func can_craft(recipe: CraftingRecipe) -> bool:
    for id in recipe.ingredients:
        if int(count_of.call(id)) < recipe.ingredients[id]:
            return false
    return bool(room_for.call(recipe.result_id, recipe.result_count))


func craft(recipe: CraftingRecipe) -> bool:
    if not can_craft(recipe):
        return false
    apply_delta.call(recipe.ingredients, recipe.result_id, recipe.result_count)
    return true
```

- `room_for` must count the space the ingredients free. A recipe that turns 10 ore into 1 bar in a full bag should succeed. Simulate the removal on a copy of the slot counts, then test the add.
- `apply_delta` does the removal and the add in one inventory method and emits `inventory_changed` once.
- Recipe discovery (which recipes the player knows) is game state, not recipe data. Keep a `Array[StringName]` of known recipe ids in the save file.

## World pickups

A pickup removes itself only when the whole stack fits. When part fits, it keeps the rest and stays in the world.

```gdscript
# item_pickup.gd
class_name ItemPickup
extends Area2D

@export var item_id: StringName = &""
@export var count: int = 1


func _ready() -> void:
    body_entered.connect(_on_body_entered)


func _on_body_entered(body: Node2D) -> void:
    if not body.has_method("give_item"):
        return
    var leftover: int = body.call("give_item", item_id, count)
    if leftover <= 0:
        queue_free()
    else:
        count = leftover
```

- `give_item` returns the leftover, the same contract as `Inventory.add_item`. A `bool` return loses the partial case.
- Set the pickup's collision mask to the player layer only, or every enemy that walks over it triggers the check.
- Use `Area3D` and `body_entered(body: Node3D)` for 3D. The logic is the same.

## Per-instance item state (durability, rolled stats)

SKILL.md compares items by reference, which is right for plain stackable items. An item with its own state (a sword at 40% durability, a ring with rolled stats) needs an **instance** that points at the shared definition. Never write that state into the shared `.tres`: every copy of the item would change.

```gdscript
# item_instance.gd
class_name ItemInstance
extends RefCounted

var definition_id: StringName
var durability: float = 1.0
var rolled_stats: Dictionary[StringName, float] = {}


func _init(id: StringName) -> void:
    definition_id = id


func to_dict() -> Dictionary:
    return {
        "id": String(definition_id),
        "durability": durability,
        "stats": rolled_stats.duplicate(),
    }


static func from_dict(data: Dictionary) -> ItemInstance:
    var inst := ItemInstance.new(StringName(str(data.get("id", ""))))
    inst.durability = float(data.get("durability", 1.0))
    var stats: Dictionary = data.get("stats", {})
    for key in stats:
        inst.rolled_stats[StringName(str(key))] = float(stats[key])
    return inst
```

- An item with instance state has `max_stack_size = 1`. Two swords with different durability cannot share a stack.
- A duplicated definition also works, but it carries every export into each save. Since 4.5, `duplicate(true)` copies only internal sub-resources; `duplicate_deep(Resource.DEEP_DUPLICATE_ALL)` gives the old full copy (upgrading_to_godot_4.5, verified by unit GW10). A small instance object keeps saves to the fields that change.
- Save instances with `to_dict()`, never the Resource. The save stays valid when the definition changes.

## Consumables

Put the effect on the item definition as a virtual method, so the inventory does not grow a `match` on item type.

```gdscript
# usable_item.gd
class_name UsableItem
extends Resource

@export var id: StringName = &""
@export var consumed_on_use: bool = true


## Override in subclasses. Return true when the use succeeded.
func use(_user: Node) -> bool:
    return false
```

```gdscript
# heal_item.gd
class_name HealItem
extends UsableItem

@export var amount: int = 25


func use(user: Node) -> bool:
    if not user.has_method("heal"):
        return false
    user.call("heal", amount)
    return true
```

The inventory calls `use()` and removes one only when it returns `true` and `consumed_on_use` is set. A potion used at full health can then return `false` and stay in the bag.
