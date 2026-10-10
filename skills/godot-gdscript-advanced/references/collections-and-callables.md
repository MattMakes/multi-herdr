# Typed collections, functional array methods and Callables as values

Adds the array methods that take a `Callable`, functions that return
lambdas, safe erasing during iteration, `dict.get()` with a default, and
pre-sized packed arrays. Read it when you filter, sort or reduce data, or
when you pass behavior as a value. Measured on Godot 4.7.2 where marked.

## Functional array methods

`Array` has `filter`, `map`, `reduce`, `any`, `all` and `sort_custom`. Each
takes a `Callable`. `filter` and `map` return a new untyped `Array`, so
assign the result to a typed array with `assign()`.

```gdscript
extends Node

class Unit:
	var name: String
	var hp: int
	var friendly: bool

	func _init(p_name: String = "", p_hp: int = 0, p_friendly: bool = false) -> void:
		name = p_name
		hp = p_hp
		friendly = p_friendly


func summary(units: Array[Unit]) -> String:
	var hostile: Array[Unit] = []
	hostile.assign(units.filter(func(u: Unit) -> bool: return not u.friendly))
	var total_hp: int = hostile.reduce(func(sum: int, u: Unit) -> int: return sum + u.hp, 0)
	var all_alive := units.all(func(u: Unit) -> bool: return u.hp > 0)
	var any_boss := units.any(func(u: Unit) -> bool: return u.name.begins_with("Boss"))
	var names := ", ".join(PackedStringArray(hostile.map(func(u: Unit) -> String: return u.name)))
	return "%s: %d hp, all alive %s, boss %s" % [names, total_hp, all_alive, any_boss]
```

`reduce(callable, accum)` starts from `accum`. Without `accum` it starts
from the first element. Measured: `[1, 2, 3, 4].reduce(func(a, b): return
a + b)` gives `10`. Give `accum` when the array can be empty or when the
result type differs from the element type.

There is no `Array.join()`. Join strings with `String.join()` on a
`PackedStringArray`, as in the example.

## Functions that return behavior

A function can build and return a lambda. The lambda keeps the values it
captured.

```gdscript
extends Node


func damage_scaler(multiplier: float) -> Callable:
	return func(base: float) -> float: return base * multiplier


func apply_all(values: PackedFloat32Array, step: Callable) -> PackedFloat32Array:
	var out := PackedFloat32Array()
	out.resize(values.size())
	for i in values.size():
		out[i] = step.call(values[i])
	return out


func _ready() -> void:
	var crit := damage_scaler(2.5)
	print(apply_all(PackedFloat32Array([10.0, 4.0]), crit))
```

The SKILL.md explains that a lambda captures locals by value. A returned
lambda is the safe use of that rule: the captured `multiplier` never
changes.

## Erasing while you iterate

Measured on 4.7.2: erasing keys inside `for key in dict:` skips entries
and gives no error. A loop over three keys that erased each key left two
keys in the dictionary. Iterate over a copy of the keys:

```gdscript
extends Node

var _timers: Dictionary[StringName, float] = {}


func expire(now: float) -> void:
	for key in _timers.keys():
		if _timers[key] <= now:
			_timers.erase(key)
```

`keys()` returns a new array, so the erase does not change what the loop
walks. The same rule holds for an array: walk backwards with an index, or
build a new array with `filter()`.

## Lookups with a default

`dict[key]` on a missing key is an error. `dict.get(key, default)` returns
the default. Use `has()` when "missing" and "the default value" must mean
different things.

```gdscript
extends Node

var _stats: Dictionary[String, float] = {"hp": 100.0}


func stat(stat_name: String) -> float:
	return _stats.get(stat_name, 0.0)
```

## Pre-sized packed arrays

When the final size is known, `resize()` once and write by index. It avoids
a reallocation per `append()`.

```gdscript
extends Node


func grid_points(width: int, height: int, spacing: float) -> PackedVector2Array:
	var points := PackedVector2Array()
	points.resize(width * height)
	for y in height:
		for x in width:
			points[y * width + x] = Vector2(x, y) * spacing
	return points
```

Index writes into a packed array held in a local variable are fine. For a
packed array *property* with a setter, the SKILL.md's Godot 4.7 note
applies: change a copy and assign it back.
