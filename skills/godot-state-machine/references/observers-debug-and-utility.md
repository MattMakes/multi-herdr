# State observers, debugging and utility selection

Adds a `state_changed` signal that drives animation and sound from outside the states, a transition history for debugging, an on-screen state label, and utility scoring that picks the best state each tick. Read it when states start to call animation and audio code directly, when a transition bug is hard to reproduce, or when an AI must choose between many actions.

All code targets Godot 4.7.

## Observers: keep presentation out of the states

When each state's `enter()` plays its own animation and sound, a state change touches three systems and every new state repeats the same lines. Emit one signal from the machine instead, and let small observer nodes react. Add to the `StateMachine` from SKILL.md section 3:

```gdscript
signal state_changed(previous: StringName, current: StringName)
```

and emit it in `transition_to()` after the new state's `enter()`.

### Animation observer

Map each state name to a node in an `AnimationTree` state machine and travel there. `travel()` follows the graph's transitions, so blends and cross-fades still apply.

```gdscript
extends Node

## Path to the state machine node inside the AnimationTree.
@export var animation_tree: AnimationTree
@export var state_machine: Node
## Logic state name -> AnimationTree state name. Missing names use the logic name.
@export var name_map: Dictionary[StringName, StringName] = {}

var _playback: AnimationNodeStateMachinePlayback


func _ready() -> void:
    _playback = animation_tree.get("parameters/playback") as AnimationNodeStateMachinePlayback
    state_machine.connect(&"state_changed", _on_state_changed)


func _on_state_changed(_previous: StringName, current: StringName) -> void:
    var target: StringName = name_map.get(current, current)
    _playback.travel(target)
```

`parameters/playback` is the playback object when the tree root is an `AnimationNodeStateMachine`. For a nested machine, use `parameters/<NodeName>/playback`.

### Sound observer

```gdscript
extends AudioStreamPlayer

@export var state_machine: Node
@export var sounds: Dictionary[StringName, AudioStream] = {}


func _ready() -> void:
    state_machine.connect(&"state_changed", _on_state_changed)


func _on_state_changed(_previous: StringName, current: StringName) -> void:
    var clip: AudioStream = sounds.get(current)
    if clip != null:
        stream = clip
        play()
```

An observer only listens. It never calls `transition_to()`; a change that starts from presentation code makes the logic depend on the view.

## Transition history

A wrong transition is easier to find when the last few transitions are on record. Keep a small ring buffer and print it when something goes wrong.

```gdscript
extends Node

@export var state_machine: Node
@export var capacity: int = 32

var _entries: PackedStringArray = PackedStringArray()


func _ready() -> void:
    state_machine.connect(&"state_changed", _on_state_changed)


func _on_state_changed(previous: StringName, current: StringName) -> void:
    var frame: int = Engine.get_physics_frames()
    _entries.append("%d: %s -> %s" % [frame, previous, current])
    if _entries.size() > capacity:
        _entries.remove_at(0)


func dump() -> String:
    return "\n".join(_entries)
```

Call `dump()` from an `assert` message or an error handler. The physics frame number shows when two transitions happened in the same tick, which points at a re-entrant transition (see `pushdown-and-transitions.md`).

Two transitions back and forth every tick (`Idle -> Run -> Idle`) mean two conditions overlap. Give one of them a margin, for example start running above speed 10 and stop below speed 5.

## On-screen state label

Show the current state above the actor while testing. A `Label` child is enough; `_draw()` is not needed.

```gdscript
extends Label

@export var state_machine: Node


func _ready() -> void:
    state_machine.connect(&"state_changed", _on_state_changed)
    visible = OS.is_debug_build()


func _on_state_changed(_previous: StringName, current: StringName) -> void:
    text = String(current)
```

`OS.is_debug_build()` is false in a release export, so the label hides itself there.

## Utility selection

A designer-written transition table gets large when an AI has many actions. Utility selection lets each state score itself from the current situation. Each tick the machine moves to the highest score. Add a margin so two close scores do not swap every tick.

```gdscript
extends Node

signal state_changed(previous: StringName, current: StringName)

## Required lead over the current state's score before a switch.
@export var switch_margin: float = 0.1

var current_state: Node = null


func _physics_process(_delta: float) -> void:
    var best: Node = current_state
    var best_score: float = _score(current_state) + switch_margin
    for child in get_children():
        if child == current_state or not child.has_method(&"score"):
            continue
        var s: float = _score(child)
        if s > best_score:
            best = child
            best_score = s
    if best != current_state:
        var previous: StringName = current_state.name if current_state != null else &""
        if current_state != null:
            current_state.call(&"exit")
        current_state = best
        current_state.call(&"enter", {})
        state_changed.emit(previous, best.name)


func _score(state: Node) -> float:
    if state == null:
        return -INF
    return float(state.call(&"score"))
```

A child state implements `score() -> float` from what it can see, for example:

```gdscript
extends Node

@export var body: CharacterBody2D
@export var health: Node


func score() -> float:
    var hp_ratio: float = float(health.get(&"current")) / float(health.get(&"maximum"))
    # Flee grows from 0 at half health to 1 at zero health.
    return clampf((0.5 - hp_ratio) * 2.0, 0.0, 1.0)


func enter(_msg: Dictionary) -> void:
    pass


func exit() -> void:
    pass
```

Keep every score in the same range (0 to 1 here), or one state wins every time. For decisions that need sequences and fallbacks, use a behavior tree instead (**godot-limboai**, **godot-beehave**).
