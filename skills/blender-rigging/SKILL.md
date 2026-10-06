---
name: blender-rigging
description: Use when a Blender mesh must deform in a game engine - building an armature with a root bone, skinning with automatic weights, cleaning, limiting and normalizing weights, testing deformation in poses, shape keys for morph targets, and authoring actions for export as separate animations. Pairs with blender-ue-pipeline, which owns the skeleton naming and FBX export settings.
---

# Blender rigging, skinning and animation

Make a skeletal mesh that imports into Unreal with a clean skeleton, deforms correctly and carries its animations. `blender-ue-pipeline` owns the conventions: the armature object named `Armature`, a single `root` bone, deform bones only, `SK_`/`SKEL_`/`AS_` names, and the FBX settings. This skill owns building the rig, the weights and the actions.

Tested helper functions are in `references/rig-helpers.md`, run against the `bpy` 5.0.1 module. Confirm calls on the installed Blender with `get_python_api_docs`.

## When to use

- A mesh needs a skeleton and skin weights.
- A rig deforms badly: candy-wrapper twists, collapsing joints, vertices that stay behind.
- Unreal reports skeleton problems on import: extra root bones, unweighted vertices, too many influences, a skeleton that does not match.
- An animation must be made, renamed or exported as its own file.
- Not for: mesh topology (`blender-modeling`); bakes (`blender-baking`); Unreal-side retargeting or Animation Blueprints (the UE teammates).

## Inputs

- The brief: the skeleton to match (a project skeleton, the UE Mannequin, or a new one), the bone list, the influence limit, the animations and frame rate.
- If the brief says to match an existing Unreal skeleton, get its bone names and hierarchy from the UE teammate through the orchestrator. Do not guess bone names; a mismatch makes Unreal create a second skeleton.

## Workflow

1. **Prepare the mesh.** Apply rotation and scale on the mesh and put its origin at the world origin. Check that the mesh has edge loops at each joint (at least three around an elbow or knee for a smooth bend). If loops are missing, report it; weights cannot fix missing topology.

2. **Build the armature.** `build_armature(name, bones)` makes an armature object named `Armature` with a `root` bone at the origin, with `use_deform` off. Rules:
   - Every other bone descends from `root`.
   - Bones that should drive vertices have `use_deform` on. Helper bones (IK targets, poles, controls) have it off, so `use_armature_deform_only` leaves them out of the export. The exporter still keeps a non-deform bone that is the parent of a deform bone; that is how `root` survives. So never parent a deform bone to a helper bone.
   - Place joints at the pivot of the real joint: the centre of the elbow volume, not its surface.
   - Keep bone rolls consistent along a chain, so rotations around one local axis bend all joints the same way. Recalculate roll (`bpy.ops.armature.calculate_roll`) for each chain in edit mode if they are not.
   - Names: ASCII, no spaces, and `_l`/`_r` (or `.L`/`.R`) suffixes for sides, so mirroring tools work. Match the target skeleton exactly when there is one.
   Check: one root, the hierarchy matches the brief, the armature object is named `Armature`, its scale is 1.

3. **Skin.** `skin_auto(mesh_ob, arm)` parents the mesh with automatic (heat) weights. If heat weighting fails (an error in the log, or vertices left with no weight), the mesh usually has non-manifold or overlapping geometry: fix it with `blender-modeling` step 3, or temporarily merge doubles and retry. Separate accessories (a strap, buttons) can be weighted 100 % to one bone. Check: the mesh has an Armature modifier pointing at the armature.

4. **Clean the weights.** `clean_weights(mesh_ob, max_influences, threshold)` removes weights under the threshold, caps the influences per vertex, and normalizes the sum to 1. Use the brief's influence limit. If there is none, use 4 and say so. Unreal accepts more, but each influence costs skinning time. Then run `weight_problems(mesh_ob, arm, max_influences)` and fix to zero: unweighted vertices, unnormalized vertices, too many influences, vertex groups with no deform bone, and weights on `root`. Check: every count is 0, and `armature_modifier` is true.

