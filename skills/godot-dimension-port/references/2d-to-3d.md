# 2D to 3D, and 2D to 2.5D

Read this when a 2D project moves to 3D, or keeps 2D art in a 3D world
(2.5D). Own text and own code. Code blocks parse on Godot 4.7.2; the
`Dir8` and `TilesToGrid` scripts also passed runtime tests on 4.7.2.

## 1. Player: CharacterBody2D platformer → CharacterBody3D

In 2D, jump speed is negative (up is `-Y`). In 3D it is positive. Move
relative to the camera's yaw, not to the world axes. Scene:

```text
Player (CharacterBody3D, this script)
├── CollisionShape3D (CapsuleShape3D, height 1.8, radius 0.4)
├── Model (Node3D; the mesh faces -Z)
└── CameraYaw (Node3D)
    └── CameraPitch (Node3D)
        └── SpringArm3D (spring_length 4)
            └── Camera3D
```

```gdscript
extends CharacterBody3D

@export var move_speed: float = 5.0          # m/s (2D: 300 px/s at 60 px/m)
@export var jump_speed: float = 4.5          # m/s, positive is up
@export var turn_speed: float = 12.0         # rad/s for the model
@export var mouse_sensitivity: float = 0.003 # rad per pixel

@onready var _yaw: Node3D = $CameraYaw
@onready var _pitch: Node3D = $CameraYaw/CameraPitch
@onready var _arm: SpringArm3D = $CameraYaw/CameraPitch/SpringArm3D
@onready var _model: Node3D = $Model


func _ready() -> void:
	Input.mouse_mode = Input.MOUSE_MODE_CAPTURED
	# The arm must not hit the player's own collider.
	_arm.add_excluded_object(get_rid())


func _unhandled_input(event: InputEvent) -> void:
	var motion := event as InputEventMouseMotion
	if motion != null and Input.mouse_mode == Input.MOUSE_MODE_CAPTURED:
		_yaw.rotate_y(-motion.relative.x * mouse_sensitivity)
		_pitch.rotation.x = clampf(_pitch.rotation.x - motion.relative.y * mouse_sensitivity,
			deg_to_rad(-70.0), deg_to_rad(30.0))


func _physics_process(delta: float) -> void:
	if not is_on_floor():
		velocity += get_gravity() * delta
	elif Input.is_action_just_pressed("jump"):
		velocity.y = jump_speed

	var input := Input.get_vector("move_left", "move_right", "move_forward", "move_back")
	# input.y is -1 for "move_forward"; the yaw basis turns it to the camera's forward.
	var direction := _yaw.global_basis * Vector3(input.x, 0.0, input.y)
	direction.y = 0.0
	direction = direction.normalized()
	velocity.x = direction.x * move_speed
	velocity.z = direction.z * move_speed
	move_and_slide()

	if direction != Vector3.ZERO:
		var target_yaw := atan2(-direction.x, -direction.z)
		_model.rotation.y = rotate_toward(_model.rotation.y, target_yaw, turn_speed * delta)
```

Notes:

- `get_gravity()` returns the project gravity plus any `Area3D` override,
  so gravity zones from the 2D game still work.
- Turn on `physics/common/physics_interpolation` when the camera looks
  jittery: the body moves at the physics rate and the camera draws at the
  frame rate. After a teleport, call `reset_physics_interpolation()`.
- Use `Basis` and `Quaternion` (`slerp`) for any 3D rotation blend. Lerping
  Euler angles on more than one axis turns along the wrong path.
- A first-person game drops `SpringArm3D` and puts `Camera3D` on
  `CameraPitch` at eye height.

## 2. Mouse to world (point and click)

```gdscript
extends Node3D

## Returns the world point under the mouse, or null when the ray hits nothing.
func mouse_world_point(camera: Camera3D, mask: int = 0xFFFFFFFF) -> Variant:
	var mouse := get_viewport().get_mouse_position()
	var from := camera.project_ray_origin(mouse)
	var to := from + camera.project_ray_normal(mouse) * 1000.0
	var query := PhysicsRayQueryParameters3D.create(from, to, mask)
	var hit := get_world_3d().direct_space_state.intersect_ray(query)
	return hit.get("position", null)
```

Call it from `_physics_process()`: the direct space state is safe to query
there.

## 3. 2D UI over 3D things

Keep the HUD in a `CanvasLayer`. To put a `Control` over a 3D object (name
tag, health bar), project its position each frame and hide it behind the
camera:

```gdscript
extends Control

@export var target: Node3D
@export var height_offset: float = 2.0


func _process(_delta: float) -> void:
	var camera := get_viewport().get_camera_3d()
	if camera == null or not is_instance_valid(target):
		visible = false
		return
	var world_pos := target.global_position + Vector3.UP * height_offset
	visible = not camera.is_position_behind(world_pos)
	if visible:
		position = camera.unproject_position(world_pos) - size * 0.5
```

## 4. Sprites in a 3D world (2.5D)

- `Sprite3D` / `AnimatedSprite3D`: set `pixel_size = 1.0 / pixels_per_meter`
  so a sprite keeps its size in meters.
