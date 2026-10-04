# Third-person and cover shooter

Shooters seen from over the shoulder, often with cover and aim assist for
pads. Gears of War, The Division and Uncharted are the reference points.
For first-person feel (view model, bob, mouse look) read `shooter-fps.md`.

## Core loop

Move between cover → peek and aim → fire → confirm the hit → pick the next
target → advance.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Character and camera | over-the-shoulder SpringArm3D, shoulder swap, aim zoom | `godot-player-controller`, `godot-camera-system` |
| Cover | detect cover, snap, peek, blind fire | this reference |
| Weapons | hitscan or projectile; data in Resources | `godot-combat-system`, `godot-resource-pattern` |
| Hits | ray and shape queries, damage, hit zones | `godot-physics-system` |
| Aim assist | slowdown and soft lock near targets for pads | `godot-input-handling` |
| Enemies | cover-using AI, flanking | `godot-ai-navigation`, `godot-limboai` |
| Animation | upper-body aim layer, cover poses | `godot-animation-system` |
| Netcode | server-validated hits, rewind | `godot-multiplayer-sync` |

## Scene tree (4.7)

```text
Player (CharacterBody3D)
├── Model (Node3D) → Skeleton3D, AnimationTree (aim blend layer)
├── CameraPivot (Node3D; yaw)
│   └── SpringArm3D (pitch; collision mask = world only)
│       └── Camera3D (offset to the right shoulder)
├── CoverProbe (Node3D; ray origins at waist and head height)
├── Weapon (Node3D) → Muzzle (Marker3D)
└── AimAssist (Node)
```

## Genre code

Aim from the camera, fire from the muzzle. Find what the crosshair points
at with a camera ray, then fire from the muzzle to that point. A second ray
from the muzzle catches a wall between the gun and the target, which the
camera does not see.

```gdscript
extends Node3D

@export var camera: Camera3D
@export var muzzle: Marker3D
@export var max_range := 200.0
@export var shooter: CollisionObject3D

func fire() -> Dictionary:
	var space := get_world_3d().direct_space_state
	var center := get_viewport().get_visible_rect().size / 2.0
	var from := camera.project_ray_origin(center)
	var to := from + camera.project_ray_normal(center) * max_range
	var aim := PhysicsRayQueryParameters3D.create(from, to)
	aim.exclude = [shooter.get_rid()]
	var aim_hit := space.intersect_ray(aim)
	var aim_point: Vector3 = aim_hit.get("position", to)
	var shot := PhysicsRayQueryParameters3D.create(muzzle.global_position, aim_point)
	shot.exclude = [shooter.get_rid()]
	return space.intersect_ray(shot)   # empty Dictionary = miss
```

Cover: cast two short rays forward from waist and head height. Waist hit and
head miss is low cover (crouch, peek over); both hit is high cover (peek
around the edge).

## Pitfalls

- Hit checks in `_process()`. Run queries in `_physics_process()`.
- The ray hits the shooter. Add the shooter's RID to `exclude`.
- `Area3D` overlaps for bullets. Fast bullets skip them; use rays, or shape
  casts for thick shots.
- Firing straight from the muzzle along its own forward vector. The bullet
  misses what the crosshair shows. Aim through the camera point as above.
- A SpringArm3D that collides with the player's own body. Exclude the
  player, or give the arm a mask with only world layers.
- Aim assist that snaps hard. Slow the stick near a target and pull gently;
  use it only with gamepad input.
- Weapon numbers in scripts. Use weapon Resources.
- Recoil only on the gun model. Kick the camera and grow the spread.
- Client-decided damage. The server validates with rewind; the client may
  show tracers and impacts early.
- TCP-style reliable sends for movement. Use ENet's unreliable channels.
