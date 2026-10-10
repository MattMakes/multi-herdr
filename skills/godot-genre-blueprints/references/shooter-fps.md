# First-person shooter

Shooters seen through the player's eyes: mouse look, fast movement, a
weapon view model and instant hit feedback. Doom, Counter-Strike and Halo
are the reference points. Shared gun theory (hitscan vs projectile, server
rewind) is also in `shooter.md`.

## Core loop

Move and look → aim → fire → recover from recoil → find the next target.
In arena games add: pick up weapons and health → keep moving.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| FPS controller | mouse look, acceleration, friction, air control, crouch | `godot-player-controller` |
| Hitscan and projectiles | ray queries, pooled projectiles | `godot-physics-system`, `godot-combat-system` |
| Weapon states | idle, fire, reload, switch; data in Resources | `godot-state-machine`, `godot-resource-pattern` |
| View model | bob, sway, recoil, separate render layer | `godot-3d-essentials`, `godot-camera-system` |
| Impacts | decals with a cap, sparks, sounds | `godot-particles-vfx`, `godot-audio-system` |
| Enemies | cover, flanks, line of sight | `godot-ai-navigation`, `godot-beehave` |
| Netcode | server authority, lag compensation | `godot-multiplayer-sync` |
| HUD | crosshair, ammo, hit markers | `godot-hud-system` |

## Scene tree (4.7)

```text
Player (CharacterBody3D, capsule)
├── Head (Node3D; pitch only; yaw is on the body)
│   ├── Camera3D (cull mask without the view-model layer)
│   ├── ViewModelViewport (SubViewport + its own Camera3D for the gun)
│   └── AimRay (Marker3D at the camera)
├── Weapons (Node; WeaponData Resources, current weapon state)
└── Audio (AudioStreamPlayer3D x3: mechanism, shot, tail)
```

The view model renders in its own `SubViewport` with its own camera, so the
gun never clips into walls and keeps its own field of view.

## Genre code

Recoil has two parts: a camera kick the player must pull down, and a
spread that grows with each shot and recovers over time. Keep yaw and pitch
as floats and build the rotation from them.

```gdscript
extends CharacterBody3D

@export var sensitivity := 0.0025
@export var kick_up := 0.02       # radians per shot
@export var spread_per_shot := 0.6
@export var spread_recover := 6.0 # per second
@onready var head: Node3D = $Head

var yaw := 0.0
var pitch := 0.0
var spread := 0.0   # degrees

func _ready() -> void:
	Input.mouse_mode = Input.MOUSE_MODE_CAPTURED

func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseMotion:
		yaw -= event.relative.x * sensitivity
		pitch = clampf(pitch - event.relative.y * sensitivity, -1.5, 1.5)
		_apply_look()

func on_shot(rng: RandomNumberGenerator) -> void:
	pitch = clampf(pitch + kick_up, -1.5, 1.5)
	yaw += rng.randf_range(-0.3, 0.3) * kick_up
	spread += spread_per_shot
	_apply_look()

func _physics_process(delta: float) -> void:
	spread = move_toward(spread, 0.0, spread_recover * delta)

func _apply_look() -> void:
	rotation.y = yaw
	head.rotation.x = pitch
```

The shot direction is the camera's forward vector, `-camera.global_basis.z`,
turned by a random angle up to `spread`.

## Pitfalls

- Mouse look in `_physics_process()`. Read `InputEventMouseMotion` in
  `_unhandled_input()`; physics ticks miss events between them.
- Look stored by rotating a transform again and again. Error builds up;
  keep yaw and pitch floats.
- Velocity multiplied by `delta` before `move_and_slide()`.
- Recoil only on the gun model. The aim does not move; kick the camera.
- The player's own body blocks the ray. Exclude the player's RID.
- Hit checks in `_process()`. Use `_physics_process()`.
- One `AudioStreamPlayer` for gunfire cuts off each shot. Layer several
  players, or raise `max_polyphony`.
- Decals that stay forever. Cap the count and recycle the oldest.
- Client damage trusted. The server confirms with rewind.
- Weapon stats in nodes. Use WeaponData Resources.
- Stairs that stop the player. Step up: after `move_and_slide()` on a
  wall, test the move from `step_height` higher and lift the player if it
  is clear.
- Cooldowns compared with `==` on floats. Compare with `<=` or `>=`.
