# Particles and VFX: GPU and CPU emitters, cost, cheaper stand-ins

Target engine: Godot 4.7. How particle systems really run, which features cost what, what is
missing per renderer, and when a trail, decal or MultiMesh does the job for less.

## Use this shelf when

- You choose between GPUParticles, CPUParticles, a custom particle shader or a MultiMesh.
- An effect pops in or out at screen edges, or collision silently does nothing.
- A burst (hit spark, muzzle flash) must fire many times per second without hitches.
- The game ships on the Compatibility renderer, web or low-end mobile.
- A particle effect stutters the first time it plays.
- You need sub-emitters, trails, attractors, collision or flipbooks and want their limits.

## How the engine works here

- `GPUParticles2D`/`GPUParticles3D` simulate in a particle shader on the GPU. The node allocates
  `amount` slots. GPU cost follows `amount` even when few particles are visible or
  `amount_ratio` is low. Emission rate is `amount * amount_ratio / lifetime` per second.
- `CPUParticles2D`/`CPUParticles3D` simulate on the CPU and only render on the GPU. Fewer
  features, lower ceiling, but they run on every device and can win when the GPU is the
  bottleneck. No new features are planned for the 2D one.
- A `ParticleProcessMaterial` is a generated `shader_type particles` shader. Each enabled feature
  adds code to it. A `ShaderMaterial` with your own `start()` and `process()` replaces it.
- The simulation steps at `fixed_fps` (default 30 on GPU nodes), not at the frame rate.
  `interpolate` blends between steps for display. Collision and attractors also run per step.
- GPU particles are culled as one object by `visibility_aabb` (3D) or `visibility_rect` (2D),
  not per particle. In 3D, collision and attraction also happen only inside that box.
  `GeometryInstance3D.custom_aabb`, if set, overrides `visibility_aabb`.
- 3D particles collide only with `GPUParticlesCollision3D` nodes, never with physics bodies. 2D
  particles collide only with `LightOccluder2D` shapes through the viewport's 2D SDF.
- Results live on the GPU. Scripts cannot read particle positions cheaply
  (`GPUParticles2D.capture_rect()` syncs with the render thread).

## Advanced techniques

### GPU or CPU: decide by target, not habit

| Need | Pick |
|---|---|
| Thousands of particles, trails, sub-emitters, attractors, collision | GPUParticles |
| Compatibility renderer and old phones, small counts | CPUParticles |
| GPU-bound frame, a few dozen particles in many emitters | CPUParticles |
| Custom motion rules, per-particle state | GPUParticles + particle shader |
| Identical deterministic motion (rain sheet, star field) | MultiMesh + vertex shader |

Build with GPU nodes, then use the editor's convert menu or `convert_from_particles()` to make
the CPU fallback. GPU-only features do not convert.

### Renderer support table

| Feature | Forward+ / Mobile | Compatibility |
|---|---|---|
| Trails | yes | no |
| SDF collision (3D) | yes | no |
| `emit_particle()` | yes | no |
| Decals | yes | no |
| Turbulence | yes, costly | costly; avoid on web and mobile |
| Attractors | 3D only | 3D only |

### Pooled one-shot bursts

Do not instantiate an emitter per hit. Keep a small ring of `one_shot` emitters and call
`restart()`. Setting `emitting = true` on a one-shot does nothing until all its particles died,
and there is a short dead time even after `finished`. `restart()` has neither problem, but it
clears live particles, hence the ring. `explosiveness = 1.0` fires all `amount` at once.

```gdscript
extends Node3D

@export var emitters: Array[GPUParticles3D] = []
var _next: int = 0

func burst(at: Vector3) -> void:
	if emitters.is_empty():
		return
	var p: GPUParticles3D = emitters[_next]
	_next = (_next + 1) % emitters.size()
	p.global_position = at
	p.restart()
```

### One emitter, many spawn points with emit_particle

On Forward+ and Mobile, `emit_particle()` puts single particles into a running system with your
transform, velocity, color and custom data, chosen by `EMIT_FLAG_*` bits. One always-on emitter
(with `emitting` off) can then serve every bullet impact in view: one draw, one simulation. The
default process material overwrites the color and reads `custom` as (rotation, age, animation,
lifetime). Capacity is still `amount`.

