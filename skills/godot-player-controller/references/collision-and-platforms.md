# Collision response, one-way and moving platforms

Adds the slide-collision loop, one-way platforms with drop-through, moving platforms, the floor properties and a 2D step-up for small ledges. Read it when the body must react to what it hit, or when platforms or slopes behave wrong.

All code targets Godot 4.7 and `CharacterBody2D`.

## React to each hit after move_and_slide()

`move_and_slide()` can collide several times in one call. Each collision is a `KinematicCollision2D`. Loop over all of them; `get_last_slide_collision()` gives only the last one.

```gdscript
extends CharacterBody2D

signal bumped(collider: Object, normal: Vector2)

@export var speed: float = 200.0


func _physics_process(delta: float) -> void:
    if not is_on_floor():
        velocity += get_gravity() * delta
    velocity.x = Input.get_axis("move_left", "move_right") * speed
    move_and_slide()

    for i in get_slide_collision_count():
        var hit: KinematicCollision2D = get_slide_collision(i)
        var other: Object = hit.get_collider()
        if other is Node and (other as Node).is_in_group("bouncy"):
            velocity = velocity.bounce(hit.get_normal())
        bumped.emit(other, hit.get_normal())
```

For ground detection use `is_on_floor()` or a `ShapeCast2D`. An `Area2D` under the feet reports overlap, not floor contact, and it ignores `floor_max_angle`.

## Floor properties

Set these on the body in the Inspector or in `_ready()`.

| property | default | when to change it |
|---|---|---|
| `floor_max_angle` | 45° in radians | Steeper walkable slopes need a larger angle. |
| `floor_snap_length` | 1.0 | Raise it (4 to 16 px) so a fast body stays on the ground when it runs over the top of a slope or down steps. Snap is skipped while `velocity` points up, so jumps still work. |
| `floor_stop_on_slope` | true | Keep it true so the body does not creep down a slope while idle. |
| `floor_constant_speed` | false | Set it true so speed up and down slopes equals speed on flat ground. |
| `floor_block_on_wall` | true | Set it false if the body sticks when it moves into a wall that rises from a slope. |
| `motion_mode` | `MOTION_MODE_GROUNDED` | Use `MOTION_MODE_FLOATING` for top-down and space games. It has no floor or ceiling; every contact is a wall. |

## One-way platforms

A one-way platform is set on the platform's collider, not on the player.

- On a `CollisionShape2D` or `CollisionPolygon2D`, set `one_way_collision = true`.
- On a TileSet, enable one-way on the tile's physics polygon.
- `one_way_collision_margin` makes the platform thicker for fast bodies. Raise it if fast falls pass through.

Godot 4.7 changed one detail. The pass-through direction of a `CollisionShape2D` now follows the shape's own rotation, and the new `one_way_collision_direction` property (default `Vector2(0, 1)`) sets it. If a project from an older version rotates its one-way shapes, check each one after the upgrade.

### Drop-through

Put one-way platforms on their own physics layer. To drop, clear that bit of the player's `collision_mask` for a short time. Do not move `position` down by a pixel; that skips the collision rules and fails on thick platforms.

```gdscript
extends CharacterBody2D

const ONE_WAY_LAYER: int = 3  # layer number from Project Settings > Layer Names > 2D Physics

@export var speed: float = 200.0
@export var jump_velocity: float = -400.0
@export var drop_time: float = 0.2

var _drop_left: float = 0.0


func _physics_process(delta: float) -> void:
    if not is_on_floor():
        velocity += get_gravity() * delta

    if Input.is_action_just_pressed("jump") and is_on_floor():
        if Input.is_action_pressed("move_down"):
            _drop_left = drop_time
            set_collision_mask_value(ONE_WAY_LAYER, false)
        else:
            velocity.y = jump_velocity

    if _drop_left > 0.0:
        _drop_left -= delta
        if _drop_left <= 0.0:
            set_collision_mask_value(ONE_WAY_LAYER, true)

    velocity.x = Input.get_axis("move_left", "move_right") * speed
    move_and_slide()
```

The drop time must be long enough for the body to fall fully below the platform. If the platform is thicker than one tick of fall, raise `drop_time`.

## Moving platforms

`move_and_slide()` already carries the body with the floor it stands on. Do not add `get_platform_velocity()` to `velocity` yourself; that doubles the platform's motion.

- `platform_on_leave` decides what happens when the body leaves a moving platform:
  - `PLATFORM_ON_LEAVE_ADD_VELOCITY` (default) keeps the platform's velocity, so a jump from a fast lift goes further.
  - `PLATFORM_ON_LEAVE_ADD_UPWARD_VELOCITY` keeps only the upward part.
  - `PLATFORM_ON_LEAVE_DO_NOTHING` keeps none.
- `platform_floor_layers` limits which layers count as moving floors. `platform_wall_layers` does the same for walls that push the body.
- Animate the platform as an `AnimatableBody2D` with `sync_to_physics` on, so its motion is in step with the physics tick.
- Use `get_platform_velocity()` only to read the value, for example to choose an animation.

## Step up a small ledge

`move_and_slide()` does not climb steps. For a 2D step a few pixels high, test whether the body could move forward from a raised position, and lift it if so. `test_move()` runs the body's real shape against the world without moving it.

```gdscript
extends CharacterBody2D

@export var speed: float = 200.0
@export var max_step: float = 8.0


func _physics_process(delta: float) -> void:
    if not is_on_floor():
        velocity += get_gravity() * delta
    velocity.x = Input.get_axis("move_left", "move_right") * speed

    var step: Vector2 = Vector2(velocity.x * delta, 0.0)
    if is_on_floor() and step.x != 0.0 and test_move(global_transform, step):
        var raised: Transform2D = global_transform.translated(Vector2(0.0, -max_step))
        # Free above the step and free forward from there: climb it.
        if not test_move(global_transform, Vector2(0.0, -max_step)) and not test_move(raised, step):
            global_position.y -= max_step
    move_and_slide()
    # Snap brings the body back down onto the step top.
    apply_floor_snap()
```

This is the one place where a position change is right: `test_move()` proved the space is free. Keep `floor_snap_length` at least `max_step` so the body lands on the step top in the same tick.
