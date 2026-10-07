Adds comfort-first locomotion: snap turn around the head, teleport that keeps the player inside the play area, a comfort vignette hook, seated mode and UI distance rules; read it before writing any XR movement code.

# Comfort and locomotion

> proof: not run (needs an XR headset). Comfort also needs a human play test.

> ← Back to [SKILL.md](../SKILL.md)

The SKILL.md thumbstick recipe moves the `XROrigin3D` smoothly. Smooth
movement makes many players sick. Ship these comfort options with it.

## Comfort rules

- **Always offer teleport** (or room-scale only) as an alternative to
  smooth movement, and **snap turn** as the default turn mode. Smooth turn
  is an option the player turns on.
- **Never move or rotate the camera the player did not move.** No camera
  shake, no head bob, no forced look-at. Move the world or the origin, and
  only on player input.
- **Hold the headset's frame rate.** A missed frame is felt as judder. Cut
  effects before you cut frame rate (see
  [session-and-performance.md](session-and-performance.md)).
- **Vignette during artificial motion.** Narrowing the view while the
  stick moves the player reduces sickness.
- **Seated mode.** A height offset lets a seated player play a standing
  game.
- **UI at 1 to 3 meters,** fixed in the world or attached to a hand.
  Never billboard UI to the face: it breaks the stereo depth cue.

## Snap turn around the head

Rotating the `XROrigin3D` around its own origin swings the player around
the center of the play area, not around their head. Rotate around the
camera's position instead.

```gdscript
# snap_turn.gd - on the XROrigin3D
extends XROrigin3D

@export var camera: XRCamera3D
@export var controller: XRController3D  # Usually the right hand.
@export var turn_degrees: float = 30.0
@export var deadzone: float = 0.6

var _armed: bool = true


func _physics_process(_delta: float) -> void:
	if camera == null or controller == null:
		return
	var x: float = controller.get_vector2(&"primary").x
	if absf(x) < deadzone * 0.5:
		_armed = true  # Stick back near center: allow the next turn.
	elif _armed and absf(x) > deadzone:
		_armed = false
		rotate_around_head(deg_to_rad(-signf(x) * turn_degrees))


func rotate_around_head(angle: float) -> void:
	var pivot := Vector3(camera.global_position.x, global_position.y, camera.global_position.z)
	var t: Transform3D = global_transform
	t.origin -= pivot
	t = t.rotated(Vector3.UP, angle)
	t.origin += pivot
	global_transform = t
```

The action name (`primary` above) comes from the project's OpenXR action
map. Check `res://openxr_action_map.tres` or **Project Settings → XR →
OpenXR** for the names the project uses.

## Teleport that lands the head on the target

Moving the origin to the target puts the center of the play area there,
and the player may stand a meter away from it. Shift the origin by the
offset between the head's floor position and the target.

The play area comes from `XRInterface.get_play_area()`: a
`PackedVector3Array` of floor points in origin space, empty when the
runtime does not report one. Use it to warn the player near the real-world
boundary; the runtime's own guardian still has the final say.

```gdscript
# teleporter.gd - on the XROrigin3D
extends XROrigin3D

signal teleported(target: Vector3)

@export var camera: XRCamera3D


func teleport_to(target: Vector3) -> bool:
	if camera == null or get_tree().paused:
		return false  # Never move the player while the session is not focused.
	var head_on_floor := Vector3(camera.global_position.x, global_position.y, camera.global_position.z)
	global_position += target - head_on_floor
	teleported.emit(target)
	return true


func is_head_inside_play_area() -> bool:
	var xr: XRInterface = XRServer.primary_interface
	if xr == null or camera == null:
		return true
	var points: PackedVector3Array = xr.get_play_area()
	if points.size() < 3:
		return true  # No boundary reported.
	var polygon := PackedVector2Array()
	for p: Vector3 in points:
		polygon.append(Vector2(p.x, p.z))
	var local_head: Vector3 = to_local(camera.global_position)
	return Geometry2D.is_point_in_polygon(Vector2(local_head.x, local_head.z), polygon)
```

Validate the target before you call `teleport_to`: a ray from the
controller, a hit on walkable ground (a collision layer or a navigation
mesh query), and enough head room. Show the arc and the landing marker
while the player aims; fade to black for about 0.1 s around the move.

## Comfort vignette hook

The vignette itself is art: a mesh in front of `XRCamera3D` with a shader
that darkens the edges by a `strength` parameter. The locomotion code only
drives that parameter from the movement speed.

```gdscript
# vignette_driver.gd - on a MeshInstance3D child of XRCamera3D
extends MeshInstance3D

@export var origin: XROrigin3D
@export var max_strength: float = 0.7
@export var speed_for_max: float = 3.0  # Meters per second.
@export var enabled_by_player: bool = true

var _last_origin_pos: Vector3
var _strength: float = 0.0


func _ready() -> void:
	if origin != null:
		_last_origin_pos = origin.global_position


func _process(delta: float) -> void:
	if origin == null or delta <= 0.0:
		return
	# Only artificial motion moves the origin; head motion does not.
	var speed: float = origin.global_position.distance_to(_last_origin_pos) / delta
	_last_origin_pos = origin.global_position
	var target: float = 0.0
	if enabled_by_player:
		target = clampf(speed / speed_for_max, 0.0, 1.0) * max_strength
	_strength = move_toward(_strength, target, delta * 4.0)
	set_instance_shader_parameter(&"strength", _strength)
```

The shader must declare `instance uniform float strength;` for
`set_instance_shader_parameter` to reach it.

## Seated mode

```gdscript
# seated_mode.gd - on the XROrigin3D
extends XROrigin3D

## Height in meters added for a seated player (standing eye height minus seated eye height).
@export var seated_offset: float = 0.45
@export var seated: bool = false:
	set(value):
		seated = value
		_apply()

var _standing_y: float = 0.0


func _ready() -> void:
	_standing_y = position.y
	_apply()


func _apply() -> void:
	if is_inside_tree():
		position.y = _standing_y + (seated_offset if seated else 0.0)
```

Physics bodies that follow the player (a body capsule) must read the
camera height relative to the origin, not assume a standing player.

## Settings to expose

| Setting | Default |
|---|---|
| Movement | Teleport |
| Turn | Snap, 30° |
| Smooth turn speed | Off until chosen |
| Vignette | On |
| Seated mode | Off |
| Dominant hand | Right |
