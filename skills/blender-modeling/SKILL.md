---
name: blender-modeling
description: Use when building or fixing game-mesh geometry in Blender by script - modeling to a size and triangle budget, mesh cleanup and validation, retopology of a dense or scanned mesh, UV unwrapping and packing, texel density, lightmap UVs, LOD meshes and convex collision hulls. Pairs with blender-ue-pipeline, which owns names, axes and export.
---

# Blender modeling, topology, UVs, LODs and collision

Make the geometry right before it is exported: the right size, clean topology, a UV layout that bakes and textures cleanly, LODs that hit their budgets, and collision that is convex and cheap. `blender-ue-pipeline` owns the conventions (names, scale, axes, smoothing, export) and the workflow wrapper (tool check, locks, hand-off). This skill owns how to build what that workflow exports.

Tested helper functions for every step are in `references/mesh-helpers.md`. They were run against the `bpy` 5.0.1 module. On 2026-10-06 the same helpers ran headless (`--background --factory-startup`) on Blender 3.5.1, the Blender installed on the operator's Mac; the results are in `docs/live-checks/blender.md`. Before you rely on a call, confirm it on the installed Blender with `get_python_api_docs` or `search_api_docs`: operator arguments change between releases.

## When to use

- A brief asks for a new mesh, or for changes to an existing one.
- A mesh is too dense, has bad topology, or came from a sculpt, a scan or a boolean.
- UVs are missing, overlapping, stretched, or at the wrong texel density.
- LODs or collision hulls are missing or over budget.
- Not for: rigs and skin weights (`blender-rigging`); texture bakes (`blender-baking`); export settings (`blender-ue-pipeline`).

## Rules

1. **Script everything.** Every change is a Python script in the repository (`Scripts/Blender/<task>.py`), run with `execute_blender_code_for_cli` or `blender --background <file> --python <script>`. Interactive edits are not reviewable or repeatable. Scripts are idempotent: running twice gives the same file.
2. **Measure, do not trust the viewport.** Triangle counts come from the evaluated mesh (`evaluated_tris`), which is what the exporter writes. A quad count or the viewport statistics overlay is not the budget.
3. **Real size.** Model in metres at the brief's size, scale 1, rotation 0. Apply scale before any UV, texel density, bevel or weight work, because those read object-space sizes.
4. **Quads and triangles only.** N-gons triangulate unpredictably and break tangent space. Tri/quad meshes only, then the Triangulate modifier that `blender-ue-pipeline` rule 7 puts last.
5. **Do not destroy the source.** Keep the high-poly or source mesh in a collection named `_source` that is excluded from export. Make LODs and the low-poly from copies.

## Workflow

1. **Read the brief and the file.** Get the size in centimetres, the triangle budget per LOD, the texture size, the texel density target and the collision type. Read the current objects (`mesh_problems`, `evaluated_tris`, dimensions, modifiers, UV layers). If the brief has no texel density, measure an approved asset of the same class in the project and match it; if there is none, ask the orchestrator. Check: you have a number for every budget, or a recorded question.

2. **Build or fix the shape.** For a new mesh, build it from primitives and modifiers (Mirror, Bevel, Solidify, Array, Boolean) in a script, then apply the modifiers that are not part of the export stack. Keep silhouette edges; the silhouette is what LODs and players see. Spend triangles on the outline and on curved surfaces that catch light, not on flat areas. Check: dimensions match the brief within 1 %.

3. **Clean the topology.** Run `mesh_problems` and fix to zero: non-manifold edges, loose vertices and edges, zero-area faces and n-gons. Merge doubles with `bmesh.ops.remove_doubles` at a small distance (0.1 mm). Recalculate normals outward (`bmesh.ops.recalc_face_normals`). Boundary edges are allowed only where the mesh is meant to be open. Check: `mesh_problems` reports zeros except for intended boundaries.

4. **Retopologize when the mesh is too dense.** Pick the method by source:
   - **Hard-surface or CAD-like:** Decimate with `decimate_type='DISSOLVE'` (planar) first; it removes flat-area triangles and keeps edges. Then `COLLAPSE` if still over budget.
   - **Organic, sculpt or scan:** `bpy.ops.object.quadriflow_remesh(target_faces=...)` for an even quad mesh, or a Voxel remesh followed by Decimate. Then project details back with a Shrinkwrap modifier targeting the source, and apply it.
   - **Characters that deform:** automatic retopology rarely gives good edge loops at joints. Do an automatic pass for a starting point, then check for loops around shoulders, elbows, knees, hips, mouth and eyes. If they are missing, report it in `DONE:` as a limit and say what a manual pass needs.
   Then bake the detail from the source onto the result (`blender-baking`). Check: the evaluated triangle count is at or under the LOD0 budget.

5. **Mark seams and sharp edges together.** Every edge you mark sharp for smoothing (`blender-ue-pipeline` rule 7) also needs a UV seam, or the normal map shows a visible line along it. Add seams where the eye does not look: under, behind, inside corners, along existing hard edges. Check: every sharp edge is also a seam.

