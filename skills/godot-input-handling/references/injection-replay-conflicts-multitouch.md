# Injected input, replays, rebind conflicts and multi-touch

Adds synthetic input for tests and tutorials, a per-tick input recorder and replayer, conflict checks for the rebinding menu, and two-finger pinch and pan. Read it when a test or a demo must press buttons, when you need a replay or a ghost, when two actions end up on one key, or when a touch game needs zoom.

All code targets Godot 4.7.

## Inject input

Two engine calls create input that did not come from hardware.

| call | what it does | use it for |
|---|---|---|
| `Input.action_press(action, strength)` and `Input.action_release(action)` | Changes the action state that `Input.is_action_pressed()` and `get_vector()` read. It sends no event, so `_input()` and `_unhandled_input()` see nothing. | Code that polls `Input` in `_physics_process`. |
| `Input.parse_input_event(event)` | Sends the event through the full pipeline, as if a device had sent it: `_input()`, GUI, `_shortcut_input()`, `_unhandled_input()`, and the action state. | Code that reacts to events, menus, and anything that must see the event in order. |

A synthetic event for an action is an `InputEventAction`:

```gdscript
extends Node


## Press an action for a time, through the full event pipeline.
func tap(action: StringName, hold_time: float = 0.1) -> void:
    var down: InputEventAction = InputEventAction.new()
    down.action = action
    down.pressed = true
    down.strength = 1.0
    Input.parse_input_event(down)

    await get_tree().create_timer(hold_time).timeout

    var up: InputEventAction = InputEventAction.new()
    up.action = action
    up.pressed = false
    Input.parse_input_event(up)
```

- `Input.is_action_just_pressed()` is true for the frame after the event is processed. With input accumulation and buffering, the event can arrive one frame late. In a test, wait at least one physics frame (`await get_tree().physics_frame`) before you assert.
- Do not inject an `InputEventKey` with a fixed keycode to drive an action. The player may have rebound it. Inject the action.
- For a test framework built on this, see **godot-testing**.

## Record and replay per physics tick

To replay a run exactly, record the inputs the game logic reads, on the tick it reads them. Do not record raw events with their render frame number: render frames vary in length, so the replay drifts. A per-tick record of action states replays the same way every time, as long as the game logic itself is deterministic (fixed seed, no wall-clock time).

```gdscript
extends Node

enum Mode { IDLE, RECORDING, REPLAYING }

@export var actions: Array[StringName] = [&"move_left", &"move_right", &"jump", &"attack"]

var mode: Mode = Mode.IDLE
## One entry per physics tick: action -> strength (0 when released).
var _ticks: Array[Dictionary] = []
var _cursor: int = 0


func _ready() -> void:
    # Run before gameplay nodes in each physics tick.
    process_physics_priority = -100


func start_recording() -> void:
    _ticks.clear()
    mode = Mode.RECORDING


func start_replay() -> void:
    _cursor = 0
    mode = Mode.REPLAYING


func _physics_process(_delta: float) -> void:
    match mode:
        Mode.RECORDING:
            var frame: Dictionary = {}
            for action: StringName in actions:
                frame[action] = Input.get_action_strength(action)
            _ticks.append(frame)
        Mode.REPLAYING:
            if _cursor >= _ticks.size():
                _release_all()
                mode = Mode.IDLE
                return
            var frame: Dictionary = _ticks[_cursor]
            for action: StringName in actions:
                var strength: float = frame[action]
                if strength > 0.0:
                    Input.action_press(action, strength)
                else:
                    Input.action_release(action)
            _cursor += 1


func _release_all() -> void:
    for action: StringName in actions:
        Input.action_release(action)
```

- `process_physics_priority = -100` makes this node run its `_physics_process` before nodes with the default priority 0, so gameplay reads the replayed state in the same tick.
- During a replay, ignore real hardware in gameplay code, or the player can disturb it.
- To store a replay, save `_ticks` with `var_to_bytes()` or as JSON. A run of 10 minutes at 60 ticks per second is 36,000 entries; store only the ticks where a value changes if size matters.

