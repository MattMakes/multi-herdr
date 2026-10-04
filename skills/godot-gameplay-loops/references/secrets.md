# Secrets loop

Hidden content opens when the player does something unusual: enters a button
code, looks at a wall long enough, uses an object many times, or reaches a
completion score. Some secrets stay open across every save slot.

## Pick the trigger

| The player... | Trigger | Section |
| --- | --- | --- |
| presses a sequence of actions in time (cheat code) | sequence matcher | [Sequence](#sequence) |
| looks at a spot for a while | look-at timer | [Look-at](#look-at) |
| uses one object N times | use counter | [Use counter](#use-counter) |
| reaches a completion score | threshold | [Threshold](#threshold) |
| meets a rare event | weighted roll | [Rare event](#rare-event) |

## Sequence

Listen to input events, not to polling in `_process`. A poll misses a
press and release that both happen inside one frame. Keep the recent actions
with their times, and test whether the end of the buffer matches a code. A
match on the end lets the player start a code at any time, and a wrong press
does not clear a correct later attempt.

```gdscript
class_name SequenceMatcher
extends Node

signal sequence_matched(code_id: StringName)

## code id -> the actions in order
@export var codes: Dictionary[StringName, PackedStringArray] = {}
## The longest gap between two presses of one code.
@export var max_gap_msec: int = 600
## The actions the matcher listens to.
@export var watched_actions := PackedStringArray(
	["ui_up", "ui_down", "ui_left", "ui_right", "ui_accept", "ui_cancel"])

var _actions := PackedStringArray()
var _times := PackedInt64Array()


func _unhandled_input(event: InputEvent) -> void:
	if event.is_echo():
		return
	for action: String in watched_actions:
		if event.is_action_pressed(action):
			push_action(StringName(action), Time.get_ticks_msec())
			return


func push_action(action: StringName, now_msec: int) -> void:
	_actions.append(action)
	_times.append(now_msec)
	var longest: int = 1
	for code: PackedStringArray in codes.values():
		longest = maxi(longest, code.size())
	while _actions.size() > longest:
		_actions.remove_at(0)
		_times.remove_at(0)
	for code_id: StringName in codes:
		if _ends_with(codes[code_id]):
			_actions.clear()
			_times.clear()
			sequence_matched.emit(code_id)
			return


func _ends_with(code: PackedStringArray) -> bool:
	var n: int = code.size()
	var start: int = _actions.size() - n
	if n == 0 or start < 0:
		return false
	for i: int in n:
		if _actions[start + i] != code[i]:
			return false
		if i > 0 and _times[start + i] - _times[start + i - 1] > max_gap_msec:
			return false
	return true
```

`push_action` takes the time as an argument, so a headless check feeds a
sequence with fixed times. `_unhandled_input` lets menus and UI take input
first. Put the matcher in a scene that is active in gameplay only, or turn
off its input processing in menus.

Brute force: a macro can try many short codes fast. If a code grants an
advantage, count wrong attempts and stop listening for some seconds after N
fails. A cosmetic secret does not need this.

## Look-at

For "stare at the wall to reveal it", compare the camera's forward vector
with the direction to the spot. A dot product of two unit vectors near 1 means
the camera looks at it. This costs a few multiplies per frame for each
secret, far less than a ray cast from every candidate.

```gdscript
extends Node3D

signal revealed

@export var camera: Camera3D
@export var min_dot: float = 0.97  # about 14 degrees off center
@export var max_distance: float = 6.0
@export var hold_sec: float = 1.5

var _held: float = 0.0
var _done: bool = false


func _process(delta: float) -> void:
	if _done or camera == null:
		return
	var to_spot: Vector3 = global_position - camera.global_position
	var looking: bool = false
	if to_spot.length() <= max_distance:
		var forward: Vector3 = -camera.global_basis.z
		looking = forward.dot(to_spot.normalized()) >= min_dot
	_held = _held + delta if looking else 0.0
	if _held >= hold_sec:
		_done = true
		revealed.emit()
```

The dot test does not know about walls between the camera and the spot. If
that matters, cast one ray only after the dot test passes
(`godot-physics-system`).

## Use counter

Count uses of one object in a time window: keep the times of recent uses,
drop those older than the window, and fire at N. Reset the count when the
player leaves the object's area.

## Threshold

Compute a completion score from the other loop managers (for example found
collectibles, cleared quests, revealed secrets), and open the content at a
fixed percent. Compute it when one of those managers emits a change, not in
`_process`.

## Rare event

A rare vendor or encounter is a weighted roll. Use
`RandomNumberGenerator.rand_weighted()` (4.3 and later) with the "nothing
happens" outcome as one of the weights. Roll on an event (entering an area,
resting), not every frame.

## Meta secrets: one file for all slots

A secret the player keeps across every save slot (a gallery entry, an
unlocked mode, an achievement) does not belong in a slot file. A player who
deletes a slot keeps it. Store these flags in a separate `ConfigFile`:

```gdscript
class_name MetaUnlocks
extends RefCounted

const PATH := "user://meta_unlocks.cfg"


static func unlock(id: StringName) -> void:
	var cfg := ConfigFile.new()
	cfg.load(PATH)  # A missing file is fine: cfg stays empty.
	cfg.set_value("unlocks", String(id), true)
	cfg.save(PATH)


static func is_unlocked(id: StringName) -> bool:
	var cfg := ConfigFile.new()
	if cfg.load(PATH) != OK:
		return false
	return cfg.get_value("unlocks", String(id), false)
```

Per-slot secrets (an opened hidden door in this world) are world flags in
the slot's save data, as in [revival.md](revival.md).

## Design rules

- Hint every secret: a crack in the wall, a different texture, a sound. A
  secret with no cue looks like a bug when a player finds it by chance.
- Mark the discovery: a sound, a particle burst, a short message.
- After a find, keep the spot visible but marked as found (for example a
  faded material and no collision), instead of removing it. Players then
  know the area is done.
- In multiplayer, a secret that gives a gameplay advantage is checked on the
  server before it applies (`godot-multiplayer-sync`).

## Checks

- Feed a code with gaps under `max_gap_msec`: `sequence_matched` fires.
- Feed the same code with one gap over the limit: no match.
- Feed a wrong press, then the full code: the match fires.
- `MetaUnlocks.unlock(&"x")`, then `is_unlocked(&"x")` is `true` in a new
  call.
