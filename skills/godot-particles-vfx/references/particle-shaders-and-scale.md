Adds particle control beyond ParticleProcessMaterial: writing a `shader_type particles` process shader, collision-driven sub-particles, limiting attractors and colliders to chosen emitters, and camera-following weather collision; read it when the motion you need cannot be set up in the process material, or when attractors and colliders affect the wrong effects.

# Particle Shaders, Masks and Large-Area Effects

> ← Back to [SKILL.md](../SKILL.md). Attractor and collider setup is in [attractors-and-collision.md](attractors-and-collision.md); sub-emitter basics are in [subemitters.md](subemitters.md).

Shader code is Godot 4.7 shading language. GPU particle collision and
attractors are 3D only and need the Forward+ or Mobile renderer.

---

## 1. When to Write a Particle Shader

A `ParticleProcessMaterial` covers spawn shapes, forces, curves over lifetime,
turbulence and collision. Write a process shader when a particle must follow
a rule the material cannot express: orbiting a point, flocking to a moving
target, keeping a value across frames, or spawning a sub-particle under your
own condition.

**Start from the material, not from a blank file.** In the inspector, open
the emitter's process material menu and choose **Convert to ShaderMaterial**.
The result is a working `shader_type particles` shader with every option you
had set. Change only the part you need.

## 2. The Structure of a Process Shader

A particles shader has two functions:

- `start()` runs once when a particle spawns. Set its initial `TRANSFORM`,
  `VELOCITY`, `COLOR` and `CUSTOM` here.
- `process()` runs on every particle step for each live particle.

Useful built-ins: `TRANSFORM` (the particle's transform; `TRANSFORM[3].xyz`
is its position), `VELOCITY`, `COLOR`, `CUSTOM` (a free `vec4`),
`EMISSION_TRANSFORM` (the emitter's transform), `DELTA`, `LIFETIME`, `INDEX`
and `NUMBER` (per-particle ids), `RANDOM_SEED`, and `ACTIVE` (set it to
`false` to kill the particle). In `process()`, collision data is in
`COLLIDED`, `COLLISION_NORMAL` and `COLLISION_DEPTH`.

The shading language has no random function. Hash the particle number with
the seed instead.

```glsl
shader_type particles;
render_mode disable_velocity;

// Particles orbit the emitter and rise, fading out over their lifetime.
uniform float spawn_radius = 2.0;
uniform float orbit_speed = 1.5;
uniform float rise_speed = 0.6;
uniform vec4 base_color : source_color = vec4(0.5, 0.8, 1.0, 1.0);

float hash01(uint n) {
	n = (n << 13u) ^ n;
	n = n * (n * n * 15731u + 789221u) + 1376312589u;
	return float(n % 65536u) / 65535.0;
}

void start() {
	float angle = hash01(NUMBER + RANDOM_SEED) * TAU;
	float radius = sqrt(hash01(NUMBER * 7u + RANDOM_SEED)) * spawn_radius;
	TRANSFORM = EMISSION_TRANSFORM;
	CUSTOM = vec4(angle, radius, 0.0, 0.0); // x = angle, y = radius, z = age
	COLOR = base_color;
}

void process() {
	CUSTOM.z += DELTA;
	CUSTOM.x += orbit_speed * DELTA;
	vec3 center = EMISSION_TRANSFORM[3].xyz;
	vec3 offset = vec3(cos(CUSTOM.x) * CUSTOM.y, CUSTOM.z * rise_speed, sin(CUSTOM.x) * CUSTOM.y);
	TRANSFORM[3].xyz = center + offset;
	COLOR.a = base_color.a * clamp(1.0 - CUSTOM.z / LIFETIME, 0.0, 1.0);
}
```

`render_mode disable_velocity` stops the engine from also moving the particle
by `VELOCITY`, because `process()` sets the position itself. The age lives in
`CUSTOM.z`, so it survives from frame to frame.

The draw pass material reads `COLOR` as vertex color. On a
`StandardMaterial3D` draw material, enable **Vertex Color → Use as Albedo**
so the fade shows.

## 3. Sub-Particles on Collision

The CPU gets no event when a GPU particle hits something, so spawn the impact
effect on the GPU. In the shader converted from a material that has collision
on, find the code that handles `COLLIDED` and call `emit_subparticle()` there.
The emitter node's `sub_emitter` must point at the impact emitter.

```glsl
// Inside process(), in the shader converted from the material:
if (COLLIDED) {
	emit_subparticle(TRANSFORM, vec3(0.0), COLOR, vec4(0.0), FLAG_EMIT_POSITION);
	ACTIVE = false; // the raindrop dies; the splash takes over
}
```

Without a shader, the same effect is the material's
`sub_emitter_mode = SUB_EMITTER_AT_COLLISION` with
`sub_emitter_amount_at_collision`.

## 4. Keep Attractors and Colliders Local

An attractor or a particle collider affects **every** GPU emitter whose
`layers` overlap its `cull_mask`. With the default masks, a black-hole
attractor for a spell also pulls the rain, the torches and the dust. Assign
one render layer to the effects that should react, and set the attractor's
`cull_mask` to that layer only.

```gdscript
extends Node3D

## Layer 20 is reserved for "spell VFX" in this project.
const SPELL_VFX_LAYER := 1 << 19

@onready var vortex: GPUParticlesAttractorSphere3D = $Vortex
@onready var spell_sparks: GPUParticles3D = $SpellSparks


func _ready() -> void:
	spell_sparks.layers = SPELL_VFX_LAYER
	vortex.cull_mask = SPELL_VFX_LAYER
```

The render layer also decides which cameras draw the emitter, so make sure
the game camera's `cull_mask` includes the layer you choose.

## 5. Weather over a Large Area

Rain or snow that must stop on roofs and terrain needs collision, but a
`GPUParticlesCollisionHeightField3D` the size of the whole level is costly
and coarse. Let the height field follow the camera instead:

- Put the emitter and the height field under the camera rig, or move them
  with the camera.
- Set `follow_camera_enabled = true` on the height field. It then centers on
  the current camera.
- Set `update_mode = UPDATE_MODE_WHEN_MOVED`, so the height map renders again
  only after the camera moves.
- Keep `size` just large enough to cover what the player sees, and choose
  the lowest `resolution` that still looks right.

```gdscript
extends Node3D

## Camera-following snow collision. Child of the camera rig.
@onready var height_field: GPUParticlesCollisionHeightField3D = $SnowCollision


func _ready() -> void:
	height_field.follow_camera_enabled = true
	height_field.update_mode = GPUParticlesCollisionHeightField3D.UPDATE_MODE_WHEN_MOVED
	height_field.size = Vector3(60.0, 40.0, 60.0)
	height_field.resolution = GPUParticlesCollisionHeightField3D.RESOLUTION_512
```

The emitter's process material needs a `collision_mode` other than
`COLLISION_DISABLED`; `COLLISION_HIDE_ON_CONTACT` suits rain.
