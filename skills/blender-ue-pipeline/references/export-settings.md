# Export settings and evidence

Load at step 6. Operator names are from the Blender Python API reference (`bpy.ops.export_scene`). Defaults that the API lists and that this skill keeps are written out anyway, so a reviewer sees every value.

## FBX: static mesh

```python
import bpy

bpy.ops.export_scene.fbx(
    filepath=out_path,                  # <export folder>/SM_<Name>.fbx
    use_selection=True,                 # the mesh, its UCX_/UBX_/USP_/UCP_ children, SOCKET_ empties
    object_types={'MESH', 'EMPTY'},
    apply_unit_scale=True,
    apply_scale_options='FBX_SCALE_ALL',
    global_scale=1.0,
    axis_forward='-Z',
    axis_up='Y',
    use_space_transform=True,
    bake_space_transform=False,         # experimental; broken with armatures
    use_mesh_modifiers=True,            # applies the Triangulate modifier
    mesh_smooth_type='FACE',
    use_tspace=True,                    # needs a tris/quads-only mesh
    use_triangles=True,
    use_custom_props=False,
    path_mode='AUTO',
    embed_textures=False,
)
```

## FBX: skeletal mesh and animation

The same call, plus:

```python
    object_types={'ARMATURE', 'MESH'},
    add_leaf_bones=False,
    primary_bone_axis='Y',
    secondary_bone_axis='X',
    use_armature_deform_only=True,
    armature_nodetype='NULL',
    bake_anim=True,                     # False for a mesh-only export
    bake_anim_use_all_actions=False,    # one action per file: AS_<Name>_<Action>.fbx
    bake_anim_use_nla_strips=False,
```

## glTF: static prop, only on request

```python
bpy.ops.export_scene.gltf(
    filepath=out_path,                  # <export folder>/SM_<Name>.glb
    export_format='GLB',
    use_selection=True,
    export_yup=True,
    export_apply=True,                  # apply modifiers
    export_tangents=True,
    export_animations=False,
)
```

glTF carries metres and Y up by its specification, so there is no scale option. Whether Unreal's glTF importer reads `UCX_` collision children is not verified here. When the asset needs custom collision, use FBX.

## Verification snippet

Run in a new background process on the saved `.blend`. Assign `result` (the `_for_cli` tool returns it).

```python
import bpy

problems = []
for ob in bpy.data.objects:
    if ob.type not in {'MESH', 'ARMATURE'}:
        continue
    if any(abs(s - 1.0) > 1e-6 for s in ob.scale):
        problems.append(f"{ob.name}: scale {tuple(ob.scale)}")
    if any(abs(r) > 1e-6 for r in ob.rotation_euler):
        problems.append(f"{ob.name}: rotation {tuple(ob.rotation_euler)}")
    if ob.type == 'MESH':
        if not all(c.isalnum() or c == '_' for c in ob.name):
            problems.append(f"{ob.name}: name has other characters")
        if ob.name.startswith(('UCX_', 'UBX_', 'USP_', 'UCP_')):
            render = ob.name[4:].rsplit('_', 1)[0]
            if render not in bpy.data.objects:
                problems.append(f"{ob.name}: no render mesh named {render}")
for img in bpy.data.images:
    w, h = img.size
    if w and (w & (w - 1) or h & (h - 1)):
        problems.append(f"{img.name}: {w}x{h} is not power of two")
result = {"marker": "BLENDER-VERIFY", "problems": problems}
```

## Evidence

Checked on 2026-10-04. Blender facts are against the 5.2 LTS API and manual (the latest release, 5.2.2). Unreal facts are against the 5.8 documentation.

