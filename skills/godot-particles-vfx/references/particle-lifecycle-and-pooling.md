Adds the runtime side of particle effects: the one-shot lifecycle with `restart()` and `finished`, pooled bursts, coordinate space for trails and teleports, culling bounds, distance LOD, and what `amount_ratio` does and does not save; read it when effects are spawned often from code, pop out of view, or cost too much at a distance.

# Particle Lifecycle, Pooling and Culling

> proof: not run (needs a GPU renderer) for what the particles draw, collide with and cost. Headless Godot simulates no GPU particle.

> ← Back to [SKILL.md](../SKILL.md). Property recipes are in [process-material-basics.md](process-material-basics.md); general pitfalls are in [performance-and-pitfalls.md](performance-and-pitfalls.md).

All code targets Godot 4.7. The examples use `GPUParticles3D`; the 2D nodes
have the same lifecycle API.

---

## 1. The One-Shot Lifecycle

1. Keep `emitting = false` in the scene. An effect that emits on load fires
   once at the scene origin before your code moves it.
2. Move the node to the spawn point.
3. Call `restart()`. It clears old particles and sets `emitting` to `true`.
4. With `one_shot = true`, the node emits `finished` when the last particle
   dies, and `emitting` is `false` again.

Do not set `emitting = true` from a `finished` handler to replay. Call
`restart()`. Do not free a one-shot from a `Timer` that guesses its length;
listen to `finished`.

## 2. Pool Bursts Instead of Instancing Them

Instancing and freeing an effect scene for each hit costs allocation and
shader setup on the main thread. Keep a small pool of idle emitters and
reuse them.

```gdscript
extends Node3D

## Pool of one-shot GPUParticles3D bursts. Add as a child of the level.
@export var effect_scene: PackedScene
@export var pool_size: int = 16

var _idle: Array[GPUParticles3D] = []


func _ready() -> void:
	for i in pool_size:
		var fx := effect_scene.instantiate() as GPUParticles3D
		if fx == null:
			push_error("BurstPool: effect_scene root must be GPUParticles3D")
			return
		fx.one_shot = true
		fx.emitting = false
		add_child(fx)
		fx.finished.connect(_on_finished.bind(fx))
		_idle.append(fx)


## Plays one burst. Returns false when every emitter is busy.
func play_at(where: Vector3, normal: Vector3 = Vector3.UP) -> bool:
	if _idle.is_empty():
		return false
	var fx: GPUParticles3D = _idle.pop_back()
	fx.global_position = where
	if not normal.is_equal_approx(Vector3.UP) and not normal.is_equal_approx(Vector3.DOWN):
		fx.look_at(where + normal, Vector3.UP)
	fx.restart()
	return true


func _on_finished(fx: GPUParticles3D) -> void:
	_idle.append(fx)
```

When the pool is empty, dropping the new burst is usually better than
growing the pool in the middle of a fight. Size the pool from a play test.

## 3. Coordinate Space: `local_coords`

| Effect | `local_coords` | Why |
|---|---|---|
| Aura, shield glow, engine flame on a ship | `true` | Particles move with the node. |
| Smoke trail, sparks left behind, footstep dust | `false` | Particles stay where they were emitted, so they form a trail. |

A world-space emitter (`local_coords = false`) that teleports (respawn,
portal) draws a streak of particles across the jump. Call `restart()` right
after the teleport to clear them.

GPU particles live on the GPU. The CPU gets no signal for a single particle
collision, so a per-impact sound cannot come from a GPU collision. Use a
sub-emitter for the visual (see [subemitters.md](subemitters.md)) and a
looping or randomized sound bed for audio, or a raycast on the CPU when the
timing must be exact.

## 4. Culling Bounds

Godot culls an emitter by its bounds, not by its live particles:

- 3D: `visibility_aabb`. 2D: `visibility_rect`.
- When the bounds are too small, the effect vanishes while particles are
  still on screen. When they are too large, off-screen effects still draw.
- In the editor: select the node, then use the toolbar's **Generate
  Visibility AABB** (or **Rect**) while the effect plays.
- From code: `capture_aabb()` (3D) or `capture_rect()` (2D) returns the
  current bounds of live particles. Call it while the effect is at its
  largest and store the result.

## 5. Distance LOD in 3D

`GPUParticles3D` is a `GeometryInstance3D`, so it has visibility ranges.
Ambient effects (chimney smoke, torches, waterfalls) can stop drawing
past a distance, or hand over to a cheaper emitter.

```gdscript
extends Node3D

## Swaps a detailed torch fire for a cheap one by camera distance.
@onready var near_fx: GPUParticles3D = $FireDetailed
@onready var far_fx: GPUParticles3D = $FireCheap

@export var switch_distance: float = 25.0
@export var hide_distance: float = 80.0


func _ready() -> void:
	near_fx.visibility_range_end = switch_distance
	near_fx.visibility_range_end_margin = 2.0
	far_fx.visibility_range_begin = switch_distance
	far_fx.visibility_range_begin_margin = 2.0
	far_fx.visibility_range_end = hide_distance
	for fx: GPUParticles3D in [near_fx, far_fx]:
		fx.visibility_range_fade_mode = GeometryInstance3D.VISIBILITY_RANGE_FADE_SELF
```

Note that visibility ranges hide drawing. Set `fixed_fps` low (for example 30)
on ambient emitters as well, so the simulation also costs less.

## 6. `amount_ratio` versus `amount`

- `amount` sets the particle buffer size. Changing it rebuilds the buffer and
  restarts the effect.
- `amount_ratio` (0 to 1) emits fewer particles **without** a restart. Use it
  to vary an effect over time (a fire that dies down) or for a quick quality
  slider.
- `amount_ratio` does not shrink the buffer. GPU memory and per-particle
  processing are still sized for `amount`. For a lower memory budget on weak
  hardware, set a smaller `amount` when the level loads.

## 7. Huge Counts Are Not a Particle Job

Thousands of fish, birds or debris pieces with their own behavior need
per-instance control that particles do not give. Use a `MultiMeshInstance3D`
and write the instance transforms from code (see
**godot-optimization** [scaling-techniques.md](../../godot-optimization/references/scaling-techniques.md)).
Particles fit effects whose motion is fully described by the process
material or a particle shader.

## 8. Preprocess Has a Cost

`preprocess` simulates that many seconds on the first frame, so an ambient
effect appears already running. The simulation runs in one go: a value of 60
seconds on a large system stalls the frame it starts in. Keep it near one
`lifetime`.
