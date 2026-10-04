# Racing

Vehicles on a track against rivals or the clock: arcade, kart or
simulation handling. Mario Kart, Forza Horizon and Trackmania are the
reference points.

## Core loop

Race → overtake or beat the time → earn currency or unlocks → upgrade and
tune the car → learn the track's best line → race again.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Vehicle physics | `VehicleBody3D` for arcade, or a `RigidBody3D` with ray springs for karts | `godot-physics-system` |
| Laps and checkpoints | ordered checkpoints, lap count, position ranking | this reference |
| AI drivers | follow a racing line with look-ahead, rubber-banding | `godot-ai-navigation`, `godot-math-essentials` (curves) |
| Chase camera | smooth follow, FOV grows with speed | `godot-camera-system` |
| Engine and tyre sound | pitch from RPM, skid sounds | `godot-audio-system` |
| Speed feel | FOV, shake, wind lines, motion blur, skid marks | `godot-particles-vfx`, `godot-shader-basics` |
| Ghost and replays | sampled transforms at a fixed rate | `godot-save-load` |
| Garage | parts and stats as Resources | `godot-resource-pattern`, `godot-economy-system` |
| HUD | speedometer, lap timer, position, minimap | `godot-hud-system` |

## Scene tree (4.7)

```text
Race (Node3D)
├── Track (Node3D; road mesh + StaticBody3D, PhysicsMaterial per surface)
│   ├── RacingLine (Path3D; AI target line, also used for ranking)
│   └── Checkpoints (Node3D; Area3D children in order)
├── Cars (Node3D)
│   └── Car (VehicleBody3D, center_of_mass_mode Custom, low centre)
│       ├── VehicleWheel3D x4
│       ├── EngineAudio (AudioStreamPlayer3D)
│       └── CameraTarget (Marker3D)
├── ChaseCamera (Camera3D; follows CameraTarget with a lag)
└── HUD (CanvasLayer; SubViewport for the minimap or mirror)
```

## Genre code

Count a lap only when the car passed every checkpoint in order. A car
that skips one, or drives backwards, gets nothing.

```gdscript
extends Node

signal lap_completed(car: Node3D, lap: int, time: float)

@export var checkpoint_count := 8
var next_index: Dictionary[Node3D, int] = {}
var laps: Dictionary[Node3D, int] = {}
var lap_start: Dictionary[Node3D, float] = {}

func on_checkpoint(car: Node3D, index: int, race_time: float) -> void:
	var expected: int = next_index.get(car, 0)
	if index != expected:
		return   # skipped or backwards
	if index == 0 and laps.has(car):
		laps[car] += 1
		lap_completed.emit(car, laps[car], race_time - lap_start[car])
	elif index == 0:
		laps[car] = 0
	if index == 0:
		lap_start[car] = race_time
	next_index[car] = (index + 1) % checkpoint_count
```

Connect each checkpoint's `body_entered` with its index bound. For race
position, sort by laps, then next checkpoint, then the car's offset along
the racing line (`Curve3D.get_closest_offset()`).

## Pitfalls

- A camera fixed rigidly to the car. Follow a target with a lag, and look
  at the car.
- Floaty arcade cars. Raise `gravity_scale` to 2 to 3, raise wheel
  friction, and lower the centre of mass with a custom `center_of_mass`.
- A spring with no damping bounces forever. Tune `suspension_stiffness`
  together with `damping_compression` and `damping_relaxation`.
- Engine force multiplied by `delta`. Forces are integrated by the
  physics engine; set them as they are.
- Steering at full lock at top speed. Reduce the steering angle as speed
  grows.
- AI with fixed speeds. Rubber-band by distance to the player, within
  limits players do not notice.
- AI path queries across the whole track. Steer toward a look-ahead point
  on the racing line.
- No ordered checkpoints. Players find shortcuts across the infield.
- Slipstream from an area overlap alone. Also check that the car is behind
  with a dot product of the leader's forward vector and the gap.
- Constant engine pitch. Map RPM (`VehicleWheel3D.get_rpm()`) or speed to
  `pitch_scale`.
- Ghosts stored every physics frame. Sample position and rotation at a
  fixed rate, about 10 to 20 Hz, and interpolate on playback.
- Gear shift on `is_action_pressed()`. Use `is_action_just_pressed()`.
- Drift with no reward. Lower rear grip while drifting and give a boost on
  exit; tween the FOV for the boost.
