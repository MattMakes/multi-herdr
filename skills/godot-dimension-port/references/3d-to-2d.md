# 3D to 2D: side view, top-down with height, isometric

Read this when a 3D project becomes 2D. Own text and own code. Code blocks
parse on Godot 4.7.2; `HeightBody2D` also passed a runtime test, and the
isometric round trip below was measured on 4.7.2.

## 1. Pick what happens to the third axis

| 3D axis you drop or keep | 2D result |
|---|---|
| drop depth (Z), keep height (Y) | side view: platformer, fighter, side shooter |
| drop height (Y), keep the ground (X, Z) | top-down: the play uses the floor only |
| keep height as a number, the ground in physics | top-down with jumps, fliers, ground and air hits |
| keep a 3D look with 2D art | isometric `TileMapLayer` + Y-sort |

A 3D mechanic that needs the dropped axis (looking up, stacked floors,
cover by height) needs a 2D replacement in the plan: layers, a height
number, or a cut. Send one `QUESTION:` when the task does not say.

## 2. Side view: CharacterBody3D → CharacterBody2D

Signs flip: in 2D, up is `-Y`, so a jump speed is negative. Convert every
speed, gravity and size with one `pixels_per_meter`.

```gdscript
extends CharacterBody2D

const PIXELS_PER_METER := 64.0

@export var move_speed: float = 5.0 * PIXELS_PER_METER   # was 5 m/s
@export var jump_speed: float = -4.5 * PIXELS_PER_METER  # was +4.5 m/s; up is -Y


func _physics_process(delta: float) -> void:
	if not is_on_floor():
		velocity += get_gravity() * delta  # project 2D gravity (980 px/s² by default)
	elif Input.is_action_just_pressed("jump"):
		velocity.y = jump_speed
	velocity.x = Input.get_axis("move_left", "move_right") * move_speed
	move_and_slide()
```

Check the jump height after you convert: the 3D gravity was 9.8 m/s², and
the 2D default 980 px/s² is 15.3 m/s² at 64 px/m. Set
`physics/2d/default_gravity` to `9.8 * pixels_per_meter`, or tune the jump.

## 3. Top-down with height

Physics stays on the floor plane. Height is a number that moves the art up
the screen and shrinks the shadow. Scene: the body with children `Visual`
(the sprite) and `Shadow`.

```gdscript
class_name HeightBody2D
extends CharacterBody2D

## Top-down body that keeps a simulated height above the floor.
## Physics runs on the floor plane (x, y). `height` only moves the visual
## child up the screen and scales the shadow. Children: Visual (Node2D), Shadow (Node2D).

@export var move_speed: float = 180.0
@export var jump_speed: float = 320.0
@export var height_gravity: float = 900.0
## Vertical size used by hit checks, in the same units as `height`.
@export var body_height: float = 48.0

var height: float = 0.0
var height_velocity: float = 0.0

@onready var _visual: Node2D = $Visual
@onready var _shadow: Node2D = $Shadow


func _physics_process(delta: float) -> void:
	var input := Input.get_vector("move_left", "move_right", "move_up", "move_down")
	velocity = input * move_speed
	move_and_slide()
	if Input.is_action_just_pressed("jump") and is_grounded():
		height_velocity = jump_speed
	step_height(delta)
	_visual.position.y = -height
	var shrink := clampf(1.0 - height / 200.0, 0.5, 1.0)
	_shadow.scale = Vector2(shrink, shrink)


func step_height(delta: float) -> void:
	if height <= 0.0 and height_velocity <= 0.0:
		height = 0.0
		height_velocity = 0.0
		return
	height_velocity -= height_gravity * delta
	height = maxf(height + height_velocity * delta, 0.0)
	if height == 0.0:
		height_velocity = 0.0


func is_grounded() -> bool:
	return height <= 0.0 and height_velocity <= 0.0


## True when the vertical spans [a, a + a_size] and [b, b + b_size] overlap.
## Call it after an Area2D overlap to reject a hit at another height.
static func spans_overlap(a: float, a_size: float, b: float, b_size: float) -> bool:
	return a <= b + b_size and b <= a + a_size
```

Tested on 4.7.2: a jump at 320 px/s with 900 px/s² gravity peaks at about
54 px and lands in 42 steps of 1/60 s; `spans_overlap(0, 48, 40, 48)` is
true and `spans_overlap(0, 48, 60, 48)` is false.

Hits: 2D areas see only the floor plane, so a sword swing on the ground also
"hits" a jumping enemy. After an `Area2D` overlap, compare heights:

```gdscript
extends Area2D

## Height range of this hitbox, in the same units as HeightBody2D.height.
@export var hit_base: float = 0.0
@export var hit_height: float = 32.0


func _ready() -> void:
	body_entered.connect(_on_body_entered)


func _on_body_entered(body: Node2D) -> void:
	var target := body as HeightBody2D
	if target == null:
		return
	if HeightBody2D.spans_overlap(hit_base, hit_height, target.height, target.body_height):
		print("hit ", target.name)
```

Fliers over walls: keep one navigation layer for walkers and add a second
layer whose polygons also cover the walls. A flying agent sets
`set_navigation_layer_value(2, true)`.

## 4. Draw order (Y-sort)

Turn on `y_sort_enabled` on the parent that holds the characters and props
(and on each `TileMapLayer` that holds tall tiles). The child with the larger
`y` draws in front. Do not set `z_index` from `position.y` each frame;
`z_index` is for whole layers (floor, characters, overhead). Put the sorting
point at the feet: the sprite's `offset` moves the art up, so the node origin
stays on the floor.

Shadow on the floor: a dark sprite under the character, flattened with
`scale.y` and slanted with `Node2D.skew` away from the light. It is a look,
not a light; a human checks it.

## 5. Isometric

Let the engine do the isometric math. Set the `TileSet` to
`tile_shape = TileSet.TILE_SHAPE_ISOMETRIC` with the tile size of the art
(for example 64 × 32) and pick a `tile_layout`. Then convert cells and
positions with `TileMapLayer.map_to_local()` and `local_to_map()`. Measured
on 4.7.2 with a 64 × 32 isometric `TileSet`: cells `(0, 0)`, `(3, -2)`,
`(-5, 7)` and `(10, 10)` went to positions and back to the same cells.

Game logic can keep a square grid (`AStarGrid2D` on cell coordinates) and
convert only to draw. Do not hand-write the screen formulas.

## 6. Cameras, audio, light, navigation

- `Camera3D` follow rigs → `Camera2D` as a child of the player, or a
  separate camera with `position_smoothing_enabled`. `zoom` above 1 zooms
  in. Set the `limit_*` values to the level bounds.
- `AudioStreamPlayer3D` → `AudioStreamPlayer2D`. Distances become pixels
  (`max_distance` default 2000). Set them again.
- 3D lights → `PointLight2D` / `DirectionalLight2D`, with
  `LightOccluder2D` for shadows. For lit sprites, give each sprite a
  `CanvasTexture` with a `normal_texture`. Normal maps are art: a human makes
  them.
- `NavigationRegion3D` → `NavigationRegion2D` with a `NavigationPolygon`;
  bake again from the 2D geometry.

## 7. Baking 3D models into sprites

This needs a renderer. Under `--headless` the renderer is a dummy (measured
on 4.7.2: no frame is drawn, `frame_post_draw` never fires, the viewport
image is null). A fleet worker does not bake. Write the plan for the bake
(model list, views per model, frame size, `pixels_per_meter`, output
folder), then send it with one `QUESTION:` for an operator run. After the
sprites exist, set up `SpriteFrames` and `AnimatedSprite2D` headless.
