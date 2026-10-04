# Tank controls, pixel-art display and many bodies

Adds tank steering, a pixel-art display that does not jitter, and a way to stop off-screen bodies. Read it for a top-down vehicle, a low-resolution game, or a scene with many characters.

All code targets Godot 4.7.

## Tank controls

The left and right actions turn the body. The forward and back actions move it along its own facing. Set `motion_mode = MOTION_MODE_FLOATING` for top-down work, so no contact counts as a floor.

```gdscript
extends CharacterBody2D

@export var drive_speed: float = 160.0
@export var reverse_speed: float = 90.0
@export var turn_speed: float = 3.0  # radians per second


func _ready() -> void:
    motion_mode = MOTION_MODE_FLOATING


func _physics_process(delta: float) -> void:
    var turn: float = Input.get_axis("move_left", "move_right")
    rotation += turn * turn_speed * delta

    var drive: float = Input.get_axis("move_down", "move_up")
    var top_speed: float = drive_speed if drive >= 0.0 else reverse_speed
    # transform.x is the body's forward direction in 2D.
    velocity = transform.x * drive * top_speed
    move_and_slide()
```

For free 8-way movement, read `Input.get_vector()` (SKILL.md section 2). It already limits the diagonal to length 1. Do not add two `get_axis()` values; the diagonal is then about 1.41 times faster.

## Pixel art without jitter

Keep the physics position exact. Round only what is drawn.

- The simplest fix is a project setting: `rendering/2d/snap/snap_2d_transforms_to_pixel = true`. Every 2D node then draws at whole pixels. Use it with the `viewport` stretch mode for a fixed low-resolution game.
- If only some nodes need it, round the sprite in `_process()` and leave the body alone:

```gdscript
extends CharacterBody2D

@onready var _sprite: Sprite2D = $Sprite2D


func _physics_process(delta: float) -> void:
    velocity = Input.get_vector("move_left", "move_right", "move_up", "move_down") * 90.0
    move_and_slide()


func _process(_delta: float) -> void:
    # The body keeps sub-pixel precision; the sprite shows whole pixels.
    _sprite.global_position = global_position.round()
```

Never round `global_position` of the body in `_physics_process`. The rounding error changes the speed and breaks slow movement.

When physics interpolation is on (`physics/common/physics_interpolation`), the engine already smooths the drawn position between ticks. Then prefer the project snap setting over a manual round.

## Many bodies: stop what is off screen

A crowd of characters costs physics time even when no one sees it. A `VisibleOnScreenEnabler2D` turns its target node off when its rectangle leaves the screen and on again when it comes back.

1. Add a `VisibleOnScreenEnabler2D` as a child of the enemy.
2. Size its `rect` to cover the enemy and its attack range.
3. Set `enable_node_path` to the enemy (`..`). Leave `enable_mode` at `ENABLE_MODE_INHERIT`, so the target follows its own `process_mode` when it is on.

If you need custom logic instead, a `VisibleOnScreenNotifier2D` emits `screen_entered` and `screen_exited`:

```gdscript
extends CharacterBody2D

@onready var _notifier: VisibleOnScreenNotifier2D = $VisibleOnScreenNotifier2D


func _ready() -> void:
    _notifier.screen_exited.connect(_on_screen_exited)
    _notifier.screen_entered.connect(_on_screen_entered)


func _on_screen_exited() -> void:
    set_physics_process(false)
    velocity = Vector2.ZERO


func _on_screen_entered() -> void:
    set_physics_process(true)


func _physics_process(delta: float) -> void:
    if not is_on_floor():
        velocity += get_gravity() * delta
    move_and_slide()
```

An off-screen body that stops its own `_physics_process` still collides as a static obstacle. If enemies must keep patrolling off screen, do not use this; reduce their cost another way (see **godot-optimization**).
