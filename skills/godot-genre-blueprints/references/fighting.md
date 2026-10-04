# Fighting

One-on-one (or small team) fights built on frame data: every move has
startup, active and recovery frames, and the difference between attacker and
defender recovery decides who acts next. Street Fighter, Tekken and Guilty
Gear are the reference points.

## Core loop

Neutral → open the opponent up → confirm the hit → combo → reset to an
advantage → neutral again. A round ends on a knockout or the timer.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Fixed-step simulation | integer frames at 60 per second, no delta math in game logic | `godot-physics-system` |
| Fighter states | idle, walk, attack, hitstun, blockstun, knockdown, each counted in frames | `godot-state-machine` |
| Hit resolution | hitboxes per active frame, hurtboxes per state, throw boxes | `godot-combat-system`, `godot-physics-system` |
| Input buffer and motions | 5 to 10 frame buffer, quarter-circle and dragon-punch detection | `godot-input-handling` |
| Animation in step with frames | manual advance, one tick per game frame | `godot-animation-system` |
| Online play | input delay or rollback, state snapshots | `godot-multiplayer-sync` |
| Move data | one Resource per move: frames, damage, cancels | `godot-resource-pattern` |
| Hit feel | hitstop, shake, sparks | `godot-camera-system`, `godot-particles-vfx` |

## Scene tree (4.7)

```text
Match (Node2D)
├── Stage (Node2D; floor, walls, background)
├── P1 (Node2D; fighter, plain script, no CharacterBody needed)
│   ├── Visuals (Node2D; flip this, not the root)
│   │   └── Sprite + AnimationPlayer (callback mode Manual)
│   ├── Hurtboxes (Node2D; CollisionShape2D per state)
│   └── Hitboxes (Node2D; shapes turned on by frame data)
├── P2 (same)
├── CameraRig (Camera2D; frames both fighters)
└── HUD (CanvasLayer; health, meter, timer, combo count)
```

## Genre code

Count frames with integers. Each `_physics_process` step is one game frame;
the move Resource says when the hitbox is live and when the fighter can act
again.

```gdscript
extends Node2D

@export var startup := 5
@export var active := 3
@export var recovery := 12
@export var hitstop_frames := 8

var state := &"idle"
var state_frame := 0
var freeze := 0

func _ready() -> void:
	Input.use_accumulated_input = false

func step() -> void:
	if freeze > 0:
		freeze -= 1
		return
	state_frame += 1
	if state == &"attack" and state_frame > startup + active + recovery:
		state = &"idle"
		state_frame = 0

func hitbox_live() -> bool:
	return state == &"attack" and state_frame > startup and state_frame <= startup + active

func on_hit_confirmed() -> void:
	freeze = hitstop_frames
```

Call `step()` for both fighters from one match node in a fixed order, then
resolve hits, then advance each `AnimationPlayer` by exactly one frame with
`advance(1.0 / 60.0)`. One owner of the order keeps the result deterministic,
which rollback requires.

## Pitfalls

- Timers, `await` or `delta` in move logic. A dropped frame changes the
  outcome. Count frames.
- Hits from `area_entered` signals arrive a physics step late and in no fixed
  order. Query the shapes yourself with
  `PhysicsDirectSpaceState2D.intersect_shape()` at the moment the frame
  data says the box is live.
- Flipping the root with `scale.x = -1` flips physics bodies badly. Flip the
  `Visuals` node and mirror hitbox offsets in code.
- No damage scaling: long combos kill from full health. Reduce each hit in a
  combo, about 10% per hit, with a floor.
- Every move safe on block. Big moves need recovery the opponent can punish.
- Input accumulation on. Turn off `Input.use_accumulated_input` so
  every input event keeps its own frame.
- Frame data as constants in scripts. Put it in move Resources so designers
  can tune it, and an editor tool can scrub frames.
- Rollback with state in nodes. Keep fighter state in plain data that one
  function can save and restore in one frame.
- Snapping a fighter after a throw with physics interpolation on. Call
  `reset_physics_interpolation()` after the teleport.
- Balance targets used by the genre: jabs start in 3 to 5 frames; a single
  combo takes at most 30 to 40% of health.
