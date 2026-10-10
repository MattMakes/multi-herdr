# Action buffers, combos, echo and hold-or-toggle

Adds a reusable buffer for many actions, a timed combo detector, the rule for key-repeat (echo) events, and a hold-or-toggle setting for accessibility. Read it when one-off buffer variables multiply, when a fighting or action game needs input sequences, or when a settings menu must offer toggle instead of hold.

All code targets Godot 4.7. SKILL.md section 3 and `references/input-buffering.md` show a buffer for one action; this file generalizes it.

## A buffer for many actions

Each buffered action has its own window. The buffer counts down on the physics tick, so the window means the same time at 30 or 240 rendered frames per second. A consumer asks `consume()`: it returns true once and clears the entry, so one press never fires two moves.

```gdscript
class_name ActionBuffer
extends Node

## Window in seconds for each action this buffer tracks.
@export var windows: Dictionary[StringName, float] = {
    &"jump": 0.12,
    &"dash": 0.10,
    &"attack": 0.20,
}

var _left: Dictionary[StringName, float] = {}


func _unhandled_input(event: InputEvent) -> void:
    if event.is_echo():
        return
    for action: StringName in windows:
        if event.is_action_pressed(action):
            _left[action] = windows[action]


func _physics_process(delta: float) -> void:
    for action: StringName in _left.keys():
        _left[action] -= delta
        if _left[action] <= 0.0:
            _left.erase(action)


## True once if the action was pressed within its window; clears it.
func consume(action: StringName) -> bool:
    if _left.has(action):
        _left.erase(action)
        return true
    return false


## True if the action is waiting, without clearing it.
func peek(action: StringName) -> bool:
    return _left.has(action)


func clear() -> void:
    _left.clear()
```

A player script calls `buffer.consume(&"jump")` only at a moment the jump can happen, for example `if is_on_floor() and buffer.consume(&"jump")`. When two buffered actions compete in one tick, the caller decides the order by the order of its `consume()` calls. Call `clear()` on a cutscene or death, so a stale press does not fire afterwards.

## Combos: timed input sequences

A combo is a sequence of actions where each step follows the last within a gap. Keep a short history of `(action, time)` and match the end of it against each known sequence. Measure time with the physics clock, not with the wall clock, so a slow frame or a paused game does not break the gap.

```gdscript
extends Node

signal combo_performed(combo_name: StringName)

## Longest pause allowed between two steps, in seconds.
@export var max_gap: float = 0.35
@export var tracked: Array[StringName] = [&"move_down", &"move_right", &"move_left", &"attack"]

var combos: Dictionary[StringName, Array] = {
    &"fireball": [&"move_down", &"move_right", &"attack"],
    &"uppercut": [&"move_right", &"move_down", &"move_right", &"attack"],
}

var _history: Array[StringName] = []
var _times: PackedFloat64Array = PackedFloat64Array()
var _clock: float = 0.0


func _physics_process(delta: float) -> void:
    _clock += delta


func _unhandled_input(event: InputEvent) -> void:
    if event.is_echo():
        return
    for action: StringName in tracked:
        if event.is_action_pressed(action):
            _record(action)
            return


func _record(action: StringName) -> void:
    # A long pause starts a new sequence.
    if not _times.is_empty() and _clock - _times[_times.size() - 1] > max_gap:
        _history.clear()
        _times.clear()
    _history.append(action)
    _times.append(_clock)
    if _history.size() > 8:
        _history.remove_at(0)
        _times.remove_at(0)
    _match()


func _match() -> void:
    # Check longer combos first, so a long one is not hidden by its short tail.
    var names: Array = combos.keys()
    names.sort_custom(func(a: StringName, b: StringName) -> bool:
        return combos[a].size() > combos[b].size())
    for combo_name: StringName in names:
        var steps: Array = combos[combo_name]
        if _history.size() < steps.size():
            continue
        if _history.slice(_history.size() - steps.size()) == steps:
            _history.clear()
            _times.clear()
            combo_performed.emit(combo_name)
            return
```

- A diagonal on a stick or a d-pad fires two direction actions close together. If a combo needs a diagonal, add it as its own action, or accept either order of the two directions.
- Fighting games often allow the final button a little after the last direction. Raise `max_gap` for the last step only if players miss combos.

## Echo: key repeat

When a key stays down, the operating system sends repeat events. In Godot each one is an `InputEvent` with `is_echo()` true.

- `event.is_action_pressed(action)` ignores echoes by default. Its second argument, `allow_echo`, turns them on.
- Menu movement should repeat: holding down moves the selection down the list. Godot's `ui_up` and `ui_down` handling in `Control` already allows echo.
- Gameplay presses and confirm buttons should not repeat. A held confirm key must not skip ten lines of dialogue.

```gdscript
extends Control

signal confirmed
signal moved(step: int)


func _unhandled_input(event: InputEvent) -> void:
    # Confirm: one press, one confirm. Echo is ignored by default.
    if event.is_action_pressed(&"ui_accept"):
        confirmed.emit()
        accept_event()
    # Navigation: allow_echo = true, so holding the key scrolls.
    elif event.is_action_pressed(&"ui_down", true):
        moved.emit(1)
        accept_event()
    elif event.is_action_pressed(&"ui_up", true):
        moved.emit(-1)
        accept_event()
```

A raw `InputEventKey` check (`event is InputEventKey and event.pressed`) does not filter echo. Add `and not event.is_echo()` to it.

## Hold or toggle

Some players cannot hold a button for a long time. Offer each long-hold action (sprint, aim, crouch, block) as hold or toggle in the settings. Read the setting in one place; the rest of the game reads only the resulting state.

```gdscript
extends Node

signal changed(action: StringName, active: bool)

## Actions that the player may switch between hold and toggle.
@export var toggle_mode: Dictionary[StringName, bool] = {
    &"sprint": false,
    &"aim": false,
    &"crouch": false,
}

var _active: Dictionary[StringName, bool] = {}


func is_active(action: StringName) -> bool:
    return _active.get(action, false)


func set_toggle(action: StringName, use_toggle: bool) -> void:
    toggle_mode[action] = use_toggle
    _apply(action, false)  # start from a known state after a settings change


func _unhandled_input(event: InputEvent) -> void:
    for action: StringName in toggle_mode:
        if not event.is_action(action) or event.is_echo():
            continue
        if toggle_mode[action]:
            if event.is_action_pressed(action):
                _apply(action, not is_active(action))
        else:
            _apply(action, event.is_action_pressed(action))


func _apply(action: StringName, value: bool) -> void:
    if is_active(action) == value:
        return
    _active[action] = value
    changed.emit(action, value)
```

Save `toggle_mode` with the other input settings (see `references/action-rebinding.md` for `ConfigFile`). Turn toggled states off when the game pauses or a menu opens; a sprint that stays on after a menu closes surprises the player.