```gdscript
extends GPUParticles3D

func spark(at: Vector3, dir: Vector3) -> void:
	var flags: int = EMIT_FLAG_POSITION | EMIT_FLAG_VELOCITY
	emit_particle(Transform3D(Basis.IDENTITY, at), dir * 6.0, Color.WHITE, Color.BLACK, flags)
```

### Vary density without a restart

Changing `amount` restarts the system. Animate `amount_ratio` instead (engine trail by speed,
rain intensity, quality setting). The buffer stays at `amount`, so size `amount` for the peak.

### Fixed FPS as a quality dial

Lower `fixed_fps` cuts simulation cost, most visibly with collision. Keep `interpolate` on.
Fast particles tunnel through thin colliders at low values: raise `fixed_fps` for that emitter
only, or raise `collision_base_size`. `preprocess` runs the shader `fixed_fps` times per
preprocessed second in one frame; large values hitch and can crash the GPU. Keep it to a few
seconds, or bake the look into a flipbook.

### Get the visibility box right

Default 3D box is 8 m around the origin. Too small: the whole effect vanishes when the origin
leaves the view, and collision stops. Too large: the system simulates while off screen. Use the
editor's Generate AABB / Generate Visibility Rect, then pad for emitter motion. With
`local_coords` off, particles stay in world space while the box moves with the node: a fast
emitter leaves its smoke outside the box.

### Sub-emitters

Set `sub_emitter` to another particle node and pick `sub_emitter_mode` (constant, at end, at
collision, at start). The target stops emitting by itself. Its `amount` caps all children: when
full, spawns are dropped silently. Rain: parent with `COLLISION_HIDE_ON_CONTACT`, child splash
at collision. A system cannot be its own sub-emitter. In a custom shader, call
`emit_subparticle()` with `FLAG_EMIT_*` bits.

### Collision choices in 3D

- Box and sphere colliders: cheapest, can move.
- `GPUParticlesCollisionHeightField3D`: built live from meshes, no overhangs. `UPDATE_MODE_WHEN_MOVED`
  refreshes only when the collider node (or the camera with `follow_camera_enabled`) moves, not
  when meshes inside it move. `UPDATE_MODE_ALWAYS` is costly; use the lowest resolution with it.
- `GPUParticlesCollisionSDF3D`: baked in the editor, static, handles caves and interiors, costs
  more; resolution 16 to 512 cubed. Raise `thickness` if fast particles pass through.
- Parent a collider to a moving body to follow it. Masks: collider `cull_mask` against the
  emitter's visibility layers.

### Attractors and turbulence

Attractors (sphere, box, vector field) are 3D only and need `attractor_interaction_enabled`.
Turbulence samples 3D noise per particle per step: keep it to a few systems on screen and off on
mobile and web. A `GPUParticlesAttractorVectorField3D` fed by a `NoiseTexture3D` gives similar
swirl in world space and is shared by all systems inside it.

### Trails

3D: enable `trail_enabled`, use a `RibbonTrailMesh` or `TubeTrailMesh` as the draw pass, and set
`BaseMaterial3D.use_particle_trails` on its material, or nothing shows. Section counts live on
the mesh. 2D: `trail_sections` and `trail_section_subdivisions` on the node. For one long trail
behind one object (sword arc, missile), a `Line2D` or a script-built ribbon mesh is cheaper than a
particle system.

### Custom particle shaders

Start from a tuned `ParticleProcessMaterial` and convert it to a `ShaderMaterial`. Useful
built-ins: `RESTART_*` flags, `USERDATA1`..`USERDATA6` for extra state, `COLLIDED` and
`COLLISION_NORMAL`, `ATTRACTOR_FORCE`, `EMITTER_VELOCITY`, `AMOUNT_RATIO`. Render modes:
`keep_data` (state survives a restart), `disable_velocity`, `disable_force`,
`collision_use_scale`. `interp_to_end` only works with the stock material unless you read
`INTERPOLATE_TO_END` yourself.

