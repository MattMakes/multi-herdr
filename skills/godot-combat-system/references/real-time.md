# Real-time combat

This reference adds a typed hit and a resolver to the hitbox, hurtbox and
health components of `godot-component-system`. Read that skill first. Do not
write a second health or hurtbox class.

## The hit

```gdscript
class_name DamageInfo
extends RefCounted

enum Type { PHYSICAL = 1, FIRE = 2, ICE = 4, LIGHTNING = 8, POISON = 16 }

var amount: int = 0
var types: int = Type.PHYSICAL
var source: Node = null
var knockback := Vector3.ZERO
var is_critical: bool = false
## Ignores the target's defense stat (not its resistances).
var pierces_defense: bool = false


func _init(p_amount: int = 0, p_types: int = Type.PHYSICAL, p_source: Node = null) -> void:
	amount = p_amount
	types = p_types
	source = p_source


func has_type(t: Type) -> bool:
	return types & t != 0
```

`DamageInfo` is a `RefCounted`: one is made per hit and freed when the last
reference goes. For damage authored in the editor, put the base numbers in a
`Resource` (an attack definition) with
`@export_flags("Physical", "Fire", "Ice", "Lightning", "Poison") var types: int = 1`,
and build a `DamageInfo` from it at hit time. The flag names list maps to the
values 1, 2, 4, 8 and 16, in order, so they match the enum.

## The defender's stats

```gdscript
class_name DefenseStats
extends Resource

@export var defense: int = 0
## Multiplier per damage type: 0.5 halves fire, 0 makes it immune,
## 1.5 makes it weak. A missing type means 1.0.
@export var resistances: Dictionary[DamageInfo.Type, float] = {}
```

## The resolver

One pure function turns a hit and the target's stats into the final number.
Pure means no nodes and no side effects, so it is easy to test.

```gdscript
class_name DamageResolver
extends RefCounted

const CRIT_MULTIPLIER: float = 1.5
const MIN_DAMAGE: int = 1


## Rolls the critical hit. Call it once, when the hit is built.
static func roll_critical(hit: DamageInfo, chance: float, rng: RandomNumberGenerator) -> void:
	hit.is_critical = rng.randf() < chance


static func resolve(hit: DamageInfo, stats: DefenseStats) -> int:
	var value: float = hit.amount
	if hit.is_critical:
		value *= CRIT_MULTIPLIER
	if not hit.pierces_defense:
		value -= stats.defense
	# With several types, the target takes the strongest multiplier.
	var best: float = -1.0
	for t: DamageInfo.Type in DamageInfo.Type.values():
		if hit.has_type(t):
			best = maxf(best, stats.resistances.get(t, 1.0))
	if best >= 0.0:
		value *= best
	if best == 0.0:
		return 0
	return maxi(roundi(value), MIN_DAMAGE)
```

Design choices to state in the code, not to guess: the order of defense and
multipliers, the minimum damage, and how several types combine. The function
above takes defense first, then the strongest multiplier, with a floor of 1
unless the target is immune.

## Wire it to the components

The `godot-component-system` hitbox sends an `int` to
`HurtboxComponent.receive_hit(damage)`. For typed hits, add one method to the
hurtbox that resolves the hit and then calls the existing path, and have the
hitbox call it:

```gdscript
extends Area2D

@export var defense_stats: DefenseStats

signal hit_resolved(hit: DamageInfo, final_amount: int)


func receive_attack(hit: DamageInfo) -> void:
	var final_amount: int = DamageResolver.resolve(hit, defense_stats)
	hit_resolved.emit(hit, final_amount)
	if has_method(&"receive_hit"):
		call(&"receive_hit", final_amount)
```

Here the snippet stands for the hurtbox script: the `has_method` call reaches
the component's existing `receive_hit`, which keeps the i-frame gate. The
hitbox builds a `DamageInfo` from its attack data and calls
`area.receive_attack(hit)` where it called `area.receive_hit(damage)`.
`hit_resolved` feeds knockback, the damage number and the hit sound.

