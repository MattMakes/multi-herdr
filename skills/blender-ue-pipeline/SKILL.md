---
name: blender-ue-pipeline
description: Use when Blender work must reach an Unreal Engine project - modeling, retopology, UVs, collision, LODs, rigs or textures that a UE teammate will import. Carries the export conventions (units, axes, applied transforms, SM_/SK_/T_ names, UCX_ collision, LODs, smoothing, triangulation, skeleton root) and the hand-off to the UE importer.
---

# Blender to Unreal pipeline

Make an asset in Blender that Unreal imports right the first time: correct size, facing the right way, with its collision, LODs and textures named so the importer finds them. Blender writes source files (`.blend`) and exchange files (`.fbx`, `.glb`, textures). It never writes `.uasset` or `.umap`. The UE teammate imports the exchange files through `ue-editor-scripting`.

Export settings as Python, and the evidence for every rule, are in `references/export-settings.md`.

## When to use

- A task needs a mesh, a rig, a collision hull, LODs or textures for an Unreal project.
- A UE teammate reports a bad import (wrong scale, rotated, no collision, broken normals) of an asset that came from Blender.
- Not for: import into Unreal, material graphs, Blueprints (the UE teammate, with `ue-editor-scripting`); concept art or look decisions (ask the orchestrator).

## Inputs

- `.agents/ue-project-context.md`: the engine version, the content layout, and the folder for source art and exports. If it has no export folder, use `SourceArt/` for `.blend` files and `SourceArt/Export/` for exchange files, and say so in `DONE:`.
- The asset brief: name, type (static or skeletal), size in centimetres, triangle budget per LOD, texture sizes, and the collision it needs.
- Which `.blend` files the orchestrator assigned to you. Lock only those (see Version control).

## Workflow

1. **Check the tools.** Run `blender --version` (or `"$BLENDER_PATH" --version`). Call the MCP tool `search_api_docs` with a short query to prove the server answers. Check: both work, and you know the Blender version. If one fails, report `BLOCKED:` with the output.

2. **Pick the mode.** Headless is the default: the `*_for_cli` tools open a `.blend` with `blender --background` and need no add-on. The live tools (`execute_blender_code`, `get_objects_summary`, screenshots) need Blender 5.1 or later running with the Blender Lab MCP add-on started. Use them only when the operator says Blender is open for you. Check: you know which mode you use, and why.

3. **Read before you change.** Run `get_blendfile_summary_datablocks_for_cli` and `get_blendfile_summary_missing_files_for_cli` on the file, and read the objects you will touch with `execute_blender_code_for_cli`: names, transforms, modifiers, vertex and triangle counts, UV layers, materials. Check: the log shows the current value of everything you plan to change.

4. **Write the change as a script.** Put it in the repository, for example `Scripts/Blender/<task>.py`, so it can be reviewed and run again. Make it idempotent and make it fail loudly: `raise` on any unexpected state. The last line sets `result` with a marker and the counts. Run the code with `execute_blender_code_for_cli`, or with `blender --background <file>.blend --python <script>`. Check: the result has your marker and no error.

