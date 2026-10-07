# Platformer feel extras

> proof: not run (needs a human play test) for how a value feels.

Adds air control, ceiling bonk, wall slide with a wall-coyote window, knockback that decays, a dash with an invulnerability signal and a jump-arc debug view. Read it after the platformer controller in SKILL.md works and the jump needs tuning.

All code targets Godot 4.7 and a `CharacterBody2D` with the default `MOTION_MODE_GROUNDED`.

## Rules

- `velocity` is in units per second. Do not multiply it by `delta` before `move_and_slide()`. The body applies the physics step itself.
- Change `velocity`, never `position`, for gameplay motion. A position change skips collision, floor snap and one-way rules.
- Read `is_on_floor()`, `is_on_wall()` and `is_on_ceiling()` after `move_and_slide()`. They describe the last move, not the next one.
- Keep each timer in seconds, not in frames. A frame count changes with `physics/common/physics_ticks_per_second`.

## Air control and ceiling bonk

Ground and air use different acceleration. Low air acceleration makes a jump feel committed. A turn in the air uses full air acceleration, so the player can still correct a jump.

A head hit must end the rise at once. Without the reset, the body stays against the ceiling until gravity cancels the upward speed.

```gdscript
extends CharacterBody2D

@export var run_speed: float = 220.0
@export var ground_accel: float = 1800.0
@export var ground_decel: float = 1600.0
@export var air_accel: float = 700.0
@export var air_decel: float = 200.0
@export var jump_velocity: float = -420.0


func _physics_process(delta: float) -> void:
    if not is_on_floor():
        velocity += get_gravity() * delta

    if Input.is_action_just_pressed("jump") and is_on_floor():
        velocity.y = jump_velocity

    var input_x: float = Input.get_axis("move_left", "move_right")
    var accel: float
    if is_on_floor():
        accel = ground_accel if input_x != 0.0 else ground_decel
    else:
        accel = air_accel if input_x != 0.0 else air_decel
    velocity.x = move_toward(velocity.x, input_x * run_speed, accel * delta)

    move_and_slide()

    # Ceiling bonk: the rise ends on contact.
    if is_on_ceiling() and velocity.y < 0.0:
        velocity.y = 0.0
```

`get_gravity()` returns the gravity at the body, including `Area2D` gravity overrides. Prefer it over a cached `ProjectSettings` value when the level has gravity zones.

## Wall slide and wall-coyote window

A wall slide caps the fall speed while the player presses into a wall. The wall-coyote window lets the player jump a short time after they leave the wall, like coyote time on a ledge. Store the wall normal while in contact, because `get_wall_normal()` is only valid while `is_on_wall()` is true.

```gdscript
extends CharacterBody2D

@export var run_speed: float = 200.0
@export var slide_max_fall: float = 90.0
@export var wall_jump_push: float = 260.0
@export var wall_jump_rise: float = -380.0
@export var wall_coyote_time: float = 0.12
@export var input_lock_time: float = 0.15

var _wall_timer: float = 0.0
var _wall_normal_x: float = 0.0
var _input_lock: float = 0.0


func _physics_process(delta: float) -> void:
    velocity += get_gravity() * delta if not is_on_floor() else Vector2.ZERO
    _input_lock = maxf(_input_lock - delta, 0.0)

    var input_x: float = Input.get_axis("move_left", "move_right")

    # Contact only counts when the player pushes into the wall.
    var pressing_wall: bool = is_on_wall_only() and input_x != 0.0 \
            and signf(input_x) == -signf(get_wall_normal().x)
    if pressing_wall:
        _wall_timer = wall_coyote_time
        _wall_normal_x = get_wall_normal().x
        velocity.y = minf(velocity.y, slide_max_fall)
    else:
        _wall_timer = maxf(_wall_timer - delta, 0.0)

    if Input.is_action_just_pressed("jump") and not is_on_floor() and _wall_timer > 0.0:
        velocity = Vector2(_wall_normal_x * wall_jump_push, wall_jump_rise)
        _wall_timer = 0.0
        # A short lock stops the player from steering straight back into the wall.
        _input_lock = input_lock_time

    if _input_lock <= 0.0:
        velocity.x = move_toward(velocity.x, input_x * run_speed, 1400.0 * delta)

    move_and_slide()
```

