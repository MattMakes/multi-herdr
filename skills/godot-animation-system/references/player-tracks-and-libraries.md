Adds `AnimationPlayer` work the base skill lacks: the 4.2+ `AnimationMixer` property names, the RESET animation, shared animation libraries, building animations from code, value-track update modes, audio tracks, physics-tick playback, frame-exact `seek()`, syncing to a clock, and off-screen culling. Read it when animations leave properties in a wrong state, when many characters share clips, when code must create or change animations, or when animation must stay in sync with physics, audio or the network.

> ← Back to [SKILL.md](../SKILL.md)

# AnimationPlayer: Tracks, Libraries and Timing

## Names since 4.2: `AnimationMixer`

Since 4.2, `AnimationPlayer` and `AnimationTree` share the base class `AnimationMixer`. Old tutorials use names that no longer apply (checked by unit GW10 against the 4.7.2 doctool dump and godot-docs 4.7):

| Old (4.0 / 4.1) | 4.2+ on `AnimationMixer` |
|---|---|
| `playback_process_mode`, `AnimationPlayer.ANIMATION_PROCESS_*` | `callback_mode_process`, `AnimationMixer.ANIMATION_CALLBACK_MODE_PROCESS_*` |
| `method_call_mode` | `callback_mode_method` |
| `playback_active` | `active` |
| `root_node` on the player only | `root_node` on both player and tree |

The `AnimationPlayer.ANIMATION_PROCESS_*` constants still exist in 4.7 but are deprecated. Use the `AnimationMixer` names in new code.

## The RESET animation

An animation sets properties and leaves them there. Stop a "hit flash" in the middle, and the sprite stays red. Play "open" on a door, save the scene in the editor, and the closed door is saved open.

An animation named exactly `RESET` holds the rest value of every animated property, as one key at time 0. With `reset_on_save` on (the default), the editor applies `RESET` before it saves the scene, so the scene file holds rest values, not the frame you last previewed.

At run time, return to rest by playing it: `anim_player.play(&"RESET")` followed by `anim_player.advance(0)` applies it at once. Add a track to `RESET` for every property that any other animation on the player animates.

## Build an animation in code

Procedural feedback (a bounce whose height depends on damage, a generated cutscene) can build an `Animation` at run time and add it to a library.

```gdscript
# bounce_builder.gd
class_name BounceBuilder
extends RefCounted


## Returns a one-shot squash-and-bounce on a Node2D child named `target_path` (relative to the root node).
static func build(target_path: String, height: float, seconds: float) -> Animation:
    var anim := Animation.new()
    anim.length = seconds
    anim.loop_mode = Animation.LOOP_NONE

    var pos := anim.add_track(Animation.TYPE_VALUE)
    anim.track_set_path(pos, NodePath(target_path + ":position:y"))
    anim.track_set_interpolation_type(pos, Animation.INTERPOLATION_CUBIC)
    anim.track_insert_key(pos, 0.0, 0.0)
    anim.track_insert_key(pos, seconds * 0.4, -height)
    anim.track_insert_key(pos, seconds, 0.0)

    var scale := anim.add_track(Animation.TYPE_VALUE)
    anim.track_set_path(scale, NodePath(target_path + ":scale"))
    anim.track_insert_key(scale, 0.0, Vector2(1.2, 0.8))
    anim.track_insert_key(scale, seconds * 0.4, Vector2(0.9, 1.1))
    anim.track_insert_key(scale, seconds, Vector2.ONE)

    # A method key: call on_landed() on the root node at the end.
    var calls := anim.add_track(Animation.TYPE_METHOD)
    anim.track_set_path(calls, NodePath("."))
    anim.track_insert_key(calls, seconds, {"method": &"on_landed", "args": []})
    return anim


static func install(player: AnimationPlayer, anim_name: StringName, anim: Animation) -> void:
    # get_animation_library() logs an error for a missing name, so test first.
    if not player.has_animation_library(&""):
        player.add_animation_library(&"", AnimationLibrary.new())
    var library := player.get_animation_library(&"")
    if library.has_animation(anim_name):
        library.remove_animation(anim_name)
    library.add_animation(anim_name, anim)
```

