# Animation, advanced

Target engine: Godot 4.7. How `AnimationMixer` blends, how skeleton modifiers and IK run after
it, and how to keep many animated characters cheap.

## Use this shelf when

- Blended animations give wrong poses, pops, or values that drift toward zero.
- You add IK, look-at, spring bones or constraints on top of played animation.
- One animation set must drive several characters with different skeletons.
- Root motion or tweens must stay in step with physics.
- Animation or skinning shows up in the profiler and you need a way to cut it per character.

## How the engine works here

- `AnimationPlayer` and `AnimationTree` both extend `AnimationMixer`. The mixer owns the
  animation libraries, the track cache, the blend pass and root motion. The subclass only decides
  which animations play at which weight.
- One mixer update blends every track of the playing animations, applies the values and emits
  `mixer_applied`. `callback_mode_process` picks idle, physics or manual stepping
  (`AnimationMixer.advance`). `callback_mode_method` picks deferred (default, safe when a method
  track frees nodes) or immediate calls.
- `deterministic` changes the blend maths. False (the `AnimationPlayer` default): weights are
  normalized to 1.0 and a track that is missing in one animation is left alone. True (the
  `AnimationTree` default): weights are not normalized and blending starts from an initial value,
  taken from the `RESET` animation, else 0, else bone rest for 3D bone tracks. With total weight
  0 the result is the `RESET` pose.
- `callback_mode_discrete` decides what a discrete track does inside a blend. The tree default
  `ANIMATION_CALLBACK_MODE_DISCRETE_FORCE_CONTINUOUS` treats it as continuous with nearest
  interpolation. Types that cannot be interpolated (object, node path, dictionary and others) fall
  back to recessive discrete. `bool` and `int` blend as floats and are rounded.
- Animation nodes are resources, so every instance of a scene shares them. Per-instance state
  lives on the tree node under `parameters/...`. Set values there, never on the shared node.
- Skeleton order per frame: the mixer writes bone poses, then `Skeleton3D` runs each child
  `SkeletonModifier3D` in child order, blends each result by its `influence`, emits
  `modification_processed` per modifier and `skeleton_updated` at the end. Skinning reads that.
  `Skeleton3D.modifier_callback_mode_process` picks idle, physics or manual
  (`Skeleton3D.advance`).

## Advanced techniques

### RESET tracks as the blend baseline

In a deterministic tree, a property that only some animations key is blended against the `RESET`
value, or against 0 when `RESET` has no such track. The symptom is a position that slides to the
origin during a cross-fade. Add every blended property to `RESET` with one key at time 0.

### Rest pose decides the rotation path

Rotation 3D tracks, and 2D rotation tracks set to Linear Angle or Cubic Angle, blend along the
shortest path from the baseline, not between the two clips. A blend can never pass more than 180
degrees from rest. Import humanoids in T-pose so that rest sits near the middle of each joint's
range. For turns larger than that, rotate the character with root motion.

### Root motion in the physics step

Set `root_motion_track` to a bone track. The mixer removes that motion from the pose and exposes
it per update. Set `callback_mode_process` to physics so the delta matches the body step.

```gdscript
extends CharacterBody3D

@onready var tree: AnimationTree = $AnimationTree

func _physics_process(delta: float) -> void:
	quaternion = quaternion * tree.get_root_motion_rotation()
	var basis_fix: Quaternion = tree.get_root_motion_rotation_accumulator().inverse() * quaternion
	velocity = basis_fix * tree.get_root_motion_position() / delta
	move_and_slide()
```

`RootMotionView` gives a floor grid for preview. The `*_accumulator` values jump when a clip loops.

### Capture for blends from an unknown pose

A track in `Animation.UPDATE_CAPTURE` mode starts from the value the property has right now. With
`playback_auto_capture` (true by default) a plain `play()` does this for clips that hold capture
tracks. `play_with_capture()` sets duration, transition and ease. Use it for hit reactions and UI
that can be interrupted at any point.

### State machine structure

- `travel()` runs A* over the transitions and plays each state on the way. With no path it
  teleports. Lower `priority` wins. `get_travel_path()` shows what it chose.
- `advance_mode`: Enabled is used by `travel()` only. Auto fires on its own when
  `advance_condition` and `advance_expression` pass. Disabled blocks the edge.
- `advance_expression` runs through `Expression` on `advance_expression_base_node`. It can call
  methods and compare values, so no pairs of opposite bools are needed. It is case sensitive.
- `state_machine_type`: Root, Nested (seek and end behave as one clip inside a parent) and
  Grouped (the parent machine travels through it; its `state_started` and `state_finished`
  signals reach the parent playback with the group name as prefix).
- `break_loop_at_end` on a transition or on `AnimationNodeOneShot` lets an At End switch leave a
  looping clip at the end of the current cycle.

### Locomotion that stays in phase

Blend spaces have `sync_mode` (4.7): none freezes clips with zero weight, independent advances
them, and the two cyclic modes time-scale every clip so the feet stay in phase. Cyclic modes need
every point to be an `AnimationNodeAnimation` of fixed length. For clips of different length, set
`use_custom_timeline` with `timeline_length`, `stretch_time_scale` and `start_offset` on the
animation node. That fixes which foot leads without editing the clip. `blend_mode` Discrete or
Carry suits frame-by-frame 2D sets.

### Modifier stacks

Order the children of `Skeleton3D` on purpose: retarget, then constraints, then IK, then twist
correction, then spring bones, then look-at for the head. A parent bone modifier must sit above
the modifier of its child bone.