### Cheaper stand-ins

- MultiMesh with a vertex shader: positions come from `INSTANCE_CUSTOM` and `TIME`, no
  simulation at all. Set `MultiMesh.custom_aabb` and use `visible_instance_count` as the dial.
- Flipbook on one quad: pre-rendered smoke or explosion (`CanvasItemMaterial.particles_animation`
  in 2D, `BILLBOARD_PARTICLES` with `particles_anim_h_frames` in 3D).
- `Decal` for scorch marks and puddles instead of lingering ground particles (not on
  Compatibility).
- Additive `Sprite2D` flash instead of a 2D light plus particles.

## Niche but useful

- `request_particles_process()` with `speed_scale = 0.0` seeks a system: scrub VFX in cutscenes.
- `use_fixed_seed` and `seed` give repeatable bursts for replays, tests and Movie Maker.
- `emission_point_texture` (editor: create emission points from a mesh) spawns on a model's
  surface.
- `DRAW_ORDER_INDEX` is the only 3D draw order with motion vectors: use it for opaque particles
  under TAA to stop ghosting.
- `inherit_velocity_ratio` makes sparks carry the emitter's speed.
- `GeometryInstance3D.visibility_range_end` on an emitter is a free distance cutoff.
- `transform_align` with `TRANSFORM_ALIGN_Y_TO_VELOCITY` stretches streaks without a shader.

## Traps

- Shaded, large, overlapping billboards are a fill-rate cost, not a particle-count cost. Use
  unshaded or vertex-lit materials and fewer, larger quads.
- An effect added by script compiles its pipelines on first show. Keep a hidden instance in the
  scene from load time (see `shaders-advanced`).
- `alpha_curve` and `emission_curve` multiply vertex color: a 3D material needs
  `vertex_color_use_as_albedo`.
- A black-background flipbook needs Add blend, or an alpha channel.
- `fixed_fps` does not slow the effect; `speed_scale` does.
- The 2D particle page still tells you to turn physics interpolation off on 2D particle nodes
  (note written for 4.3). Test before relying on either behaviour.

## Version notes

- 4.4: `restart(keep_seed)` on all four particle nodes.
- 4.7: `request_particles_process()` gained `process_time_residual`. Per-axis 3D scale and
  rotation (`use_scale_3d`, `use_rotation_3d`), `TRANSFORM_ALIGN_LOCAL_BILLBOARD` and
  `particle_flag_inherit_emitter_scale` are present in the 4.7 class reference.

## Related skills

- `godot-particles-vfx`: setup steps and common effect recipes.
- `godot-shader-basics`: draw-pass material shaders.
- `godot-perf-measure`: warm-up lists and hitch maps.
- `pixel-vfx`: pre-rendered pixel-art effect sheets as flipbook sources.
- Sibling shelves: `shaders-advanced`, `2d-rendering`, `gpu-3d-optimization`.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/3d/particles/index.html
- https://docs.godotengine.org/en/stable/tutorials/3d/particles/properties.html
- https://docs.godotengine.org/en/stable/tutorials/3d/particles/collision.html
- https://docs.godotengine.org/en/stable/tutorials/3d/particles/attractors.html
- https://docs.godotengine.org/en/stable/tutorials/3d/particles/subemitters.html
- https://docs.godotengine.org/en/stable/tutorials/3d/particles/trails.html
- https://docs.godotengine.org/en/stable/tutorials/3d/particles/turbulence.html
- https://docs.godotengine.org/en/stable/tutorials/2d/particle_systems_2d.html
- https://docs.godotengine.org/en/stable/tutorials/shaders/shader_reference/particle_shader.html
- https://docs.godotengine.org/en/stable/tutorials/rendering/renderers.html
- https://docs.godotengine.org/en/stable/tutorials/performance/pipeline_compilations.html
- https://docs.godotengine.org/en/stable/classes/class_gpuparticles3d.html
- https://docs.godotengine.org/en/stable/classes/class_particleprocessmaterial.html
