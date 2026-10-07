---
name: godot-dimension-port
description: "Use when a Godot 4.7 project moves between 2D and 3D: a 2D game rebuilt in 3D or 2.5D (sprites in a 3D world), or a 3D game reduced to 2D, top-down with simulated height, or isometric. Covers the choice of target, the coordinate and unit mapping (pixels and +Y down against meters, +Y up and -Z forward), the 2D-to-3D node and shape map, physics layers and gravity, camera rigs, movement, sprites in 3D with 8-direction views, TileMapLayer to GridMap, height simulation and Y-sort in 2D, isometric tiles, audio, navigation, and what a headless worker cannot do (baking 3D models to sprites needs a renderer). Code is own GDScript, tested on Godot 4.7.2."
---

# Godot dimension port (2D ↔ 3D, Godot 4.7)

A port between 2D and 3D is a rewrite of the scene tree, not a rename.
Godot keeps 2D and 3D apart: different nodes, physics servers, layer
tables, units and cameras. This skill gives the map and the order of work.

| file | read when |
|---|---|
| [references/2d-to-3d.md](references/2d-to-3d.md) | 2D → 3D or 2D → 2.5D. |
| [references/3d-to-2d.md](references/3d-to-2d.md) | 3D → 2D, top-down with height, or isometric. |

Read with it: `godot-scene-files`, `godot-build-verify`, and the target side's
skills (`godot-3d-essentials` or `godot-2d-essentials`, `godot-physics-system`,
`godot-camera-system`, `godot-player-controller`).

## Step 1: choose the target, then ask once

Pick one target and write it in the plan. Send one `QUESTION:` to the
orchestrator with your recommendation when the task does not say.

| target | what it is | pick it when |
|---|---|---|
| full 3D | meshes, 3D physics, a free or follow camera | depth is part of the play (aim, height, look around) |
| 2.5D | 3D world and physics, flat sprites (`Sprite3D`) | 2D art must stay, but the camera or levels need depth |
| 2D top-down with height | 2D physics on the floor plane, height is a number | the game is read from above; jumps and fliers matter |
| 2D side view | 2D physics, the 3D depth axis dropped | the play happens on one plane |
| isometric 2D | `TileMapLayer` with an isometric `TileSet`, Y-sort | a 3D look with 2D art and 2D cost |

Do not use an orthographic `Camera3D` to fake 2D: you keep the 3D
renderer, lights and physics. Do not port 2D to 3D by putting every node at
`z = 0`: that is a 2D game with a 3D cost.

## Step 2: fix the mapping before you write code

| | 2D | 3D |
|---|---|---|
| unit | pixel | meter (1 unit = 1 m; physics and light assume it) |
| up | `-Y` (`Vector2.UP` is `(0, -1)`) | `+Y` |
| forward | none; a sprite usually faces `+X` | `-Z` (`Vector3.FORWARD`) |
| rotation | one `float`, positive turns clockwise on screen | `Basis` / `Quaternion`; `rotation.y` positive turns counter-clockwise seen from above |
| default gravity | `physics/2d/default_gravity` = 980 (px/s²) | `physics/3d/default_gravity` = 9.8 (m/s²) |
| physics layer names | `layer_names/2d_physics/layer_N` | `layer_names/3d_physics/layer_N` (a separate table) |

Pick one `pixels_per_meter` (for example 64) and use it for every position,
speed, gravity and shape size. A top-down 2D plane maps to the 3D ground:
2D `+X` → 3D `+X`, 2D `+Y` → 3D `+Z`. A side view maps 2D `+X` → 3D `+X`,
2D `-Y` → 3D `+Y`.

```gdscript
class_name PlaneMap
extends RefCounted

## Maps between a 2D top-down plane (pixels, +Y down) and the 3D ground
## plane (meters, +Y up, -Z forward). 2D +X -> 3D +X, 2D +Y -> 3D +Z.

static var pixels_per_meter: float = 64.0


static func to_3d(p: Vector2, height: float = 0.0) -> Vector3:
	return Vector3(p.x, 0.0, p.y) / pixels_per_meter + Vector3(0.0, height, 0.0)


static func to_2d(p: Vector3) -> Vector2:
	return Vector2(p.x, p.z) * pixels_per_meter


## 2D rotation (radians, 0 = facing +X) -> Node3D rotation.y for a model
## that faces -Z.
static func yaw_from_2d_rotation(rotation_2d: float) -> float:
	var dir := Vector2.from_angle(rotation_2d)
	return atan2(-dir.x, -dir.y)


## Node3D rotation.y (model faces -Z) -> 2D rotation (0 = facing +X).
static func rotation_2d_from_yaw(yaw: float) -> float:
	var forward := Basis(Vector3.UP, yaw) * Vector3.FORWARD
	return Vector2(forward.x, forward.z).angle()
```

