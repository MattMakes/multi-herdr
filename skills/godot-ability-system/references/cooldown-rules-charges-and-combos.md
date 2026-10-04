# Global and shared cooldowns, charges, combo finishers and saved cooldowns

Adds a global cooldown, cooldown groups that several abilities share, abilities with charges, combo chains that unlock a finisher, the rule for per-caster state in shared Resources, and how to save cooldowns. Read it when the `AbilityComponent` in SKILL.md section 2 needs more than one timer per ability.

All code targets Godot 4.7. The examples are one standalone component; they keep the same contract as SKILL.md (`try_activate()` returns true when the ability fires) so they can replace or extend it.

## Where the timers live

The `Ability` Resource in SKILL.md holds data only. Every runtime value (time left, charges left, combo progress) lives in a component under the caster. One `.tres` file is shared by every caster that uses it; a value stored on it changes for all of them at once.

If a Resource must hold runtime state, give each caster its own copy with `ability.duplicate(true)` when you grant it, or set `resource_local_to_scene = true` on it so each scene instance gets a copy. Status effects that count down are the usual case.

Tick the timers where the gameplay runs. If gameplay runs in `_physics_process`, tick there too, so cooldowns pause and slow down with the game (`get_tree().paused`, `Engine.time_scale`) in the same way as everything else. The HUD only reads the values.

## Global cooldown, groups and charges

Three rules decide whether an ability may fire:

- **Global cooldown (GCD).** Any cast locks every ability that respects the GCD for a short time (0.5 to 1.5 s). It stops button mashing from firing a whole hotbar in one second.
- **Cooldown group.** Abilities that share a group share one timer. Two healing potions, or all summons, block each other.
- **Charges.** An ability stores up to `max_charges` uses. One charge comes back every `recharge_time`. A charge ability can fire while it recharges, as long as one charge is left.

```gdscript
class_name CooldownBook
extends Node

signal charges_changed(ability_id: StringName, charges: int, max_charges: int)
signal group_cooldown_started(group: StringName, duration: float)

@export var global_cooldown: float = 1.0

## ability_id -> settings. Keys: "group" (StringName), "cooldown" (float),
## "max_charges" (int, default 1), "uses_gcd" (bool, default true).
@export var abilities: Dictionary[StringName, Dictionary] = {}

var _gcd_left: float = 0.0
var _group_left: Dictionary[StringName, float] = {}
var _charges: Dictionary[StringName, int] = {}
var _recharge_left: Dictionary[StringName, float] = {}


func _ready() -> void:
    for id: StringName in abilities:
        _charges[id] = _max_charges(id)


func can_use(id: StringName) -> bool:
    if not abilities.has(id):
        return false
    var spec: Dictionary = abilities[id]
    if spec.get("uses_gcd", true) and _gcd_left > 0.0:
        return false
    var group: StringName = spec.get("group", id)
    if _group_left.get(group, 0.0) > 0.0:
        return false
    return _charges.get(id, 0) > 0


## Call after the ability has fired.
func consume(id: StringName) -> void:
    var spec: Dictionary = abilities[id]
    if spec.get("uses_gcd", true):
        _gcd_left = global_cooldown
    _charges[id] -= 1
    charges_changed.emit(id, _charges[id], _max_charges(id))
    var cooldown: float = spec.get("cooldown", 0.0)
    if _max_charges(id) > 1:
        # A charge ability: start the recharge clock if it is not running.
        if _recharge_left.get(id, 0.0) <= 0.0:
            _recharge_left[id] = cooldown
    else:
        # A plain ability: the group timer is its cooldown; the one charge comes back with it.
        var group: StringName = spec.get("group", id)
        _group_left[group] = cooldown
        group_cooldown_started.emit(group, cooldown)


func _physics_process(delta: float) -> void:
    _gcd_left = maxf(_gcd_left - delta, 0.0)
    for group: StringName in _group_left.keys():
        _group_left[group] -= delta
        if _group_left[group] <= 0.0:
            _group_left.erase(group)
            _refill_group(group)
    for id: StringName in _recharge_left.keys():
        _recharge_left[id] -= delta
        if _recharge_left[id] > 0.0:
            continue
        _charges[id] = mini(_charges[id] + 1, _max_charges(id))
        charges_changed.emit(id, _charges[id], _max_charges(id))
        if _charges[id] < _max_charges(id):
            _recharge_left[id] += abilities[id].get("cooldown", 0.0)
        else:
            _recharge_left.erase(id)


func _refill_group(group: StringName) -> void:
    for id: StringName in abilities:
        if _max_charges(id) == 1 and abilities[id].get("group", id) == group:
            _charges[id] = 1
            charges_changed.emit(id, 1, 1)


func _max_charges(id: StringName) -> int:
    return int(abilities[id].get("max_charges", 1))
```

