# Levels, skill trees, derived stats and damage formulas

Adds an experience curve with a cap, level-up rewards, a skill tree with prerequisites, ranks and point refunds, derived stats that follow their base stats, and one place for the damage formula. Read it when abilities and stats must grow over a playthrough, or when damage math is spread across character scripts.

All code targets Godot 4.7. The examples work with the `StatSet` from `references/stat-modifiers.md` (`get_value()`, `set_base()`, `add_modifier()`), but they reach it through duck typing so they also stand alone.

## Experience curve

A level curve gives the experience needed for each level. A pure exponential curve (`base * growth ^ level`) grows without bound: by level 60 at growth 1.2 it needs over 4 million points, and a large exponent overflows an `int`. Cap the growth or the level.

```gdscript
class_name LevelCurve
extends Resource

@export var base_xp: int = 100
@export var growth: float = 1.15
@export var max_level: int = 50
## Above this level, each level costs the same as this one.
@export var flat_after: int = 30


## Experience needed to go from `level` to `level + 1`. 0 at max level.
func xp_to_next(level: int) -> int:
    if level >= max_level:
        return 0
    var effective: int = mini(level, flat_after)
    return int(round(base_xp * pow(growth, effective - 1)))
```

```gdscript
class_name Progression
extends Node

signal xp_changed(current: int, needed: int)
signal leveled_up(new_level: int)

@export var curve: LevelCurve
@export var points_per_level: int = 1

var level: int = 1
var xp: int = 0
var skill_points: int = 0


func add_xp(amount: int) -> void:
    if amount <= 0:
        return
    xp += amount
    # One large reward can cross several levels.
    while curve.xp_to_next(level) > 0 and xp >= curve.xp_to_next(level):
        xp -= curve.xp_to_next(level)
        level += 1
        skill_points += points_per_level
        leveled_up.emit(level)
    if curve.xp_to_next(level) == 0:
        xp = 0  # max level: do not bank experience
    xp_changed.emit(xp, curve.xp_to_next(level))
```

Apply level rewards as changes to base values (`set_base()`), not as modifiers. A modifier with a source can be removed by a cleanse or an unequip; a level-up reward must not be.

## Skill tree

A skill tree is progression data: which skills are unlocked and at what rank. It does not cast anything. When a skill unlocks, the tree tells the caster's `AbilityComponent` to `grant()` the ability, or adds a permanent stat change. Keep the tree per character (or per save file), not in a global singleton that combat code reads.

```gdscript
class_name SkillDef
extends Resource

@export var id: StringName
@export var max_rank: int = 1
@export var cost_per_rank: int = 1
@export var required_level: int = 1
## Skill ids that must have at least rank 1 first.
@export var requires: Array[StringName] = []
```

```gdscript
class_name SkillTree
extends Node

signal rank_changed(skill_id: StringName, rank: int)

@export var defs: Array[SkillDef] = []
@export var progression: Progression

var _by_id: Dictionary[StringName, SkillDef] = {}
var _rank: Dictionary[StringName, int] = {}


func _ready() -> void:
    for d: SkillDef in defs:
        _by_id[d.id] = d


func rank(id: StringName) -> int:
    return _rank.get(id, 0)


func can_unlock(id: StringName) -> bool:
    var d: SkillDef = _by_id.get(id)
    if d == null or rank(id) >= d.max_rank:
        return false
    if progression.level < d.required_level or progression.skill_points < d.cost_per_rank:
        return false
    for req: StringName in d.requires:
        if rank(req) < 1:
            return false
    return true


func unlock(id: StringName) -> bool:
    if not can_unlock(id):
        return false
    progression.skill_points -= _by_id[id].cost_per_rank
    _rank[id] = rank(id) + 1
    rank_changed.emit(id, _rank[id])
    return true


## Refund every rank. Clearing all ranks at once means dependency order does not matter.
func respec() -> void:
    for id: StringName in _rank.keys():
        progression.skill_points += _rank[id] * _by_id[id].cost_per_rank
        _rank[id] = 0
        rank_changed.emit(id, 0)
    _rank.clear()


func save_data() -> Dictionary:
    return _rank.duplicate()


func load_data(data: Dictionary) -> void:
    _rank.clear()
    for id: StringName in data:
        if _by_id.has(id):
            _rank[id] = clampi(int(data[id]), 0, _by_id[id].max_rank)
```

- A listener on `rank_changed` applies the effect: rank 1 grants the ability, higher ranks swap in a stronger `Ability` resource or add a permanent base change.
- Refund one skill only if no other unlocked skill requires it. A full respec, as above, avoids that check.
- Check the tree for cycles when you author it (a skill that requires itself through others can never unlock). A short `@tool` script or a unit test that walks `requires` is enough.
- Save ranks by id, as above, and clamp on load. A save from an older version with a removed skill or a lower `max_rank` then still loads.

## Derived stats

A derived stat is computed from other stats: maximum health from vitality, crit chance from agility. Compute it when a base stat changes, not every frame, and clamp the result so a debuff cannot make it zero or negative.

```gdscript
class_name DerivedStats
extends Node

signal derived_changed(stat: StringName, value: float)

## A StatSet (references/stat-modifiers.md) or any node with get_value() and stat_changed.
@export var stats: Node

var max_health: float = 1.0
var crit_chance: float = 0.0


func _ready() -> void:
    stats.connect(&"stat_changed", _on_stat_changed)
    _recompute()


func _on_stat_changed(stat_name: String, _value: float) -> void:
    if stat_name in ["vitality", "agility", "level"]:
        _recompute()


func _recompute() -> void:
    var vit: float = stats.call(&"get_value", "vitality")
    var agi: float = stats.call(&"get_value", "agility")
    var new_max: float = maxf(1.0, 50.0 + vit * 10.0)
    # Diminishing returns: approaches 50 % but never reaches it.
    var new_crit: float = 0.5 * agi / (agi + 100.0)
    if not is_equal_approx(new_max, max_health):
        max_health = new_max
        derived_changed.emit(&"max_health", max_health)
    if not is_equal_approx(new_crit, crit_chance):
        crit_chance = new_crit
        derived_changed.emit(&"crit_chance", crit_chance)
```

When maximum health changes, decide what happens to current health: keep the same fraction (common), or keep the same number and clamp. Write it down; players notice.

## One damage formula

Put the damage math in one static function. Every attacker (player, enemy, trap) calls it, so a balance change happens in one place, and a unit test can call it without a scene.

```gdscript
class_name DamageMath
extends RefCounted

## Mitigation curve: armor 100 halves the damage, armor 300 quarters it. Never reaches zero.
static func mitigate(raw: float, armor: float) -> float:
    return raw * 100.0 / (100.0 + maxf(armor, 0.0))


## Final damage of one hit. `rng` is passed in so tests and replays are repeatable.
static func resolve(attack: float, armor: float, crit_chance: float, crit_mult: float,
        rng: RandomNumberGenerator) -> Dictionary:
    var is_crit: bool = rng.randf() < crit_chance
    var amount: float = mitigate(attack, armor)
    if is_crit:
        amount *= crit_mult
    return {"amount": maxf(1.0, round(amount)), "crit": is_crit}
```

- Use `float` for percentages and ratios. An `int` truncates 0.25 to 0.
- A minimum of 1 damage stops a high-armor target from being immune. Remove it if immunity is a design goal.
- Pass a `RandomNumberGenerator` with a fixed `seed` in tests; use a shared one seeded at game start in play.
- The hit itself (hitboxes, hurtboxes, knockback) belongs to **godot-component-system** and **godot-combat-system**; this function only answers "how much".
