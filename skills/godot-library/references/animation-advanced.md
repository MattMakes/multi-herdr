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
  which animations play at which weight and time. Both run the same blend code, so root motion
  and capture work in either, and an `AnimationTree` can hold its own libraries with no
  `AnimationPlayer`. The two differ only in defaults, kept for old projects.
- One mixer update blends every track of the playing animations, applies the values and emits
  `mixer_applied`. `callback_mode_process` picks idle, physics or manual stepping
  (`AnimationMixer.advance`). `callback_mode_method` picks deferred (default, safe when a method
  track frees nodes) or immediate calls.
- `deterministic` changes the blend maths. False (the `AnimationPlayer` default): weights are
  normalized to 1.0 and a track that is missing in one animation is left alone. True (the
  `AnimationTree` default): weights are not normalized and blending starts from an initial value,
  taken from the `RESET` animation, else 0, else bone rest for 3D bone tracks. With total weight
  0 the result is the `RESET` pose. The tree needs this mode: additive nodes
  (`AnimationNodeAdd2`, `AnimationNodeSub2`) and weights that do not sum to 1 only make sense
  against a baseline, and retargeted models with extra bones would keep a stale pose without it.
- A discrete track writes its value during the blend pass, only on key frames. A continuous
  track writes after the pass, every frame. So when both hit one property, continuous wins.
  `callback_mode_discrete` makes the rule explicit. Recessive (the `AnimationPlayer` default)
  keeps that order. Dominant lets the discrete key win on its frames and can flicker.
  `ANIMATION_CALLBACK_MODE_DISCRETE_FORCE_CONTINUOUS` (the tree default) treats the track as
  continuous with nearest interpolation, so it can blend.
  Types that cannot be interpolated (object, node path, dictionary and others) fall
  back to recessive discrete. `bool` and `int` blend as floats and are rounded.
- Animation nodes are resources, so every instance of a scene shares them. Per-instance state
  lives on the tree node under `parameters/...`. Set values there, never on the shared node.
- Skeleton order per frame: the mixer writes bone poses, then `Skeleton3D` runs each child
  `SkeletonModifier3D` in child order, blends each result by its `influence`, emits
  `modification_processed` per modifier and `skeleton_updated` at the end. Skinning reads that.
  The update is deferred and runs once per frame. The skeleton saves the animated pose before
  the modifiers and restores it after skinning, so `get_bone_pose()` in `_process()` never shows
  modifier output. Read it inside those two signals. `modifier_callback_mode_process` picks
  idle, physics or manual (`Skeleton3D.advance()`, since 4.5); use physics when a physics-driven
  node feeds a modifier.
- The old pose override API was dropped because its result depended on node order in the scene
  tree and two override users could not be chained. Modifiers always run after the mixer.

## Advanced techniques

### RESET tracks as the blend baseline

In a deterministic tree, a property that only some animations key is blended against the `RESET`
value, or against 0 when `RESET` has no such track. The symptom is a position that slides to the
origin during a cross-fade. Add every blended property to `RESET` with one key at time 0.

The same reset appears in a state machine transition with zero cross-fade: it is still a blend
internally, so tracks missing in the new state snap to the baseline. This is the usual surprise in
2D projects. Either complete `RESET`, or turn `deterministic` off on the tree, which gives up
additive blending. The reverse is useful too: `deterministic = true` on an `AnimationPlayer`
makes GUI clips that key different properties return untouched ones to `RESET`.

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
  Grouped (a folder for states: it cannot be played alone, the parent playback addresses its
  states by path, such as `Attack/Attack2`, and its `state_started` and
  `state_finished` signals arrive with that prefix). Nested fits states with no transitions
  between them, which reproduces an "any state". Grouped has one entry and one exit only.
- `break_loop_at_end` on a transition or on `AnimationNodeOneShot` lets an At End switch leave a
  looping clip at the end of the current cycle.

### Locomotion that stays in phase

Blend spaces have `sync_mode` (4.7): none freezes clips with zero weight, independent advances
them, and the two cyclic modes time-scale every clip so the feet stay in phase. Cyclic modes need
every point to be an `AnimationNodeAnimation` of fixed length. For clips of different length, set
`use_custom_timeline` with `timeline_length`, `stretch_time_scale` and `start_offset` on the
animation node. That fixes which foot leads without editing the clip. Markers (4.4) can define
that timeline. Blend points have names since 4.7 (`find_blend_point_by_name()`). `blend_mode`
Discrete or Carry suits frame-by-frame 2D sets.

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

- The solvers are minimal by design. Polish is done by other modifiers around them, which also
  serve a custom solver: a constraint for a pre-twist or to point the end bone at the target,
  `BoneTwistDisperser3D` for twist, `LimitAngularVelocityModifier3D` for smoothing. Since 4.6 a
  `BoneConstraint3D` can take a `Node3D` as reference, not only a bone.
