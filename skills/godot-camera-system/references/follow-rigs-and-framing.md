# Frame-rate independent smoothing, decoupled rigs and multi-target framing

Adds smoothing that behaves the same at any frame rate, exponential zoom, a camera rig that does not inherit the player's rotation or physics jitter, a camera that frames several targets, and a priority-based camera director. Read it when the camera feels different at 30 and 144 frames per second, when it spins or shakes with the player, or when it must keep two or more players in view.

All code targets Godot 4.7.

## Smoothing that ignores the frame rate

`position.lerp(target, speed * delta)` is common, and it is wrong in a small way. The fraction it moves each frame is a straight line in `delta`, but the result over many frames is not. The same `speed` follows faster at a low frame rate than at a high one, and at a very low frame rate `speed * delta` can pass 1 and overshoot.

Use an exponential weight instead. `1.0 - exp(-speed * delta)` gives the same curve over time at any frame rate, and it never passes 1.

```gdscript
extends Camera2D

@export var target: Node2D
## Higher is snappier. About 5 is soft, 15 is tight.
@export var follow_rate: float = 8.0


func _process(delta: float) -> void:
    if target == null:
        return
    var weight: float = 1.0 - exp(-follow_rate * delta)
    global_position = global_position.lerp(target.global_position, weight)
```

The same fix applies to the look-ahead and follow code in SKILL.md section 2 and to the 3D follow in `references/camera3d-patterns.md`: replace each `speed * delta` weight with `1.0 - exp(-speed * delta)`.

`position_smoothing_enabled` on `Camera2D` is a built-in alternative. Use it when you do not need custom follow logic.

### Exponential zoom

Zoom feels even when each step multiplies the zoom by the same factor. A linear step (`zoom += 0.1`) is a large change when zoomed out and a small one when zoomed in. Smooth zoom in log space:

```gdscript
extends Camera2D

@export var zoom_rate: float = 10.0
@export var min_zoom: float = 0.25
@export var max_zoom: float = 4.0
@export var wheel_step: float = 1.15  # each wheel notch multiplies zoom by this

var _target_zoom: float = 1.0


func _unhandled_input(event: InputEvent) -> void:
    if event is InputEventMouseButton and event.pressed:
        var mb: InputEventMouseButton = event
        if mb.button_index == MOUSE_BUTTON_WHEEL_UP:
            _target_zoom = clampf(_target_zoom * wheel_step, min_zoom, max_zoom)
        elif mb.button_index == MOUSE_BUTTON_WHEEL_DOWN:
            _target_zoom = clampf(_target_zoom / wheel_step, min_zoom, max_zoom)


func _process(delta: float) -> void:
    var weight: float = 1.0 - exp(-zoom_rate * delta)
    # Interpolate the logarithm, so zoom 1 -> 2 takes as long as 2 -> 4.
    var z: float = exp(lerpf(log(zoom.x), log(_target_zoom), weight))
    zoom = Vector2(z, z)
```

## Decouple the camera from the body

A `Camera2D` or `Camera3D` that is a child of the player inherits every rotation and every physics correction. A car that spins spins the screen; a body that jitters on a slope jitters the view.

Keep the camera outside the player's subtree and drive it with a `RemoteTransform2D` (or `RemoteTransform3D`) on the player:

1. Put the camera next to the player in the scene, not under it.
2. Add a `RemoteTransform2D` as a child of the player.
3. Set its `remote_path` to the camera.
4. Turn off `update_rotation` and `update_scale`. Keep `update_position` on.

The camera then follows the player's position only. Shake and zoom stay on the camera; the player's script never touches them.

For full control, do it in script: a separate camera node that reads the target position each frame and applies its own smoothing, as in the first example above. With physics interpolation on, read the target's `get_global_transform_interpolated()` in `_process()` (see `references/interpolation-camera.md` in **godot-physics-system**).

Keep shake on `offset` (2D) or `h_offset` and `v_offset` (3D), never on `position`. Then the follow code and the shake code do not overwrite each other.

## Frame several targets

For local co-op or a versus game, the camera fits a rectangle that holds every target, plus a margin. It moves to the center of that rectangle and picks the zoom that makes the rectangle fit the screen.

```gdscript
extends Camera2D

@export var targets: Array[Node2D] = []
@export var margin: Vector2 = Vector2(160.0, 120.0)
@export var min_zoom: float = 0.4
@export var max_zoom: float = 1.5
@export var follow_rate: float = 6.0


func _process(delta: float) -> void:
    var live: Array[Node2D] = targets.filter(func(t: Node2D) -> bool: return is_instance_valid(t))
    if live.is_empty():
        return
    var box: Rect2 = Rect2(live[0].global_position, Vector2.ZERO)
    for t: Node2D in live:
        box = box.expand(t.global_position)
    box = box.grow_individual(margin.x, margin.y, margin.x, margin.y)

    var screen: Vector2 = get_viewport_rect().size
    # Zoom > 1 magnifies in Godot 4. Fit the box on both axes; take the smaller fit.
    var fit: float = minf(screen.x / box.size.x, screen.y / box.size.y)
    var target_zoom: float = clampf(fit, min_zoom, max_zoom)

    var weight: float = 1.0 - exp(-follow_rate * delta)
    global_position = global_position.lerp(box.get_center(), weight)
    var z: float = exp(lerpf(log(zoom.x), log(target_zoom), weight))
    zoom = Vector2(z, z)
```

- `min_zoom` stops the camera from zooming out without limit when players run apart. Below it, push players back, split the screen (see `references/split-screen.md`), or let the leader drag the others.
- In 3D, do the same with an `AABB` of the targets: move the camera along its back axis until the box fits the field of view, `distance = (box_radius / tan(deg_to_rad(fov) / 2.0))`.

## A camera director with priorities

A game has a follow camera, room cameras, a boss arena camera and cutscene cameras. Let each camera register a priority with one director. The director makes the highest-priority camera current. A trigger that enters a boss room raises that camera's priority; leaving lowers it. No script needs to know which camera was active before.

```gdscript
extends Node

## Camera2D -> priority. Higher wins; ties keep the current camera.
var _cameras: Dictionary[Camera2D, int] = {}
var _current: Camera2D = null


func set_priority(cam: Camera2D, priority: int) -> void:
    _cameras[cam] = priority
    _refresh()


func remove(cam: Camera2D) -> void:
    _cameras.erase(cam)
    _refresh()


func _refresh() -> void:
    var best: Camera2D = _current if _cameras.has(_current) else null
    var best_priority: int = _cameras[best] if best != null else -2147483648
    for cam: Camera2D in _cameras:
        if is_instance_valid(cam) and _cameras[cam] > best_priority:
            best = cam
            best_priority = _cameras[cam]
    if best != null and best != _current:
        _current = best
        best.make_current()
```

Make this an autoload. For a blend instead of a cut, run the tween from `references/transitions.md` before `make_current()`. If the project uses the Phantom Camera addon, it already does this; see **godot-phantom-camera**.

Only one `Camera2D` per viewport is current. Do not rely on scene order or on `enabled` to decide which one; call `make_current()` explicitly.