5. **Test the deformation.** For each major joint, rotate it with `pose_test` (45 to 90 degrees on its main axis) and look at the result with a render or a screenshot, or measure the volume. Watch for: shoulders and hips that collapse, elbows and knees that pinch, forearm twist that wraps like a candy wrapper (add a twist bone that shares the rotation), and vertices that stay behind. Fix weights in the script (vertex group assignments by region), not by hand in weight paint mode, so the fix can be repeated. Always restore the rest pose afterwards. Check: no visible collapse at the brief's range of motion; the rest pose is restored.

6. **Shape keys (morph targets).** Unreal imports shape keys as morph targets. Name them for their purpose (`Blink_L`, `Smile`). The basis shape key is the rest mesh. Warning: the FBX exporter's Apply Modifiers option (`use_mesh_modifiers=True`) prevents exporting shape keys. For a mesh with shape keys, triangulate the mesh data itself (`bmesh.ops.triangulate` on all faces, on the basis and therefore on every key) and remove the Triangulate modifier, then export with `use_mesh_modifiers=False`. Write in the hand-off that this asset has morph targets. Check: the exported file has the shape keys (import it into an empty scene and count them).

7. **Author the actions.** One action per animation, named `AS_<Name>_<Action>`. Rules:
   - Key only the deform bones (and `root` for root motion). Keys on helper bones are fine if constraints bake them onto deform bones at export.
   - Set the frame rate from the brief (`scene.render.fps`) before keying.
   - Loops: the last frame matches the first, and the exported range excludes the duplicate, or Unreal plays the pose twice.
   - Root motion: move the `root` bone, not the armature object, and say "root motion" in the hand-off so the UE teammate enables it on the sequence.
   - Turn on the fake user (`action.use_fake_user = True`) for every action, so an unassigned action is not deleted when the file is saved.
   Check: `actions_for_export(arm)` lists every action with the `AS_` prefix and the expected frame range.

8. **Export each animation.** Per `blender-ue-pipeline`, one action per file: assign the action (`arm.animation_data.action = act`), set `scene.frame_start` and `scene.frame_end` to its range, and export with `bake_anim=True`, `bake_anim_use_all_actions=False`, `bake_anim_use_nla_strips=False`. Set `bake_anim_simplify_factor=0.0` when exact keys matter (fast motions, contact frames); the default simplifies curves. Export the mesh once without animation (`bake_anim=False`) as `SK_<Name>.fbx`. Check: each `AS_*.fbx` exists, and re-importing it gives the expected frame count.

9. **Verify in a new process and hand over.** Re-open the saved file and assert: the armature name, one root, deform-bone count, `weight_problems` all zero, the shape key names, the actions and their ranges. In the hand-off, list the skeleton (bone count, root, whether it matches an existing UE skeleton), the influence limit, the morph targets, each animation file with its frame count, frame rate, loop flag and root motion flag.

## Common mistakes

- An armature object not named `Armature`: Unreal adds it as an extra root bone.
- Weights on `root`: the whole mesh drifts with root motion.
- Unapplied armature or mesh scale: bones import 100 times too big or too small, or the skeleton is scaled.
- Non-deform helper bones left with `use_deform` on: they appear in the Unreal skeleton.
- Actions without a fake user: lost on save.
- Shape keys exported with Apply Modifiers on: they are silently dropped.
- Checking deformation in local coordinates of an object whose location was not applied: the numbers are offset. Compare world-space positions (`matrix_world @ co`).

## Review checklist

- [ ] Armature object named `Armature`; one `root` bone at the origin, with no weights.
- [ ] Only deform bones have `use_deform`; names match the target skeleton if there is one.
- [ ] `weight_problems` reports zeros; the influence limit is the brief's (or 4, stated).
- [ ] Each major joint was posed and checked; the rest pose was restored.
- [ ] Shape keys, if any, exported with modifiers off, and counted after re-import.
- [ ] Each action is `AS_<Name>_<Action>`, has a fake user, and was exported to its own file with the right range.
- [ ] The hand-off lists the skeleton, morph targets, and each animation with frames, rate, loop and root motion.

## References

- `references/rig-helpers.md`: load at step 2. Armature building with a root bone, automatic skinning, weight cleanup, the weight problem report, pose testing and the action listing, as tested Python.
