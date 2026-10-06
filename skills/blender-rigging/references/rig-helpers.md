# Rig helpers

Load at step 2. Paste the functions you need at the top of your task script, or keep them in `Scripts/Blender/rig_helpers.py` and import it. Every function was run against the `bpy` 5.0.1 module in a background process: a two-bone tube was built and skinned, `weight_problems` reported 32 unnormalized vertices before `clean_weights` and none after, a 45-degree pose moved the top of the mesh in world space and the restore returned it, and an `AS_` action exported to FBX with `root`, `lower` and `upper` bones and re-imported with its 24 frames.

```python
import bpy


def build_armature(name, bones):
    """Armature object `Armature` with a `root` bone at the origin.

    bones: list of (bone_name, parent_name, head_xyz, tail_xyz, deform).
    Parent "root" for top-level bones. Heads and tails in metres, world
    space, with the armature object at the origin and unrotated.
    """
    arm_data = bpy.data.armatures.new(f"SKEL_{name}")
    arm = bpy.data.objects.new("Armature", arm_data)
    bpy.context.scene.collection.objects.link(arm)
    bpy.ops.object.select_all(action='DESELECT')
    arm.select_set(True)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode='EDIT')
    eb = arm_data.edit_bones
    root = eb.new("root")
    root.head, root.tail = (0, 0, 0), (0, 0.1, 0)   # points +Y, along the bone axis rule
    root.use_deform = False                         # root carries no weights
    for bname, parent, head, tail, deform in bones:
        b = eb.new(bname)
        b.head, b.tail = head, tail
        b.parent = eb[parent]
        b.use_deform = deform
    bpy.ops.object.mode_set(mode='OBJECT')
    return arm


def skin_auto(mesh_ob, arm):
    """Parent with automatic weights. Apply the mesh's scale and rotation first."""
    bpy.ops.object.select_all(action='DESELECT')
    mesh_ob.select_set(True)
    arm.select_set(True)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.parent_set(type='ARMATURE_AUTO')


def clean_weights(mesh_ob, max_influences=4, threshold=0.01):
    """Drop tiny weights, cap influences per vertex, renormalize to 1."""
    bpy.ops.object.select_all(action='DESELECT')
    mesh_ob.select_set(True)
    bpy.context.view_layer.objects.active = mesh_ob
    bpy.ops.object.vertex_group_clean(group_select_mode='ALL', limit=threshold)
    bpy.ops.object.vertex_group_limit_total(group_select_mode='ALL', limit=max_influences)
    bpy.ops.object.vertex_group_normalize_all(group_select_mode='ALL', lock_active=False)


def weight_problems(mesh_ob, arm, max_influences=4, eps=1e-3):
    """Skinning problems that Unreal imports badly or not at all."""
    deform = {b.name for b in arm.data.bones if b.use_deform}
    groups = {g.index: g.name for g in mesh_ob.vertex_groups}
    out = {
        "unweighted_verts": 0,
        "not_normalized": 0,
        "over_influence": 0,
        "groups_without_bone": sorted(n for n in groups.values() if n not in deform),
        "deform_bones_without_group": sorted(deform - set(groups.values())),
        "weights_on_root": 0,
    }
    for v in mesh_ob.data.vertices:
        ws = [(groups[g.group], g.weight) for g in v.groups if g.weight > 0 and groups[g.group] in deform]
        if not ws:
            out["unweighted_verts"] += 1
            continue
        if abs(sum(w for _, w in ws) - 1.0) > eps:
            out["not_normalized"] += 1
        if len(ws) > max_influences:
            out["over_influence"] += 1
        if any(n == "root" for n, _ in ws):
            out["weights_on_root"] += 1
    mod = next((m for m in mesh_ob.modifiers if m.type == 'ARMATURE'), None)
    out["armature_modifier"] = bool(mod and mod.object == arm)
    return out


def pose_test(arm, bone_name, axis='X', degrees=45.0):
    """Rotate one pose bone for a deformation check; returns a restore function."""
    import math
    pb = arm.pose.bones[bone_name]
    old_mode, old = pb.rotation_mode, pb.rotation_quaternion.copy()
    pb.rotation_mode = 'XYZ'
    setattr(pb.rotation_euler, axis.lower(), math.radians(degrees))
    bpy.context.view_layer.update()

    def restore():
        pb.rotation_euler = (0, 0, 0)
        pb.rotation_mode = old_mode
        pb.rotation_quaternion = old
        bpy.context.view_layer.update()
    return restore


def actions_for_export(arm):
    """Actions that will be exported, with frame ranges and AS_ naming check."""
    out = []
    for act in bpy.data.actions:
        start, end = act.frame_range
        out.append({
            "action": act.name,
            "frames": (int(start), int(end)),
            "named_ok": act.name.startswith("AS_"),
            "fake_user": act.use_fake_user,
        })
    return out
```

## Notes

- `build_armature` points `root` along `+Y`, matching `primary_bone_axis='Y'` in the export settings.
- `skin_auto` uses heat weighting (`ARMATURE_AUTO`). On failure the log says so, and vertices are left unweighted; `weight_problems` counts them.
- `clean_weights` runs clean, then limit, then normalize. Normalize goes last because clean and limit remove weight and leave sums below 1.
- `weight_problems` ignores vertex groups for non-deform bones when it sums weights, as the exporter does with `use_armature_deform_only=True`.
- `pose_test` changes the pose bone's rotation mode for the test. Always call the restore function it returns, even when an assertion fails (use `try`/`finally`).
- Measure deformation in world space (`ob.matrix_world @ v.co` on the evaluated mesh). A mesh whose location was not applied reads offset in local coordinates.
- Blender's FBX importer can shift a re-imported action's frame range by one frame. Compare the frame count, not the start frame.
- Shape keys survive an FBX export only with `use_mesh_modifiers=False`. With it on and any modifier on the mesh, they are dropped without an error. Triangulating the mesh data with `bmesh.ops.triangulate` keeps the shape keys.