| rule | evidence |
|---|---|
| Apply Scalings values and meaning (`FBX_SCALE_NONE`, `FBX_SCALE_UNITS`, `FBX_SCALE_CUSTOM`, `FBX_SCALE_ALL`); defaults `apply_scale_options='FBX_SCALE_NONE'`, `apply_unit_scale=True` | [bpy.ops.export_scene](https://docs.blender.org/api/current/bpy.ops.export_scene.html) |
| `FBX All` with Unit Scale 1.0 avoids a root bone scaled by 100 | community reports, not Epic: [Epic forum, root bone x100](https://forums.unrealengine.com/t/root-bone-is-scaled-by-x100/484622), [Blender add-ons #51704](https://projects.blender.org/blender/blender-addons/issues/51704). The hand-off bounds check catches a wrong scale. |
| Apply Transform is experimental and broken with armatures | API text: "WARNING! experimental option, use at own risk, known to be broken with armatures/animations" ([bpy.ops.export_scene](https://docs.blender.org/api/current/bpy.ops.export_scene.html)) |
| Axis defaults `-Z` forward, `Y` up; Use Space Transform | [bpy.ops.export_scene](https://docs.blender.org/api/current/bpy.ops.export_scene.html); [Blender manual, FBX](https://docs.blender.org/manual/en/5.1/addons/import_export/scene_fbx.html) |
| Blender `+X` to Unreal `+X`, Blender `-Y` to Unreal `+Y` | not in Epic or Blender docs; from Unreal's left-handed Z-up frame. The hand-off asks the UE teammate to check it on the first import. |
| Smoothing options; "prefer Normals Only if your target importer understands custom normals" | [bpy.ops.export_scene](https://docs.blender.org/api/current/bpy.ops.export_scene.html). `FACE` is chosen because Unreal's FBX importer warns when no smoothing groups are present. |
| Tangent space "will only work correctly with tris/quads only meshes" | [bpy.ops.export_scene](https://docs.blender.org/api/current/bpy.ops.export_scene.html) |
| Unreal meshes must be triangulated; pivot at the origin; FBX 2020.2 | [Epic: FBX Static Mesh Pipeline](https://dev.epicgames.com/documentation/unreal-engine/fbx-static-mesh-pipeline-in-unreal-engine) |
| `UBX_`, `UCP_`, `USP_`, `UCX_[RenderMeshName]_##`; name identical to the render mesh; `SOCKET_` prefix | [Epic: FBX Static Mesh Pipeline](https://dev.epicgames.com/documentation/unreal-engine/fbx-static-mesh-pipeline-in-unreal-engine); [Epic Send to Unreal: static mesh](https://epicgames.github.io/BlenderTools/send2ue/asset-types/static-mesh.html) |
| `_LOD0` postfix stripped from the asset name | [Epic Send to Unreal: skeletal mesh](https://epicgames.github.io/BlenderTools/send2ue/asset-types/skeletal-mesh.html) |
| Skeletal mesh pivot is the root bone | [Epic: FBX Skeletal Mesh Pipeline](https://dev.epicgames.com/documentation/unreal-engine/fbx-skeletal-mesh-pipeline-in-unreal-engine) |
| Armature object named `Armature` removes the extra root bone | community, not Epic docs: [Epic forum, extra bone](https://forums.unrealengine.com/t/extra-bone-added-to-armature-on-import/368845), [krisredbeard tutorial](https://krisredbeard.wordpress.com/tutorials/tutorial-prevent-blender-fbx-exporter-adding-extra-root-bone/) |
| Leaf bones, bone axes, deform-only, `armature_nodetype` | [bpy.ops.export_scene](https://docs.blender.org/api/current/bpy.ops.export_scene.html) |
| `SM_`, `SK_`, `SKEL_`, `PHYS_`, `AS_`, `T_`, `M_`, `MI_` | [Epic: Recommended Asset Naming Conventions](https://dev.epicgames.com/documentation/unreal-engine/recommended-asset-naming-conventions-in-unreal-engine-projects). The texture descriptors `_BC`, `_N`, `_ORM` are a common studio convention, not Epic's list. |
| Power-of-two sizes; 8192 maximum; mips | [Epic: Texture format support and settings](https://dev.epicgames.com/documentation/unreal-engine/texture-format-support-and-settings-in-unreal-engine) |
| Unreal 5.8 is the latest release; 5.8 adds uFBX (experimental) | [Epic: UE 5.8 release notes](https://dev.epicgames.com/documentation/unreal-engine/unreal-engine-5-8-release-notes) |
| `bpy.ops.export_scene.gltf` options (`export_yup`, `export_apply`, `export_tangents`) | [bpy.ops.export_scene](https://docs.blender.org/api/current/bpy.ops.export_scene.html) |