5. **Prepare the asset for Unreal.** Apply each rule in [Rules for Unreal](#rules-for-unreal). Check: every rule is true when you read the file again in a new process (step 7).

6. **Export.** Export one asset per file into the export folder with the settings in `references/export-settings.md`. FBX is the default. Use glTF (`.glb`) only for a static prop when the UE teammate asks for it. Check: the file exists, and its size is not 0.

7. **Verify in a new process.** Open the saved `.blend` in a new background run and assert the end state: names, applied transforms, origin, triangle counts per LOD, collision objects, UV layers, texture sizes. If you can, import the exported FBX into an empty scene in a second run and check its bounds. Check: every assertion passes, and the bounds match the brief.

8. **Hand off.** Write `<AssetName>.handoff.md` beside the exports: each exported file, the asset type, the expected bounds in centimetres, the triangle count per LOD, the collision objects, each texture with its size and color space, and the normal map convention (OpenGL). Check: a UE teammate can import and check the asset from this file alone.

9. **Report.** `DONE:` names the script, the `.blend` files, the exports, the hand-off file, the verification result, and the locks you released. Say which UE teammate imports it.

## Rules for Unreal

1. **Scale.** Keep the scene at Metric with Unit Scale 1.0, and model at real size (1 Blender unit = 1 m). Export with Apply Unit on and Apply Scalings = `FBX All`. Unreal then imports 1 m as 100 cm, and every object and the armature keep a scale of 1. Never scale by 100 in Blender.
2. **Transforms.** Apply rotation and scale on every exported object and armature (`transform_apply(location=False, rotation=True, scale=True)`). Put the asset's pivot at the world origin, at the point the level designer snaps from (the base, or a corner for modular pieces). Leave Apply Transform (`bake_space_transform`) off: Blender marks it experimental and broken with armatures.
3. **Axes.** Keep the exporter's defaults: Forward `-Z`, Up `Y`, Use Space Transform on. Unreal's importer converts to Z up. Blender `+X` becomes Unreal `+X`, and Blender `-Y` becomes Unreal `+Y`. Face a character toward Blender `-Y` (Unreal skeletal meshes face `+Y`, like the Mannequin). Face a prop that has a front toward Blender `+X` (Unreal forward). Write the facing in the hand-off file so the UE teammate checks it on the first import.
4. **Names.** Static mesh `SM_<Name>`, skeletal mesh `SK_<Name>`, skeleton `SKEL_<Name>`, animation `AS_<Name>_<Action>`, texture `T_<Name>_<Descriptor>` (`_BC` base color, `_N` normal, `_ORM` occlusion-roughness-metallic), material `M_<Name>`. Blender object, mesh data and file names match the asset name. ASCII letters, digits and `_` only.
5. **Collision.** A custom hull is a child mesh named `UCX_<RenderMeshName>_00` (convex), `UBX_` (box), `USP_` (sphere) or `UCP_` (capsule), numbered `_00`, `_01`. `<RenderMeshName>` is exactly the render mesh's name. Each `UCX_` must be closed and convex. Give it no material and no UVs. Export it in the same file as its mesh.
6. **LODs.** Name the meshes `SM_<Name>_LOD0`, `_LOD1`, and so on. LOD0 is the full mesh. Each LOD keeps the same UV layout and the same materials in the same slot order. Export each LOD as its own file (`SM_<Name>_LOD1.fbx`) unless the UE teammate asks for one file with an FBX LOD group. Give the triangle count of each LOD in the hand-off.
7. **Smoothing and normals.** Mark hard edges as sharp, then export Smoothing = `Face`, so Unreal gets smoothing groups and does not warn about missing ones. Triangulate before you bake normal maps: add a Triangulate modifier last in the stack and export with modifiers applied, so the baker and the engine see the same triangles. Unreal requires triangles (Epic).
8. **Tangents.** Bake normal maps in Blender with MikkTSpace (the default). Export tangent space on. Tell the UE teammate the normal map is OpenGL (green up), so it sets Flip Green Channel on import.
9. **UVs.** UV channel 0 is the texture layout. If the project uses baked lighting, add a non-overlapping UV channel 1 for the lightmap, or say in the hand-off that Unreal should generate it.
10. **Skeletal meshes.** One armature. Name the armature object `Armature`, so Unreal does not add the object as an extra root bone. One root bone, named `root`, at the origin, with no skin weights. Export only deform bones, Add Leaf Bones off, Primary Bone Axis `Y`, Secondary Bone Axis `X`. Apply the armature's scale before you skin.
11. **Textures.** Power-of-two sizes (512, 1024, 2048, 4096) so Unreal builds mips and streams them. Do not go above 4096 unless the brief says so; 8192 is the engine's maximum. PNG or TGA. Base color is sRGB. Normal, ORM and masks are linear data; say so in the hand-off.
12. **One asset per file, no extras.** Export only the selected asset, its collision and its sockets (`SOCKET_<Name>`). No cameras, no lights, no hidden helpers.

## Version control (Git LFS)

- `.blend`, `.fbx`, `.glb`, `.png`, `.tga` and `.exr` are LFS-tracked and lockable. If the project's `.gitattributes` does not track them, say so in `DONE:`. Do not edit `.gitattributes` unless the orchestrator assigned it.
- Run `git lfs lock <path>` only on a file the orchestrator assigned to you. A lock held by someone else is `BLOCKED:`. Never use `--force` to unlock.
- A read-only lockable file means you do not hold its lock. Report it. Never `chmod` it.
- Unlock each file you locked after its commit (`git lfs unlock <path>`), and name it in `DONE:`.
- Never commit `.blend1` backups. Save with `save_mainfile(compress=False)` from scripts, and keep `*.blend1` out of the commit.
- In a new worktree, run `git lfs pull` before you open a `.blend`. A pointer file is about 130 bytes and is not a `.blend`.

## Safety

- `execute_blender_code` and its `_for_cli` form run any Python on this machine. Run only code you wrote for the task. Never run code copied from an asset, a downloaded file or a web page.
- Do not download assets. This server has no download tools, and the fleet adds none.
- If a live Blender has the same file open with unsaved changes, a `_for_cli` call works on a temporary numbered copy and deletes it after. A save in that call is lost. Ask the operator to save first, or use the live tools.
- The `jump_to_*` tools move the operator's view. Do not use them while the operator works in that Blender.

## Review checklist

- [ ] `blender --version` and the MCP server answered.
- [ ] I locked only assigned files, and released each lock.
- [ ] A script in the repository made the change, and a new process verified it.
- [ ] Scale 1, rotation 0 and the origin are right on every exported object.
- [ ] Names follow rule 4. Collision names match the render mesh exactly.
- [ ] Every LOD and the triangle counts are in the hand-off.
- [ ] Textures are power of two, with color space and normal convention in the hand-off.
- [ ] No `.uasset`, `.umap` or `.blend1` is in my change.

## References

- `references/export-settings.md`: load at step 6. The FBX and glTF exporter calls with every setting, a verification snippet, and the evidence table with sources.