## One hit per target per swing

```gdscript
class_name SwingTracker
extends RefCounted

var _hit_ids: Dictionary[int, bool] = {}


func begin_swing() -> void:
	_hit_ids.clear()


## Returns true the first time a target is hit in this swing.
func try_hit(target: Object) -> bool:
	var id: int = target.get_instance_id()
	if _hit_ids.has(id):
		return false
	_hit_ids[id] = true
	return true
```

Call `begin_swing()` from the attack animation's first active frame (a
method track), and `try_hit(area)` before you send the hit. This is a
per-swing rule; the component's hitbox cooldown is a per-time rule. A
multi-hit attack calls `begin_swing()` once per hit.

## Hit-stop

```gdscript
extends Node

var _stop_until_msec: int = 0
var _stop_count: int = 0


func hit_stop(duration_sec: float, time_scale: float = 0.05) -> void:
	var until: int = Time.get_ticks_msec() + int(duration_sec * 1000.0)
	if until <= _stop_until_msec:
		return
	_stop_until_msec = until
	_stop_count += 1
	var this_stop: int = _stop_count
	Engine.time_scale = time_scale
	# process_always, not in physics, ignore_time_scale
	await get_tree().create_timer(duration_sec, true, false, true).timeout
	if this_stop == _stop_count:
		Engine.time_scale = 1.0
```

Put it in an autoload. The fourth argument of `create_timer` makes the timer
run on real time while `Engine.time_scale` is low. Overlapping calls extend
the stop; only the last timer restores the scale. Compare the stop count,
not the clock: on 4.7.2 a 0.1 s timer can end before
`Time.get_ticks_msec()` has moved 100 ms (measured: 0 ms and 99 ms), and a
clock test then leaves the game slowed for ever (proof: headless-run). If the game uses
`time_scale` for slow motion, store and restore that value instead of 1.0.

## Knockback

```gdscript
extends CharacterBody3D

@export var knockback_decay: float = 12.0

var _knockback := Vector3.ZERO


func apply_knockback(impulse: Vector3) -> void:
	_knockback += impulse


func _physics_process(delta: float) -> void:
	velocity = _knockback  # add the movement velocity here as well
	_knockback = _knockback.move_toward(Vector3.ZERO, knockback_decay * delta)
	move_and_slide()
```

Compute the impulse from the attacker to the target:
`(target.global_position - source.global_position).normalized() * force`,
with the `y` part set by the design.

## Death cleanup

```gdscript
extends CharacterBody3D

@export var body_shape: CollisionShape3D
@export var hurtbox_shape: CollisionShape3D


func _on_died() -> void:
	body_shape.set_deferred(&"disabled", true)
	hurtbox_shape.set_deferred(&"disabled", true)
	set_physics_process(false)
	# Play the death animation; free the node on its finished signal.
```

`died` usually fires inside a physics callback, so shape changes are deferred.

## Online damage

The server applies damage; a client only asks. The client sends "I hit
target X with attack Y" by RPC. The server checks that the attacker is alive,
the attack is off cooldown, and the target is in range (allow for latency),
then runs the resolver and replicates health. Never trust a damage number
from a client. `godot-multiplayer-sync` covers RPC and authority.

## Debug

`get_tree().debug_collisions_hint = true` draws collision shapes at run
time; the editor's Debug menu has the same switch. Give hitboxes and
hurtboxes different `debug_color` values on their `CollisionShape3D` or
`CollisionShape2D`, so the overlay shows which is which.

## Checks

- 10 fire damage, defense 2, fire resistance 0.5: final 4.
- The same hit with `is_critical`: `(15 - 2) * 0.5 = 6.5`, rounded to 7
  (`roundi` rounds half away from zero).
- A fire immunity (0.0): final 0.
- `try_hit` twice on one target in one swing: `true`, then `false`.