- `TwoBoneIK3D` and `SplineIK3D` always give the same pose for the same input. Iterative solvers
  (`IterateIK3D`) continue from last frame's result unless `deterministic` is on.
  `max_iterations` times `angular_delta_limit` caps the turn per frame (4 x 2 degrees by
  default). Carried over, a far target is reached over several frames. Deterministic, it may
  never be reached with small limits, so raise them.
- Deterministic IK depends only on target and animated pose. For networked avatars, replicate
  the IK target position and let each peer solve. It can turn bones very fast; follow it with
  `LimitAngularVelocityModifier3D`.
- Two deterministic `FABRIK3D` passes reproduce the "magnet" option of the old `SkeletonIK3D`.
- `mutable_bone_axes = false` (4.6) caches bone axes from rest: slightly faster, but valid only
  when no earlier modifier moves bone positions.
- Call `SpringBoneSimulator3D.reset()` after a teleport or a hard cut with no fade. Use
  `external_force` for wind. Do not scale the skeleton or its bones.
- Fade a modifier in and out with `influence`. Set `active = false` to skip its cost.

### Custom modifier

Extend `SkeletonModifier3D` and override `_process_modification_with_delta()` (4.5; the older
`_process_modification()` is deprecated). Get the skeleton with `get_skeleton()`, read
`get_bone_global_pose()` and write `set_bone_global_pose()`. Write the full result and let the
skeleton apply `influence`; add your own amounts only per bone. Mark the script `@tool` or the
editor shows no preview. For a bone picker, fill an enum hint in `_validate_property()` from
`Skeleton3D.get_concatenated_bone_names()`. It only runs under a `Skeleton3D` parent. A modifier
is a temporary effect on top of animation, not a way to edit the pose for good.

### Sharing animations between characters

Two routes. Import-time: set a `BoneMap` with `SkeletonProfileHumanoid` on each model, enable the
rest fixer (Overwrite Axis is the key option) and import the clips as an `AnimationLibrary` with
the Remove Tracks options on. Position tracks are normalized by hip height and scaled back by
`Skeleton3D.motion_scale`, so stride matches leg length. Runtime (since 4.4): put a
`RetargetModifier3D` with the profile under the source skeleton and the target skeleton below it.
The original rests are kept, which the import route discards. That matters because modifiers
such as look-at and IK take their bone axes from the rest. `use_global_pose` needs matching
bone lengths.

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
  modes. `tween_await()` (4.7) holds a step until a signal fires, which suits dialogue and
  cutscenes; add `AwaitTweener.set_timeout()` for a signal that may never come.
- `PropertyTweener.set_custom_interpolator()` takes a callable such as a `Curve` sampler.
- `set_ignore_time_scale()` (4.4) keeps UI motion at real speed during slow motion.

## Niche but useful

- Animation markers (4.4): `play_section_with_markers()` plays a named part of a clip. One long
  imported clip can serve several actions.
- `AnimationPlayer.seek(t, true, true)` updates the pose without firing method and audio tracks.
- `AnimationNodeExtension` (4.4) is the base for custom blend tree nodes written in script.

## Traps

- Bezier tracks and value tracks on the same property do not blend with each other.
- `animation_finished` is not emitted for looping clips, nor by `seek()` to the end.
- Outdated: `SkeletonIK3D` and the `Skeleton3D` global pose override methods are deprecated.
  Use the modifier nodes.

## Version notes

- Advice written for 4.0 or 4.1 on blending, discrete tracks and capture is outdated: 4.2 and
  4.3 redefined them.
- 4.4: `LookAtModifier3D`, `SpringBoneSimulator3D`, `RetargetModifier3D`, markers,
  `tween_subtween()`, `set_ignore_time_scale()`, `AnimationNodeExtension`.
- 4.5: `BoneConstraint3D` family, `ModifierBoneTarget3D`, `Skeleton3D.advance()`,
  `_process_modification_with_delta()`.
- 4.6: `IKModifier3D` family, `BoneTwistDisperser3D`, `LimitAngularVelocityModifier3D`,
  `Node3D` references in constraints. `current_animation` and `assigned_animation` became `StringName`. The default of
  `MeshInstance3D.skeleton` changed; project setting
  `animation/compatibility/default_parent_skeleton_in_mesh_instance_3d` restores the old one.
- 4.7: blend space `sync_mode` replaces the `sync` bool. A tree that stopped blending as before
  after the upgrade needs this property set. Named blend points. `tween_await()`.

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
- https://godotengine.org/article/migrating-animations-from-godot-4-0-to-4-3/
- https://godotengine.org/article/design-of-the-skeleton-modifier-3d/
- https://godotengine.org/article/inverse-kinematics-returns-to-godot-4-6/
- https://godotengine.org/releases/4.4/
- https://godotengine.org/releases/4.5/
- https://godotengine.org/releases/4.6/
- https://godotengine.org/releases/4.7/