In `AbilityComponent.try_activate()`, replace the `_cooldowns` check with `book.can_use(id)` and call `book.consume(id)` after `activate()`. The `charge_pips.gd` HUD element in `references/ui-binding.md` can listen to `charges_changed`.

- The recharge adds `cooldown` to the time left instead of setting it, so a long frame does not lose the extra time.
- Do not put a movement ability on the GCD. A dodge that waits for a spell's GCD feels broken. Set `"uses_gcd": false` for it.

## Combo finishers

A combo here is a chain of ability casts, not of raw inputs (for button sequences, see **godot-input-handling**). After each cast, add the ability id to a short history. When the end of the history matches a recipe, grant a finisher. The finisher is a normal ability; the combo only makes it available for a short time.

```gdscript
class_name AbilityCombo
extends Node

signal finisher_ready(finisher_id: StringName)
signal finisher_expired(finisher_id: StringName)

@export var chain_window: float = 1.5     # max time between two casts in a chain
@export var finisher_window: float = 2.0  # time the finisher stays available
## finisher id -> chain of ability ids that unlocks it.
@export var recipes: Dictionary[StringName, Array] = {}

var _chain: Array[StringName] = []
var _since_last: float = 0.0
var _ready_finisher: StringName = &""
var _finisher_left: float = 0.0


func register_cast(id: StringName) -> void:
    if id == _ready_finisher:
        _ready_finisher = &""
        _chain.clear()
        return
    if _since_last > chain_window:
        _chain.clear()
    _chain.append(id)
    _since_last = 0.0
    for finisher: StringName in recipes:
        var need: Array = recipes[finisher]
        if _chain.size() >= need.size() and _chain.slice(_chain.size() - need.size()) == need:
            _ready_finisher = finisher
            _finisher_left = finisher_window
            _chain.clear()
            finisher_ready.emit(finisher)
            return


func is_finisher_ready(id: StringName) -> bool:
    return id == _ready_finisher


func _physics_process(delta: float) -> void:
    _since_last += delta
    if _ready_finisher != &"":
        _finisher_left -= delta
        if _finisher_left <= 0.0:
            finisher_expired.emit(_ready_finisher)
            _ready_finisher = &""
```

The finisher's `can_activate()` asks `is_finisher_ready()`. A gameplay tag (`references/tags-and-conditions.md`) works too: add a `combo.finisher_ready` tag on `finisher_ready` and remove it on cast or expiry.

## Saving cooldowns

Choose what a cooldown means while the game is not running:

| meaning | save | load |
|---|---|---|
| Game time (most action games): the timer stops while the game is closed | the seconds left | the seconds left |
| Real time (daily rewards, energy refills): the timer runs while the game is closed | the absolute end time, `Time.get_unix_time_from_system() + left` | `maxf(0.0, end - Time.get_unix_time_from_system())` |

Save charges and recharge time with the cooldowns, or a reload refills every charge. A real-time timer trusts the device clock; a player can move the clock forward. If that matters (an online economy), take the time from a server.

Do not save the GCD or a ready finisher. Both last a second or two; a loaded game starts without them.