Tested on 4.7.2: for 6 angles, the yaw turns `Vector3.FORWARD` onto the
mapped 2D direction, and the round trip returns the start angle.

## Step 3: map the nodes

Same idea, different node. Shapes, layers and units do not carry over: make
each shape again at the new scale.

| 2D | 3D | note |
|---|---|---|
| `Node2D` | `Node3D` | |
| `CharacterBody2D` / `RigidBody2D` / `StaticBody2D` / `AnimatableBody2D` | the `3D` node | `up_direction`, gravity and speeds change unit |
| `Area2D` | `Area3D` | |
| `CollisionShape2D` + `RectangleShape2D` / `CircleShape2D` / `CapsuleShape2D` | `CollisionShape3D` + `BoxShape3D` / `SphereShape3D` (or `CylinderShape3D`) / `CapsuleShape3D` | both `size` values are full sizes |
| `CollisionPolygon2D` | `CollisionPolygon3D` (extruded) or `ConvexPolygonShape3D` | |
| `RayCast2D` / `ShapeCast2D` | `RayCast3D` / `ShapeCast3D` | `target_position` is a `Vector3` |
| `Camera2D` | `Camera3D` (+ `SpringArm3D` for a follow camera) | see the references |
| `Sprite2D` / `AnimatedSprite2D` | `Sprite3D` / `AnimatedSprite3D`, or `MeshInstance3D` | `pixel_size` sets meters per pixel |
| `TileMapLayer` | `GridMap` + `MeshLibrary` | copy code in `2d-to-3d.md` |
| `PointLight2D` / `DirectionalLight2D` | `OmniLight3D` / `DirectionalLight3D` | a 3D scene also needs `WorldEnvironment` |
| `LightOccluder2D` | shadows from meshes (`shadow_enabled` on the light) | |
| `GPUParticles2D` / `CPUParticles2D` | the `3D` node | `ParticleProcessMaterial` works in both; give its values again in meters |
| `AudioStreamPlayer2D` / `AudioListener2D` | `AudioStreamPlayer3D` / `AudioListener3D` | distance unit changes |
| `NavigationRegion2D` (`NavigationPolygon`) | `NavigationRegion3D` (`NavigationMesh`) | bake again |
| `NavigationAgent2D` | `NavigationAgent3D` | |
| `Path2D` / `PathFollow2D` | `Path3D` / `PathFollow3D` | `progress` on both |
| `Marker2D`, `RemoteTransform2D`, `VisibleOnScreenNotifier2D` | the `3D` node | |
| `Parallax2D` | real depth in the 3D scene | |
| `Line2D`, `Polygon2D` | `MeshInstance3D` (or `CSGPolygon3D` along a `Path3D`) | |
| `Skeleton2D` | `Skeleton3D` | rig and animations are made again |
| `CanvasLayer` + `Control` UI | unchanged | UI stays 2D on top of a 3D world |

## Step 4: port in this order

1. Project settings: name the layers in the target table; set the gravity.
2. One test level: floor, player, camera, light. Make it run first.
3. Movement and camera (code in the references).
4. Collision layers and masks on every body and area. Use
   `set_collision_layer_value(n, true)`; never copy bit masks without
   checking the names in both tables.
5. Level geometry (GridMap, meshes, or tiles), then props and enemies.
6. Navigation: bake for the new geometry.
7. Audio, particles, UI that points at world positions.
8. Run the parse check and the tests (`godot-build-verify`) after each step.

## Headless limits

> proof: not run (needs a GPU renderer) for sprite bakes, screenshots and visual checks.

- Under `--headless` the renderer is a dummy. Measured on 4.7.2: a
  `SubViewport` with a `Camera3D` draws nothing, `RenderingServer.frame_post_draw`
  never fires, and `get_texture().get_image()` returns null. A fleet worker
  cannot bake 3D models into sprite sheets, take screenshots, or judge
  lighting. Send `QUESTION:` for an operator step, and list the scenes that
  need a visual check in `DONE:`.
- Art is binary. New meshes, sprite sheets and normal maps are a human's
  work: list them in `DONE:`.

## DONE

A `DONE:` names the target choice, `pixels_per_meter`, the scenes ported,
the parse check and test results, and the human work still open (art,
visual checks, sprite bakes).

## Related skills

`godot-version-migration`, `godot-2d-essentials`, `godot-3d-essentials`,
`godot-physics-system`, `godot-camera-system`, `godot-player-controller`,
`godot-ai-navigation`, `godot-audio-system`, `godot-math-essentials`.