`is_on_wall_only()` is true when the body touches a wall and no floor or ceiling. Use it so a slide does not start while the player stands next to a wall.

## Knockback that decays

Keep an external velocity apart from the input velocity. Input then cannot cancel a hit in one frame, and the knockback fades over time at the same rate at any tick rate.

```gdscript
extends CharacterBody2D

@export var move_speed: float = 180.0
@export var knockback_decay: float = 900.0  # units per second, per second

var _knockback: Vector2 = Vector2.ZERO


func apply_knockback(from_position: Vector2, strength: float) -> void:
    var away: Vector2 = (global_position - from_position).normalized()
    _knockback = away * strength


func _physics_process(delta: float) -> void:
    var input_dir: Vector2 = Input.get_vector("move_left", "move_right", "move_up", "move_down")
    var input_velocity: Vector2 = input_dir * move_speed
    _knockback = _knockback.move_toward(Vector2.ZERO, knockback_decay * delta)
    velocity = input_velocity + _knockback
    move_and_slide()
```

Do not decay with `lerp(Vector2.ZERO, 0.2)` each frame. That rate depends on the tick rate.

## Dash with invulnerability

The dash reports its start and end as signals. A hurtbox or health component listens and ignores damage between the two. The dash also refuses to start again until the cooldown ends.

```gdscript
extends CharacterBody2D

signal dash_started
signal dash_ended

@export var dash_speed: float = 650.0
@export var dash_time: float = 0.18
@export var dash_cooldown: float = 0.45
@export var keep_after_dash: float = 0.4  # fraction of dash speed kept on exit

var _dash_left: float = 0.0
var _cooldown_left: float = 0.0
var _dash_dir: Vector2 = Vector2.RIGHT
var _facing: float = 1.0


func is_dashing() -> bool:
    return _dash_left > 0.0


func _physics_process(delta: float) -> void:
    _cooldown_left = maxf(_cooldown_left - delta, 0.0)
    var input_dir: Vector2 = Input.get_vector("move_left", "move_right", "move_up", "move_down")
    if input_dir.x != 0.0:
        _facing = signf(input_dir.x)

    if Input.is_action_just_pressed("dash") and not is_dashing() and _cooldown_left <= 0.0:
        _dash_dir = input_dir.normalized() if input_dir != Vector2.ZERO else Vector2(_facing, 0.0)
        _dash_left = dash_time
        _cooldown_left = dash_time + dash_cooldown
        dash_started.emit()

    if is_dashing():
        _dash_left -= delta
        velocity = _dash_dir * dash_speed
        if _dash_left <= 0.0:
            velocity = _dash_dir * dash_speed * keep_after_dash
            dash_ended.emit()
    else:
        if not is_on_floor():
            velocity += get_gravity() * delta
        velocity.x = move_toward(velocity.x, input_dir.x * 200.0, 1400.0 * delta)

    move_and_slide()
```

To make the player pass through enemies during the dash, clear the enemy bit of `collision_mask` on `dash_started` and set it again on `dash_ended` with `set_collision_mask_value()`.

## Jump-arc debug view

Tune coyote time, jump height and air control while you watch the path. Add this node as a child of the level, set `body`, and remove it before release.

```gdscript
extends Node2D

@export var body: CharacterBody2D
@export var max_points: int = 120

var _trail: PackedVector2Array = PackedVector2Array()


func _physics_process(_delta: float) -> void:
    if body == null:
        return
    _trail.append(to_local(body.global_position))
    if _trail.size() > max_points:
        _trail.remove_at(0)
    queue_redraw()


func _draw() -> void:
    if _trail.size() < 2:
        return
    draw_polyline(_trail, Color.AQUA, 2.0)
    var tip: Vector2 = _trail[_trail.size() - 1]
    draw_line(tip, tip + body.velocity * 0.1, Color.YELLOW, 2.0)
```

The trail samples in `_physics_process`, so each point is one physics tick. Equal spacing means constant speed. The apex is where the points bunch together.