- A track path is relative to the mixer's `root_node` (default: the player's parent). `"Sprite2D:position:y"` animates one component; `"Sprite2D:position"` the whole vector.
- A method key is a `Dictionary` with `"method"` and `"args"`.
- Do not change a library while one of its animations plays. Stop the player, change it, then play.

## Value-track update modes

`Animation.value_track_set_update_mode(track, mode)` decides how a value track writes its property:

| Mode | Writes | Use for |
|---|---|---|
| `UPDATE_CONTINUOUS` | every frame, interpolated | position, color, any float |
| `UPDATE_DISCRETE` | only when the play head crosses a key | `visible`, an enum state, a texture swap, `frame` on a sprite |
| `UPDATE_CAPTURE` | blends from the current value to the first key | a "go to pose" from wherever the object is |

A `bool` or `int` animated with `UPDATE_CONTINUOUS` toggles at the halfway point between keys, which looks like a one-frame glitch. Use `UPDATE_DISCRETE` for them.

## Audio tracks

An audio track (`Animation.TYPE_AUDIO`) plays a stream on an `AudioStreamPlayer`, `AudioStreamPlayer2D` or `AudioStreamPlayer3D` at an exact time in the clip. Prefer it over a method key that calls `play()`: the audio track seeks with the animation, scrubs in the editor, and stops when the animation stops.

```gdscript
# footstep_track.gd
class_name FootstepTrack
extends RefCounted


## Adds footstep sounds at `times` (seconds) to `anim`, played by the node at `player_path`.
static func add(anim: Animation, player_path: NodePath, step: AudioStream, times: PackedFloat32Array) -> void:
    var track := anim.add_track(Animation.TYPE_AUDIO)
    anim.track_set_path(track, player_path)
    for t in times:
        anim.audio_track_insert_key(track, t, step)
```

## Physics tick and manual stepping

- **A player that moves a physics body** (a moving platform, a door with collision) must run on the physics tick: `callback_mode_process = AnimationMixer.ANIMATION_CALLBACK_MODE_PROCESS_PHYSICS`. On the default idle tick, the body moves between physics steps, and characters on it jitter or slide.
- **Deterministic stepping** (replays, lockstep multiplayer, tests): set `callback_mode_process = ANIMATION_CALLBACK_MODE_PROCESS_MANUAL` and call `advance(delta)` yourself from the simulation step.
- **Method keys** run deferred by default (`callback_mode_method = ANIMATION_CALLBACK_MODE_METHOD_DEFERRED`), at the end of the frame. Set `ANIMATION_CALLBACK_MODE_METHOD_IMMEDIATE` when a hitbox enabled by a method key must be active in the same physics step.

## Exact positions with `seek()`

`seek(seconds, update)` moves the play head. With `update = false` (the default), the new pose is applied at the next process step, so a read of the animated property right after `seek()` still sees the old value. Pass `update = true` when code reads the result at once (a thumbnail, a test, a networked pose).

To keep a long animation in sync with something else (music, a server clock), set its position from that clock each frame, `seek(clock_time, true)`, instead of trusting `speed_scale`. Two players at the same `speed_scale` drift apart over time, because each one adds up its own frame deltas, and a hitch, a pause or a time-scale change reaches them differently.

## Loops and the finished signal

`animation_finished` is emitted when a non-looping animation reaches its end. A looping animation (`loop_mode = LOOP_LINEAR` or `LOOP_PINGPONG`) does not finish, so code that waits for it waits forever. To act on each loop of a cycle, put a method key at the end of the clip. (`AnimatedSprite2D` has an `animation_looped` signal; `AnimationPlayer` does not.)

## Off-screen culling

A crowd of animated characters costs CPU even when no camera sees them. For visual-only animation (no gameplay depends on the pose), stop processing when off screen:

```gdscript
# offscreen_animation_culler.gd
class_name OffscreenAnimationCuller
extends VisibleOnScreenNotifier3D

@export var mixer: AnimationMixer


func _ready() -> void:
    screen_entered.connect(func() -> void: mixer.active = true)
    screen_exited.connect(func() -> void: mixer.active = false)
    mixer.active = is_on_screen()
```

Size the notifier's `aabb` to cover the whole character, including raised arms, or limbs pop when the box leaves the screen first. Keep gameplay animation (a boss whose attack timing comes from its animation) active.
