# Pushdown stacks, guarded and deferred transitions

Adds a pushdown state stack that resumes the state below, transition guards, safe transitions requested from inside `enter()`, a payload for `enter()`, and states that end on a timer. Read it when a state must interrupt another and later give control back (stun, pause, dialogue), or when illegal or re-entrant transitions cause bugs.

All code targets Godot 4.7. The examples are standalone; they do not need the `State` and `StateMachine` classes from SKILL.md section 3, but the same ideas apply to them.

## Pushdown: interrupt, then resume

A flat machine forgets where it came from. A pushdown machine keeps a stack. `push_state()` pauses the current state and starts a new one on top. `pop_state()` ends the top state and resumes the one below. Use it for overlays: a stun over any movement state, a pause menu over gameplay, a dialogue over exploration.

The contract for a state node is four methods: `enter(msg: Dictionary)`, `exit()`, `pause()` and `physics_update(delta: float)`. The stack calls them by name, so a state can extend any node type.

```gdscript
class_name StateStack
extends Node

signal state_changed(previous: Node, current: Node)

@export var initial_state: Node
@export var max_depth: int = 8

var _stack: Array[Node] = []


func _ready() -> void:
    if initial_state != null:
        push_state(initial_state)


func current() -> Node:
    return _stack.back() if not _stack.is_empty() else null


func push_state(state: Node, msg: Dictionary = {}) -> void:
    if _stack.size() >= max_depth:
        push_error("StateStack: depth %d reached; a pop is missing" % max_depth)
        return
    var previous: Node = current()
    if previous != null:
        previous.call(&"pause")  # paused, not exited: it resumes later
    _stack.push_back(state)
    state.call(&"enter", msg)
    state_changed.emit(previous, state)


func pop_state() -> void:
    if _stack.size() <= 1:
        push_error("StateStack: cannot pop the last state")
        return
    var ended: Node = _stack.pop_back()
    ended.call(&"exit")
    var resumed: Node = current()
    resumed.call(&"enter", {"is_resume": true})
    state_changed.emit(ended, resumed)


func replace_state(state: Node, msg: Dictionary = {}) -> void:
    # A plain FSM transition: the top state ends, the new one takes its place.
    var ended: Node = _stack.pop_back()
    ended.call(&"exit")
    _stack.push_back(state)
    state.call(&"enter", msg)
    state_changed.emit(ended, state)


func _physics_process(delta: float) -> void:
    var top: Node = current()
    if top != null:
        top.call(&"physics_update", delta)
```

Rules for the stack:

- A paused state gets `pause()`, not `exit()`. It gets exactly one `exit()`, when it leaves the stack. Two calls to `exit()` stop the same sound or tween twice and can free shared nodes.
- Every push needs a known pop. A stun pops when its timer ends; a menu pops on close. `max_depth` turns a missing pop into an error instead of slow growth.
- Only the top state runs `physics_update`. A state below the top is frozen.

### Resume is not a fresh start

On resume, `enter()` receives `{"is_resume": true}`. Skip the one-time work: the entry sound, the entry animation, timers that must not restart.

```gdscript
extends Node

@export var animation_player: AnimationPlayer


func enter(msg: Dictionary) -> void:
    if msg.get("is_resume", false):
        animation_player.play(&"run")  # continue the loop, no start-up
        return
    animation_player.play(&"run_start")
    animation_player.queue(&"run")


func pause() -> void:
    animation_player.pause()


func exit() -> void:
    animation_player.stop()


func physics_update(_delta: float) -> void:
    pass
```

## Pass data into enter()

A transition often carries data: the hit direction for a stun, the target for an attack. Pass it in the `msg` dictionary instead of a global. The state stays reusable and testable.

```gdscript
extends Node

signal finished

var _knock_dir: Vector2 = Vector2.ZERO
var _time_left: float = 0.0


func enter(msg: Dictionary) -> void:
    if msg.get("is_resume", false):
        return
    _knock_dir = msg.get("direction", Vector2.ZERO)
    _time_left = msg.get("duration", 0.4)


func pause() -> void:
    pass


func exit() -> void:
    _knock_dir = Vector2.ZERO


func physics_update(delta: float) -> void:
    _time_left -= delta
    if _time_left <= 0.0:
        finished.emit()
```

The caller pushes it with `stack.push_state($Stunned, {"direction": hit_dir, "duration": 0.6})`, and connects `finished` to `stack.pop_state`.

For data that many states share for the whole life of the actor (the body, the animation tree, the blackboard), give each state one reference to a context object at start-up. Do not copy it into every message.

## Timed states

Stun, dash and hit-stop end after a fixed time. Count the time in `physics_update` (as above) so a paused state also pauses its timer. A `SceneTreeTimer` from `get_tree().create_timer()` keeps running while the state is paused under the stack, and it has no stop method. Use it only for states that are never paused.

## Guards: block illegal transitions

Keep the allowed transitions in one table. The machine checks it, so a bug that asks for `Dead -> Idle` fails loudly instead of reviving the player.

```gdscript
extends Node

signal transition_refused(from_state: StringName, to_state: StringName)

const ALLOWED: Dictionary[StringName, Array] = {
    &"Idle": [&"Run", &"Jump", &"Attack", &"Dead"],
    &"Run": [&"Idle", &"Jump", &"Attack", &"Dead"],
    &"Jump": [&"Fall", &"Dead"],
    &"Fall": [&"Idle", &"Run", &"Dead"],
    &"Attack": [&"Idle", &"Dead"],
    &"Dead": [],
}

var current_state: StringName = &"Idle"


func can_transition(to_state: StringName) -> bool:
    var targets: Array = ALLOWED.get(current_state, [])
    return to_state in targets


func request(to_state: StringName) -> bool:
    if not can_transition(to_state):
        transition_refused.emit(current_state, to_state)
        return false
    current_state = to_state
    return true
```

- Use `StringName` constants (`&"Run"`) for state names. Put them in one place. A typo in a plain string such as `"Idel"` fails silently.
- A state can also veto its own exit, for example an attack that must finish its animation. Give the state a `can_exit() -> bool` method and check it in `request()` before the table.
- Typed dictionaries (`Dictionary[StringName, Array]`) exist since Godot 4.4.

## Transitions requested inside enter() or exit()

A state that asks for a transition from inside its own `enter()` starts a second transition while the first one is still running. The machine then calls `exit()` on a state whose `enter()` has not returned, and the `state_changed` signal fires in the wrong order.

Queue the request and run it after the current transition ends:

```gdscript
extends Node

signal state_changed(previous: StringName, current: StringName)

var current_state: StringName = &"Idle"
var _busy: bool = false
var _queued: StringName = &""


func transition_to(to_state: StringName) -> void:
    if _busy:
        _queued = to_state  # last request wins
        return
    _busy = true
    var previous: StringName = current_state
    _exit_state(previous)
    current_state = to_state
    _enter_state(to_state)
    state_changed.emit(previous, to_state)
    _busy = false
    if _queued != &"":
        var next: StringName = _queued
        _queued = &""
        transition_to(next)


func _exit_state(_state: StringName) -> void:
    pass  # stop timers, tweens and sounds of the state


func _enter_state(_state: StringName) -> void:
    pass  # may call transition_to(); the call is queued
```

`call_deferred("transition_to", next)` also works. It waits until the end of the frame, so one frame runs in the first state. The queue above runs the second transition at once, in the same tick.
