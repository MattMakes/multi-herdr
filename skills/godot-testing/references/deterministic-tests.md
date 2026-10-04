Adds rules and helpers for tests that give the same result on every run: choosing the test layer, stepping frames instead of sleeping, waiting for physics, seeding random numbers and seeded fuzzing; read it when a test is flaky, slow, or touches time, physics or randomness.

# Deterministic tests

> ← Back to [SKILL.md](../SKILL.md)

The helpers here use engine signals only, so they work inside GUT and
gdUnit4 tests alike. Call them from a test method with `await`.

## Pick the smallest layer that proves the behaviour

| What you test | Layer | Needs the scene tree |
|---|---|---|
| Pure logic: damage math, inventory rules, parsers | Unit test on a `RefCounted` or `Resource` | No |
| Signal contracts of one node | Unit test with the node added to the test | Yes, one node |
| Node interaction after instantiating a scene | Scene test | Yes |
| Physics contacts, movement over time | Scene test that steps physics frames | Yes |
| Several peers talking | Network test in one process (see [budgets-leaks-and-network-tests.md](budgets-leaks-and-network-tests.md)) | Yes |
| The whole game boots | Smoke run: `godot --headless --path . --quit-after 120` and grep the log | Yes |

Most tests belong in the first two rows. A failure in a large scene test
says "something broke"; a failure in a unit test says what.

## Step frames, never sleep

A test that waits wall-clock time (`OS.delay_msec`, timers of N seconds)
passes on a fast machine and fails on a loaded CI runner. Wait for a
number of frames instead. Physics behaviour advances on physics frames,
not process frames.

```gdscript
# test_support/frames.gd
class_name TestFrames
extends RefCounted


## Wait for `count` process frames.
static func idle(tree: SceneTree, count: int = 1) -> void:
	for i: int in count:
		await tree.process_frame


## Wait for `count` physics ticks. Movement and contacts update here.
static func physics(tree: SceneTree, count: int = 1) -> void:
	for i: int in count:
		await tree.physics_frame


## Wait until `condition` returns true, checking once per physics tick.
## Returns false if it did not happen within `max_ticks`.
static func until(tree: SceneTree, condition: Callable, max_ticks: int = 120) -> bool:
	for i: int in max_ticks:
		if condition.call():
			return true
		await tree.physics_frame
	return bool(condition.call())
```

Usage inside a test method (either framework):

```gdscript
# test_falling_crate.gd - body of a test; shown as a plain helper for the parse check
extends Node


## In a project, the class_name TestFrames is enough; loading by path keeps
## this snippet self-contained.
var frames: GDScript = load("res://test_support/frames.gd")


func check_crate_lands(crate: RigidBody2D, floor_y: float) -> bool:
	# 180 physics ticks = 3 game seconds at 60 ticks per second.
	return await frames.until(get_tree(), func() -> bool: return crate.global_position.y >= floor_y - 1.0, 180)
```

Rules:
- Give `until` a bound. A test that can wait forever hangs the CI job,
  and the CI job usually has no per-test timeout.
- At 60 physics ticks per second, 120 ticks is 2 game seconds. Compute the
  bound from the behaviour, not from a guess in seconds.
- A body moved by setting `global_position` updates its physics state on
  the next physics tick. Wait one tick before you query collisions.

## Under `--headless`, nothing is drawn

Tests run headless in CI. Measured on Godot 4.7.2 with `--headless`:
`RenderingServer.frame_post_draw` is never emitted (a test that awaits it
hangs), and `get_viewport().get_texture().get_image()` returns `null`.

So:
- Never await `frame_post_draw` in a test.
- Do not write screenshot comparison tests for the headless gate. Test the
  data that drives the visuals (SKILL.md, "What NOT to Test"), and use the
  state goldens in [golden-state-tests.md](golden-state-tests.md).

## Seed every random number

Code under test that calls the global `randi()` / `randf()` cannot be
reproduced. Pass a `RandomNumberGenerator` into the system instead, and
give it a fixed seed in tests.

```gdscript
# loot_roller.gd - production code that takes its randomness as a dependency
class_name LootRoller
extends RefCounted

var rng: RandomNumberGenerator


func _init(p_rng: RandomNumberGenerator = null) -> void:
	rng = p_rng if p_rng != null else RandomNumberGenerator.new()


func roll(table: Dictionary) -> StringName:
	# table: item -> weight (int > 0)
	var total: int = 0
	for item: StringName in table:
		total += int(table[item])
	var pick: int = rng.randi_range(1, total)
	for item: StringName in table:
		pick -= int(table[item])
		if pick <= 0:
			return item
	return &""
```

A test builds `RandomNumberGenerator.new()`, sets `seed = 12345`, and then
asserts exact results. Production code builds an unseeded generator.

## Seeded fuzzing

Fuzzing feeds many random inputs and checks invariants (no crash, health
never negative, inventory count never above capacity). Print the seed in
the failure message, so the failing case can be replayed.

```gdscript
# test_support/fuzz.gd
class_name Fuzz
extends RefCounted


## Calls `check(rng)` `runs` times with seeds base_seed, base_seed + 1, ...
## `check` returns "" when the invariant holds, or a failure message.
## Returns "" if every run passed, else the first failure with its seed.
static func run(check: Callable, runs: int = 200, base_seed: int = 1) -> String:
	for i: int in runs:
		var rng := RandomNumberGenerator.new()
		rng.seed = base_seed + i
		var failure: String = check.call(rng)
		if not failure.is_empty():
			return "seed %d: %s" % [base_seed + i, failure]
	return ""
```

```gdscript
# Example invariant check, called from a test as Fuzz.run(check_health_never_negative)
extends RefCounted


func check_health_never_negative(rng: RandomNumberGenerator) -> String:
	var health: int = 100
	for i: int in 50:
		var change: int = rng.randi_range(-500, 500)
		health = clampi(health + change, 0, 100)
		if health < 0:
			return "health %d after change %d" % [health, change]
	return ""
```

Keep the run count small enough for CI (a fuzz test should take well under
a second) and fixed: a fuzz test that picks new seeds each run is flaky
by design.