- Billboard: `billboard = BaseMaterial3D.BILLBOARD_FIXED_Y` keeps characters
  upright on the ground; `BILLBOARD_ENABLED` turns fully to the camera
  (particles, pickups).
- Pixel art: `texture_filter = BaseMaterial3D.TEXTURE_FILTER_NEAREST`.
  Sprites that overlap and sort badly: `alpha_cut = SpriteBase3D.ALPHA_CUT_DISCARD`.
- `shaded = true` makes a sprite take 3D light.

```gdscript
extends Sprite3D

const PIXELS_PER_METER := 64.0


func _ready() -> void:
	pixel_size = 1.0 / PIXELS_PER_METER
	billboard = BaseMaterial3D.BILLBOARD_FIXED_Y
	texture_filter = BaseMaterial3D.TEXTURE_FILTER_NEAREST
	alpha_cut = SpriteBase3D.ALPHA_CUT_DISCARD
	shaded = true
```

A character with 8 drawn views picks the view from the camera position.
With a sheet of 8 frames in this order, set `frame = Dir8.view_index(...)`:

```gdscript
class_name Dir8
extends RefCounted

## Which of 8 sprite views to show for a character seen from a camera.
## 0 front, 1 front-left, 2 left, 3 back-left, 4 back, 5 back-right,
## 6 right, 7 front-right ("left" is the character's own left side).
## The character model faces -Z.
static func view_index(character_basis: Basis, character_pos: Vector3, camera_pos: Vector3) -> int:
	var facing := -character_basis.z
	facing.y = 0.0
	var to_camera := camera_pos - character_pos
	to_camera.y = 0.0
	if facing.is_zero_approx() or to_camera.is_zero_approx():
		return 0
	var angle := facing.signed_angle_to(to_camera, Vector3.UP)
	return posmod(roundi(angle / (TAU / 8.0)), 8)
```

Tested on 4.7.2: a character that faces `-Z`, seen from `-Z`, `+Z`, `+X`,
`-X` and front-left, gives 0, 4, 6, 2 and 1.

For thousands of the same sprite, use one `MultiMeshInstance3D` with a
billboard material, not one node each.

## 5. TileMapLayer → GridMap

Make a `MeshLibrary` with one item per tile kind (a human or a script builds
the meshes). Then copy the cells. The 2D cell `(x, y)` goes to the 3D cell
`(x, floor, y)`; set `GridMap.cell_size` to the tile size in meters.

```gdscript
class_name TilesToGrid
extends RefCounted

## Copies the cells of a TileMapLayer into a GridMap at floor `y`.
## `item_for_tile` maps "source_id:atlas_x:atlas_y" to a MeshLibrary item id.
## Returns the number of cells that had no item in the map (they are skipped).
static func copy_layer(layer: TileMapLayer, grid: GridMap, item_for_tile: Dictionary, y: int = 0) -> int:
	var missing := 0
	for cell in layer.get_used_cells():
		var atlas := layer.get_cell_atlas_coords(cell)
		var key := "%d:%d:%d" % [layer.get_cell_source_id(cell), atlas.x, atlas.y]
		var item: int = item_for_tile.get(key, GridMap.INVALID_CELL_ITEM)
		if item == GridMap.INVALID_CELL_ITEM:
			missing += 1
			continue
		grid.set_cell_item(Vector3i(cell.x, y, cell.y), item)
	return missing
```

Tested on 4.7.2: a mapped tile lands in the right cell with its item; an
unmapped tile is counted and skipped. Collision and navigation come from the
`MeshLibrary` items, not from the 2D `TileSet`: set them on the items.

## 6. Light and environment

A 3D scene with no light and no environment is close to black. The least a
test level needs:

```gdscript
extends Node3D


func _ready() -> void:
	var sun := DirectionalLight3D.new()
	sun.rotation = Vector3(deg_to_rad(-50.0), deg_to_rad(30.0), 0.0)
	sun.shadow_enabled = true
	add_child(sun)

	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.45, 0.6, 0.8)
	env.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	env.ambient_light_color = Color(0.4, 0.4, 0.45)
	var world_env := WorldEnvironment.new()
	world_env.environment = env
	add_child(world_env)
```

`PointLight2D` has a `height`; an `OmniLight3D` at that height above the
ground is the closest match. Its `omni_range` is in meters. Light quality is
a human's call: list the levels to check in `DONE:`.

## 7. Audio and navigation

- `AudioStreamPlayer2D.max_distance` is in pixels (default 2000).
  `AudioStreamPlayer3D` uses meters: `unit_size` (default 10) and
  `max_distance` (0 = no limit), with an `attenuation_model`. Set them again;
  do not divide the 2D numbers.
- Make a `NavigationRegion3D` with a `NavigationMesh` and bake it from the new
  geometry (`bake_navigation_mesh()`). Agents become `NavigationAgent3D`.
  Read `godot-ai-navigation`.

## 8. Cost

3D costs more per object than 2D. Measure on the target: draw calls with
`Performance.get_monitor(Performance.RENDER_TOTAL_DRAW_CALLS_IN_FRAME)` and
frame time with `Performance.TIME_PROCESS`, in a non-headless run (headless
draws nothing). Then read `godot-optimization`. Do not plan from fixed
"draw-call budgets".