| Need | Node | Since |
|---|---|---|
| Head or turret aim with limits | `LookAtModifier3D` | 4.4 |
| Hair, tails, cloth strips | `SpringBoneSimulator3D` + `SpringBoneCollision3D` children | 4.4 |
| Copy or remap a bone from another | `CopyTransformModifier3D`, `ConvertTransformModifier3D` | 4.5 |
| Aim a bone at another bone | `AimModifier3D` | 4.5 |
| Limb with pole target, exact | `TwoBoneIK3D` | 4.6 |
| Long chains | `FABRIK3D`, `CCDIK3D`, `JacobianIK3D`, `SplineIK3D` | 4.6 |
| Forearm twist after IK | `BoneTwistDisperser3D` | 4.6 |
| Cap how fast a bone may turn | `LimitAngularVelocityModifier3D` | 4.6 |
| Node that follows the modified bone | `ModifierBoneTarget3D` | 4.5 |

- Iterative solvers (`IterateIK3D`) continue from last frame's result unless `deterministic` is
  on. `max_iterations` times `angular_delta_limit` caps the turn per frame (4 x 2 degrees by
  default), so a far target is reached over several frames.
- `mutable_bone_axes = false` caches bone axes from rest: slightly faster, but valid only when
  no earlier modifier moves bone positions.
- Call `SpringBoneSimulator3D.reset()` after a teleport or a hard cut with no fade. Use
  `external_force` for wind. Do not scale the skeleton or its bones.
- Fade a modifier in and out with `influence`. Set `active = false` to skip its cost.

### Custom modifier

Extend `SkeletonModifier3D` and override `_process_modification_with_delta()`. Read and write
bone poses there. Write the full result and let the skeleton apply `influence`.

### Sharing animations between characters

Two routes. Import-time: set a `BoneMap` with `SkeletonProfileHumanoid` on each model, enable the
rest fixer (Overwrite Axis is the key option) and import the clips as an `AnimationLibrary` with
the Remove Tracks options on. Position tracks are normalized by hip height and scaled back by
`Skeleton3D.motion_scale`, so stride matches leg length. Runtime (since 4.4): put a
`RetargetModifier3D` with the profile under the source skeleton and the target skeleton below it.
The original rests are kept. `use_global_pose` needs matching bone lengths.

### Animation cost at scale

Bone animation and blending run on the CPU per character. Options, cheapest first:

- Stop it: `VisibleOnScreenEnabler3D` disables processing of a target node when its box leaves
  the screen. The check is approximate and ignores occluders.
- Tick it less: set a distant mixer to `ANIMATION_CALLBACK_MODE_PROCESS_MANUAL` and call
  `advance()` every Nth frame with the summed delta.
- Replace it: for crowds, drop the skeleton and animate vertices in a shader on a `MultiMesh`,
  with phase and clip data in per-instance custom data. See `shaders-advanced`.
- Import settings: 30 FPS bake is enough because playback interpolates. Keep the optimizer on.
  Use trimming and Remove Immutable Tracks. `Animation.compress()` cuts memory for long cutscene
  clips, is lossy, and makes tracks read-only in the editor.

### Tween internals

- The `SceneTree` processes tweens, not the node. `Node.create_tween()` binds the tween: it halts
  while the node is outside the tree and dies with the node. The default pause mode
  `Tween.TWEEN_PAUSE_BOUND` follows the bound node's `process_mode`.
- A finished or killed tween is invalid and cannot take new tweeners. Create a new one per run,
  and `kill()` the old one first when two could fight over a property.
- `set_process_mode(Tween.TWEEN_PROCESS_PHYSICS)` for anything that moves a body.
- `tween_subtween()` (4.4) nests a timeline. The parent overrides the child's pause and process
  modes. `tween_await()` holds a step until a signal fires; add `AwaitTweener.set_timeout()`.
- `PropertyTweener.set_custom_interpolator()` takes a callable such as a `Curve` sampler.
- `set_ignore_time_scale()` keeps UI motion at real speed during slow motion.

## Niche but useful

- Animation markers (4.4): `play_section_with_markers()` plays a named part of a clip. One long
  imported clip can serve several actions.
- `AnimationPlayer.seek(t, true, true)` updates the pose without firing method and audio tracks.
- Import option "Save to file" keeps custom tracks on imported clips across reimports.
- `AnimationNodeExtension` (4.4) is the base for custom blend tree nodes written in script.

## Traps

- Bezier tracks and value tracks on the same property do not blend with each other.
- `animation_finished` is not emitted for looping clips, nor by `seek()` to the end.
- Outdated: `SkeletonIK3D` and the `Skeleton3D` global pose override methods are deprecated.
  Use the modifier nodes.

## Version notes

- 4.4 to 4.6: the modifier nodes in the table above; markers, `tween_subtween()` and
  `AnimationNodeExtension` in 4.4.
- 4.6: `current_animation` and `assigned_animation` became `StringName`. The default of
  `MeshInstance3D.skeleton` changed; project setting
  `animation/compatibility/default_parent_skeleton_in_mesh_instance_3d` restores the old one.
- 4.7: blend space `sync_mode` replaces the `sync` bool. A tree that stopped blending as before
  after the upgrade needs this property set.

## Related skills

- `godot-animation-system`: setup steps and recipes for players, trees and each modifier node.
- `godot-tween-animation`: tween recipes and easing tables.
- `godot-assets-pipeline`: import settings for models and clips.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/animation/introduction.html
- https://docs.godotengine.org/en/stable/tutorials/animation/animation_track_types.html
- https://docs.godotengine.org/en/stable/tutorials/animation/animation_tree.html
- https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/retargeting_3d_skeletons.html
- https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/importing_3d_scenes/import_configuration.html
- https://docs.godotengine.org/en/stable/tutorials/performance/vertex_animation/animating_thousands_of_fish.html
- https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.7.html
