# Platformer

Side-view games about jumping well: tight control, readable hazards, short
retries. Celeste, Super Meat Boy and Mario are the reference points.

## Core loop

Run and jump → clear an obstacle → reach a checkpoint → reach the goal → the
next level adds a twist. Precision platformers respawn in under a second.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Character controller | coyote time, jump buffer, variable jump, fall gravity | `godot-player-controller` |
| Level geometry | TileMapLayer with physics layers and one-way tiles | `godot-2d-essentials` |
| Moving platforms and hazards | AnimatableBody2D, Area2D hazards, layers | `godot-physics-system` |
| Camera | smoothing, look-ahead, room limits | `godot-camera-system`, `godot-phantom-camera` |
| Animation | idle, run, jump, fall, land states from velocity | `godot-animation-system` |
| Juice | squash and stretch, dust, landing shake, sounds | `godot-tween-animation`, `godot-particles-vfx`, `godot-audio-system` |
| Checkpoints and progress | last checkpoint, collectibles, level unlocks | `godot-gameplay-loops` (revival, collection), `godot-save-load` |
| Assist options | slower game speed, extra jumps, remapping | `godot-input-handling` |

## Scene tree (4.7)

```text
Level (Node2D)
├── Ground (TileMapLayer; physics layer 1)
├── OneWay (TileMapLayer; one-way collision, physics layer 2)
├── Platforms (Node2D)
│   └── Mover (AnimatableBody2D, sync_to_physics on) + AnimationPlayer
├── Hazards (Area2D per hazard group)
├── Checkpoints (Area2D each, with an id)
├── Player (CharacterBody2D, capsule shape)
│   ├── Visuals (Node2D; scale this for squash, pivot at the feet)
│   │   └── AnimatedSprite2D
│   └── DustParticles (GPUParticles2D)
├── Camera2D (position smoothing on)
└── HUD (CanvasLayer)
```

## Genre code

Dropping through a one-way platform: put one-way tiles on their own
collision layer, and turn that layer off in the player's mask for a moment.

```gdscript
extends CharacterBody2D

const ONE_WAY_LAYER := 2
@export var drop_time := 0.2

func _physics_process(_delta: float) -> void:
	if is_on_floor() and Input.is_action_pressed(&"move_down") \
			and Input.is_action_just_pressed(&"jump"):
		_drop_through()
	move_and_slide()

func _drop_through() -> void:
	set_collision_mask_value(ONE_WAY_LAYER, false)
	position.y += 1.0
	await get_tree().create_timer(drop_time, false, true).timeout
	set_collision_mask_value(ONE_WAY_LAYER, true)
```

The movement itself (coyote time, buffer, variable height, fall gravity)
is in `godot-player-controller`. Tune these knobs per game: coyote about
0.08 to 0.12 s, buffer about 0.1 to 0.15 s, fall gravity 1.5 to 2.5 times
rise gravity, release cut 0.4 to 0.6 of upward speed.

## Pitfalls

- `velocity *= delta` before `move_and_slide()`. The method applies the time
  step; multiply only accelerations, such as gravity, by `delta`.
- No coyote time or jump buffer. Jumps feel dropped. Both are cheap.
- A fixed jump height. Cut upward speed when the button is released.
- Floaty jumps. Raise gravity after the apex.
- Moving platforms built from CharacterBody2D or a plain Node2D. Use
  `AnimatableBody2D` with `sync_to_physics`, so riders move with it.
- A player who jumps off a rising platform and loses the boost. Check
  `platform_on_leave`; the default adds the platform velocity.
- Fast dashes that tunnel through thin walls. Keep wall tiles thick, or
  split the move into steps; for fast `RigidBody2D` objects set
  `continuous_cd`.
- Blind jumps. Look ahead in the move direction, or zoom out before a drop.
- One sprite node per wall tile. Use `TileMapLayer`.
- Concave or complex player shapes. Use a capsule or a rectangle.
- Teaching by text. Introduce a mechanic safely, test it with risk, then
  combine it with another one. Add a new mechanic every 2 or 3 levels.
- Squash applied to the body root. Scale the `Visuals` child, so collision
  does not change.
