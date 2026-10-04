# 3D stairs, hover suspension and VehicleBody3D

Adds stair stepping for `CharacterBody3D`, a ray-spring hover body for arcade vehicles, and the `VehicleBody3D` and `VehicleWheel3D` settings that matter. Read it when a 3D character catches on steps, or when you build a car, hovercraft or boat.

All code targets Godot 4.7. Jolt is the default 3D engine for new projects since 4.6; the code works on both engines.

## Stairs for CharacterBody3D

`move_and_slide()` treats a step as a wall. To climb it, test three moves with `test_move()` before the real move: up by the step height, forward from there, and back down onto the step. `test_move()` runs the body's real shape and never moves it.

```gdscript
extends CharacterBody3D

@export var speed: float = 5.0
@export var max_step: float = 0.35


func _physics_process(delta: float) -> void:
    if not is_on_floor():
        velocity += get_gravity() * delta
    var input: Vector2 = Input.get_vector("move_left", "move_right", "move_forward", "move_back")
    var dir: Vector3 = (global_basis * Vector3(input.x, 0.0, input.y)).normalized()
    velocity.x = dir.x * speed
    velocity.z = dir.z * speed

    if is_on_floor() and dir != Vector3.ZERO:
        _try_step(Vector3(velocity.x, 0.0, velocity.z) * delta)
    move_and_slide()


func _try_step(horizontal: Vector3) -> void:
    if not test_move(global_transform, horizontal):
        return  # nothing ahead: no step needed
    var up: Vector3 = Vector3.UP * max_step
    if test_move(global_transform, up):
        return  # ceiling too low
    var raised: Transform3D = global_transform.translated(up)
    if test_move(raised, horizontal):
        return  # a wall, not a step
    # Find the step top: move down from the raised, forward position.
    var ahead: Transform3D = raised.translated(horizontal)
    var hit: KinematicCollision3D = KinematicCollision3D.new()
    if test_move(ahead, -up, hit):
        var drop: float = hit.get_travel().length()
        global_position += up + Vector3.UP * -drop
```

- Only step when the hit is close to vertical. Add `hit.get_normal().dot(Vector3.UP) > 0.7` to the last test if the character also climbs slopes it should slide off.
- A capsule rounds its bottom edge, so it climbs small steps by itself. A cylinder or box needs this code for every step.
- Set `floor_snap_length` at least to `max_step`, so the body stays on the stairs when it walks down them.
- A single `RayCast3D` under the feet misses steps and gaps. For ground checks on stairs, use a `ShapeCast3D` with a sphere or cylinder shape (see `queries-and-casts.md`).

## Hover suspension with rays

An arcade hovercraft or a simple car is easier to tune as a `RigidBody3D` held up by springs than as a `VehicleBody3D`. Put a `RayCast3D` at each corner, pointing down. Each ray that hits pushes up with a spring force at its corner, damped by the speed at that point.

```gdscript
extends RigidBody3D

@export var hover_height: float = 1.0
@export var spring: float = 60.0  # force per metre of compression, per corner
@export var damping: float = 8.0
@export var thrust: float = 30.0
@export var turn_torque: float = 12.0

@onready var _rays: Array[RayCast3D] = [$RayFL, $RayFR, $RayBL, $RayBR]


func _physics_process(_delta: float) -> void:
    for ray: RayCast3D in _rays:
        if not ray.is_colliding():
            continue
        var contact: Vector3 = ray.get_collision_point()
        var origin: Vector3 = ray.global_position
        var compression: float = hover_height - origin.distance_to(contact)
        if compression <= 0.0:
            continue
        # Velocity of this corner = body velocity + spin around the center of mass.
        var offset: Vector3 = origin - global_position
        var point_vel: Vector3 = linear_velocity + angular_velocity.cross(offset)
        var up: Vector3 = global_basis.y
        var force: float = compression * spring - point_vel.dot(up) * damping
        apply_force(up * force * mass, offset)

    var drive: float = Input.get_axis("move_back", "move_forward")
    apply_central_force(-global_basis.z * drive * thrust * mass)
    var turn: float = Input.get_axis("move_right", "move_left")
    apply_torque(global_basis.y * turn * turn_torque * mass)
```

- Set each ray's `target_position` a little longer than `hover_height`, for example `(0, -1.5, 0)`, so the spring sees the ground before it touches.
- `apply_force(force, position)` takes the position as an offset from the body's origin in global orientation, not a world position.
- Raise `angular_damp` and `linear_damp` on the body to stop endless rocking and sliding.
- The same pattern gives buoyancy: replace the ground ray with the depth under a water surface height.

## VehicleBody3D

`VehicleBody3D` is a ray-cast wheel simulation. Use it when the car must feel like a car: weight transfer, slip, suspension travel. Add a `VehicleWheel3D` child per wheel.

| setting | where | effect |
|---|---|---|
| `engine_force` | body | Drive force on traction wheels. Negative drives backward. |
| `brake` | body | Brake force on all wheels. |
| `steering` | body | Steering angle in radians on steering wheels. |
| `use_as_traction` | wheel | This wheel receives `engine_force`. |
| `use_as_steering` | wheel | This wheel turns with `steering`. |
| `wheel_friction_slip` | wheel | Grip. Low values (about 1) drift; high values (above 5) grip hard. |
| `suspension_stiffness` | wheel | Spring rate. About 50 for a race car, 20 to 30 for an off-road car. |
| `suspension_travel` | wheel | How far the wheel can move, in metres. |
| `damping_compression`, `damping_relaxation` | wheel | Damping when the spring compresses and extends. Relaxation is usually higher. |
| `wheel_roll_influence` | wheel | Lower values (0.1) resist rolling over in turns. |

```gdscript
extends VehicleBody3D

@export var max_engine: float = 150.0
@export var max_brake: float = 5.0
@export var max_steer: float = 0.5  # radians
@export var steer_speed: float = 2.5


func _physics_process(delta: float) -> void:
    var throttle: float = Input.get_axis("move_back", "move_forward")
    engine_force = throttle * max_engine
    brake = max_brake if Input.is_action_pressed("brake") else 0.0
    var target: float = Input.get_axis("move_right", "move_left") * max_steer
    steering = move_toward(steering, target, steer_speed * delta)
```

- Lower the center of mass to stop rollovers: set `center_of_mass_mode` to custom and move `center_of_mass` down.
- `VehicleWheel3D.get_skidinfo()` returns 0 when the wheel slides fully and 1 when it grips. Use it for skid sounds and tire marks.
- Mass matters: forces are in newtons. A heavy car with a low `max_engine` barely moves.
