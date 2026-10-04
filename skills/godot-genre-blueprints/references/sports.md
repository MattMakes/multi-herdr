# Sports

Team or solo sports with a physical ball (or puck) and a referee: football,
basketball, hockey, arcade car-ball. FIFA, NBA 2K, Rocket League and Mario
Strikers are the reference points.

## Core loop

Kick-off → contest possession → build an attack → shoot → score or defend →
restart. The match clock and the score frame every action.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Ball physics | a free `RigidBody3D`, drag, spin curve, CCD | `godot-physics-system` |
| Players | movement with momentum, contact shapes per body part | `godot-player-controller`, `godot-physics-system` |
| Team AI | formation slots, press and cover roles | `godot-ai-navigation`, `godot-state-machine` |
| Context input | one button means pass, tackle or switch by situation | `godot-input-handling` |
| Animation | blend spaces, root motion for turns and shots | `godot-animation-system` |
| Referee | match states, goals, fouls, restarts | `godot-state-machine` |
| Broadcast camera | follow the ball, zoom on action, replays | `godot-camera-system` |
| Online | server-owned ball and score | `godot-multiplayer-sync` |

## Scene tree (4.7)

```text
Match (Node3D)
├── Pitch (StaticBody3D; PhysicsMaterial with tuned bounce and friction)
│   └── Goals (Area3D per goal line)
├── Ball (RigidBody3D, continuous_cd on, sphere shape)
├── TeamA (Node3D; FormationAnchor with Marker3D slots, players)
├── TeamB (Node3D)
├── Referee (Node; KICKOFF, PLAYING, GOAL, END)
├── BroadcastCamera (Camera3D)
└── HUD (CanvasLayer; score, clock)
```

## Genre code

The ball stays a free body. Add drag and the spin curve (Magnus force) in
`_integrate_forces()`, where the physics state is safe to change.

```gdscript
extends RigidBody3D

@export var drag := 0.02
@export var magnus := 0.0006

func _ready() -> void:
	continuous_cd = true

func _integrate_forces(state: PhysicsDirectBodyState3D) -> void:
	var v := state.linear_velocity
	var speed := v.length()
	if speed < 0.01:
		return
	state.apply_central_force(-v * speed * drag)
	state.apply_central_force(state.angular_velocity.cross(v) * magnus)

func kick(direction: Vector3, power: float, spin: Vector3) -> void:
	apply_central_impulse(direction.normalized() * power)
	angular_velocity = spin
```

A dribble is a series of small kicks just ahead of the player's feet. Never
parent the ball to the player.

## Pitfalls

- The ball parented to a player. Momentum, deflections and tackles break.
  Keep one free body and kick it.
- Every AI player chases the ball. Move a formation anchor with the ball
  and send each player to a slot; only one or two press.
- A perfect goalkeeper. Add reaction delay (0.2 to 0.5 s) and an error that
  grows with shot speed and angle.
- A fast ball that passes through the goal or the net. Turn on
  `continuous_cd`, and raise `physics/common/physics_ticks_per_second` (for
  example to 120) if needed.
- Jitter on high refresh screens. Turn on physics interpolation in Project
  Settings.
- Impulses in `_process()`. Apply them in physics callbacks.
- Scaled collision shapes. Change the shape's radius, never the node scale.
- Diagonal stick input faster than straight input. Read the stick with
  `Input.get_vector()`, which limits the length to 1.
- One collision shape per player. Separate head, torso and legs for headers,
  chest traps and tackles.
- A goal counted from the same frame the ball entered. Confirm the ball is
  fully across the line, and let the referee own the goal event.
- Skating feet. Use root motion from the `AnimationTree` for turns and shots.
- Client-decided goals online. The server owns ball and score.