6. **Unwrap UV channel 0.** Use seams plus `bpy.ops.uv.unwrap(method='ANGLE_BASED')` for organic and important assets, or `unwrap_smart` (Smart UV Project) for hard-surface props. Then pack with a margin. The helpers pass `margin_method='FRACTION'`, so the margin is pixels divided by the texture size: 0.004 is 8 px at 2048. Use at least 8 px at 2048 (16 px at 4096) so lower mip levels do not bleed; `unwrap_smart` defaults to 0.004. Rules:
   - Every island inside 0..1 (`uv_out_of_bounds` returns 0) unless the brief asks for a tiling or trim-sheet layout.
   - No overlapping islands for a unique bake. Mirrored parts may share UV space only when the brief allows it, and then the overlapping copies move +1 in U before baking (`blender-baking` step 3).
   - Straighten islands of hard-surface parts where you can, so texture pixels line up and do not alias.
   Check: no out-of-bounds UVs, and no stretching you can see in a checker test, if you can render one.

7. **Set the texel density.** Compute it with `texel_density(ob, texture_px)` in pixels per metre. Scale islands so the asset matches the target, within about 10 %. Parts seen up close may get more; hidden parts less. Write the measured value in the hand-off. Check: the density is within the target range.

8. **Add a lightmap UV when needed.** If the project uses baked lighting, `add_lightmap_uv` makes channel 1: non-overlapping, all islands inside 0..1, with a larger margin, because lightmaps are low resolution. Otherwise say in the hand-off that Unreal should generate it. Check: channel 0 is still the active and render UV.

9. **Make the LODs.** For each LOD in the brief, `make_lod(src, level, ratio)` copies the mesh with a Decimate (collapse) modifier, and `fit_lod_ratio(lod, target_tris)` adjusts the ratio to the budget. Then check by eye or with a render that the silhouette holds, at the distance the LOD is used. Decimate keeps UVs and material slots, but check that no island collapsed to a line. If the brief gives no budgets, use about 50 % per step (LOD1 50 %, LOD2 25 %, LOD3 12.5 %) and say so. Check: each LOD is within 3 % of its budget, with the same UV channels and material slot order as LOD0.

10. **Make the collision.** Follow the brief's collision type:
    - Simple shapes: a box (`UBX_`), sphere (`USP_`) or capsule (`UCP_`) primitive, fitted to the bounds.
    - Convex: `make_convex_collision(render_ob, index, max_faces)` builds `UCX_<RenderMeshName>_<NN>` as a convex hull, with no UVs and no material, parented to the render mesh. Keep hulls small: tens of faces, not hundreds. Use `max_faces` to simplify.
    - Concave objects (an arch, a table, a U shape): split the shape into convex parts and make one hull per part (`_00`, `_01`, ...). One hull around a concave shape fills the gap and blocks players.
    Run `is_convex` on each hull. Check: every hull is convex, closed (`mesh_problems` zeros) and named after the render mesh exactly.

11. **Verify in a new process, then hand over.** Re-open the saved file and assert: dimensions, scale and rotation, triangle counts per LOD, zero topology problems, UV channels and bounds, texel density, collision names and convexity. Put the numbers in the hand-off file that `blender-ue-pipeline` step 8 writes.

## Common mistakes

- Unapplied scale: texel density, bevel widths and collision sizes are wrong by the scale factor.
- Counting quads: the budget is triangles after the Triangulate modifier.
- LODs that change material slot order: Unreal maps materials by slot, so LOD1 shows the wrong material.
- A collision hull that wraps a concave shape: the player cannot walk through the doorway.
- Removing a UV layer from `Mesh.uv_layers` on a new mesh can fail on internal layers. Build meshes without UVs in bmesh instead, as `make_convex_collision` does.
- `bmesh.ops.convex_hull` adds the hull beside the source geometry. Hull a vertex-only bmesh, or delete the source faces.
- Merging doubles across separate parts: `remove_doubles` welds a lid to its box where their corners touch, and the mesh becomes non-manifold. Leave a gap of 1 mm or more between parts, or merge each part alone.
- `bpy.ops.uv.seams_from_islands` marked no seams in a background run on Blender 3.5.1. Set `edge.use_seam` from the sharp flags in the data API, then check that the UVs split along each sharp edge.

## Review checklist

- [ ] A script in the repository made every change, and re-running it changes nothing.
- [ ] Dimensions match the brief; scale is 1, rotation 0.
- [ ] `mesh_problems` is clean; no n-gons.
- [ ] Triangle counts (evaluated) meet every LOD budget; LODs keep UV channels and slot order.
- [ ] UV channel 0 is inside 0..1 with margins, without unintended overlaps; texel density is measured and on target.
- [ ] Sharp edges are UV seams.
- [ ] Collision hulls are convex, closed, named `UCX_<RenderMeshName>_NN`, with no UVs or material.
- [ ] The source mesh is kept in `_source`, excluded from export.

## References

- `references/mesh-helpers.md`: load at step 1. Triangle counting, topology checks, LOD creation and fitting, UV unwrap and pack, texel density, lightmap UVs, convex collision and the convexity check, as tested Python.