## Rebind conflicts

Before the rebinding menu (see `references/action-rebinding.md`) adds a new event to an action, check which other actions already use it. `InputEvent.is_match()` compares two events by key, button or axis and ignores the pressed state.

```gdscript
extends Node

## Actions that share a context and so must not share an input.
@export var gameplay_actions: Array[StringName] = [&"move_left", &"move_right", &"jump", &"attack", &"interact"]


## Actions other than `action` that already use `event`.
func find_conflicts(action: StringName, event: InputEvent) -> Array[StringName]:
    var found: Array[StringName] = []
    for other: StringName in gameplay_actions:
        if other == action:
            continue
        for existing: InputEvent in InputMap.action_get_events(other):
            if existing.is_match(event, true):
                found.append(other)
                break
    return found


## Bind `event` to `action`. With swap true, conflicting actions get the old event.
func rebind(action: StringName, old_event: InputEvent, event: InputEvent, swap: bool) -> void:
    for other: StringName in find_conflicts(action, event):
        for existing: InputEvent in InputMap.action_get_events(other):
            if existing.is_match(event, true):
                InputMap.action_erase_event(other, existing)
        if swap and old_event != null:
            InputMap.action_add_event(other, old_event)
    if old_event != null:
        InputMap.action_erase_event(action, old_event)
    InputMap.action_add_event(action, event)
```

- The second argument of `is_match()` is `exact_match`. With true, modifiers must match too, so Shift+E and E are different inputs.
- Check conflicts only inside one context. A menu action and a gameplay action may share a key on purpose (`ui_accept` and `jump`).
- Never let the player unbind the last event of `ui_cancel` or the pause action. They could then not leave the menu.

## Pinch and pan with two fingers

Each finger has an `index` on `InputEventScreenTouch` and `InputEventScreenDrag`. Keep a dictionary of the fingers that are down. With two fingers, the change in distance is the zoom and the motion of the midpoint is the pan.

```gdscript
extends Camera2D

@export var min_zoom: float = 0.5
@export var max_zoom: float = 3.0

var _fingers: Dictionary[int, Vector2] = {}


func _unhandled_input(event: InputEvent) -> void:
    if event is InputEventScreenTouch:
        var touch: InputEventScreenTouch = event
        if touch.pressed:
            _fingers[touch.index] = touch.position
        else:
            _fingers.erase(touch.index)
    elif event is InputEventScreenDrag:
        var drag: InputEventScreenDrag = event
        if not _fingers.has(drag.index):
            return
        if _fingers.size() == 2:
            _two_finger_update(drag.index, drag.position)
        else:
            # One finger drags the view.
            position -= drag.relative / zoom
        _fingers[drag.index] = drag.position
    elif event is InputEventMagnifyGesture:
        # Trackpad pinch on desktop.
        var pinch: InputEventMagnifyGesture = event
        _set_zoom(zoom.x * pinch.factor)


func _two_finger_update(moved_index: int, new_pos: Vector2) -> void:
    var other_index: int = -1
    for i: int in _fingers:
        if i != moved_index:
            other_index = i
    var other: Vector2 = _fingers[other_index]
    var old_pos: Vector2 = _fingers[moved_index]
    var old_dist: float = old_pos.distance_to(other)
    var new_dist: float = new_pos.distance_to(other)
    if old_dist > 1.0:
        _set_zoom(zoom.x * new_dist / old_dist)
    # The midpoint moves by half the finger's motion.
    position -= (new_pos - old_pos) * 0.5 / zoom


func _set_zoom(value: float) -> void:
    var z: float = clampf(value, min_zoom, max_zoom)
    zoom = Vector2(z, z)
```

- Clear `_fingers` when a touch reports `canceled` or when the app loses focus, or a finger that was lifted outside the window stays "down".
- On desktop, a trackpad sends `InputEventMagnifyGesture` and `InputEventPanGesture`, not touch events. Handle both if the game runs on both.
- Keep **Emulate Touch From Mouse** off for this script during desktop tests: a mouse gives only one finger.
