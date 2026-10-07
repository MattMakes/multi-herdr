---
name: blender-baking
description: Use when texture maps must be baked in Blender for a game asset - high-to-low tangent normal maps, ambient occlusion, base color and roughness bakes, ORM channel packing - with Cycles, cage extrusion and ray distance, padding, color spaces, overlapping and mirrored UVs, and a pixel-level check of the saved files. Pairs with blender-ue-pipeline (T_ names, OpenGL normals, power-of-two sizes).
---

# Blender texture baking

Transfer detail from a high-poly or source mesh onto the game mesh's UV layout as textures, and save them so that Unreal reads them correctly. `blender-ue-pipeline` owns the texture rules: `T_<Name>_BC`, `_N`, `_ORM` names, power-of-two sizes, sRGB for base color and linear for data, and normal maps baked with MikkTSpace in OpenGL (green up) convention. This skill owns producing those files.

Tested helper functions are in `references/bake-helpers.md`, run with Cycles on CPU against the `bpy` 5.0.1 module. On 2026-10-06 the same helpers ran headless (`--background --factory-startup`) on Blender 3.5.1, the Blender installed on the operator's Mac; the results are in `docs/live-checks/blender.md`. Confirm calls on the installed Blender with `get_python_api_docs`.

## When to use

- A low-poly mesh needs a normal map from a high-poly or sculpt.
- An asset needs AO, base color or roughness baked from materials or geometry.
- The project wants packed ORM textures.
- A bake shows artifacts: seams, waviness, missing detail, black patches, projection errors.
- Not for: material graphs in Unreal (the UE teammates); topology and UVs (`blender-modeling`).

## Before you bake

The low-poly mesh must be final, because a bake is valid only for the exact mesh it was baked on:

- Triangulated the way it will be exported (`blender-ue-pipeline` rule 7: the Triangulate modifier last; Cycles bakes the evaluated mesh, so the modifier counts).
- Smooth shading with sharp edges marked, and a UV seam on every sharp edge (`blender-modeling` step 5).
- UV channel 0 final, inside 0..1, with margins, and without unintended overlaps.
- Scale applied.

If any of these changes after the bake, bake again.

## Workflow

1. **Set up names and collections.** Low-poly: the asset name (`SM_<Name>`). High-poly: `<Name>_high`, in the `_source` collection, excluded from export. Each high and low pair overlaps in space. Check: `use_selected_to_active` will select only the intended high objects.

2. **Explode the asset when parts are close.** Where separate parts sit near each other (a lid on a box, a gun's parts), rays from one part hit its neighbour. Move each low and high pair apart by the same offset in a copy of the scene (or bake part by part), bake, then discard the moved copy. Check: no part's rays can reach another part's high mesh within the ray distance.

3. **Handle overlapping UVs.** A unique bake writes every texel once. For mirrored or repeated parts that share UV space on purpose, move the duplicate islands by exactly +1 in U before the bake, so only one copy receives the bake, and move them back after. Texture wrapping makes the shifted copy read the same pixels in the engine. Check: no two islands overlap inside 0..1 at bake time.

4. **Create the targets.** `new_bake_image(name, size, non_color, high_bit)`:
   - Normal map: Non-Color, `high_bit=True` (16-bit), to avoid banding on smooth surfaces.
   - AO, roughness, metallic, ORM: Non-Color, 8-bit.
   - Base color: sRGB, 8-bit.
   Sizes are powers of two from the brief. Check: the color space of each target matches its use.

5. **Set the projection distance.** `cage_extrusion` pushes the low mesh out along its normals before casting rays inward. Set it just large enough to enclose the high mesh everywhere: start at about 1 to 2 % of the asset's size, and measure the largest gap between low and high. `max_ray_distance` limits how far a ray travels; set it a little larger than the extrusion to stop rays hitting far geometry (0 means no limit). For hard-surface assets with large flat faces and hard edges, an explicit cage object (`cage_ob`) that is the low mesh with averaged normals removes the gaps at hard edges. Check: a test bake at a small size shows no missing detail (rays too short) and no projected neighbours (rays too long).

6. **Bake the normal map.** `bake(low, [high], 'NORMAL', img, size_px, extrusion, max_ray)`. The helper sets tangent space with the OpenGL (`+X`, `+Y`, `+Z`) swizzle and a margin of about size/128 px (16 px at 2048) with the Extend type, so mip levels do not bleed seam colors. Check: `normal_map_stats(img)` returns about (0.5, 0.5, 1.0) on average with a blue channel well above 0.5. A mean far from that means the low's normals, the projection or the color space are wrong.

7. **Bake AO.** `bake(low, [high], 'AO', ...)` from the high mesh, or `bake(low, [], 'AO', ...)` from the low mesh alone. Use enough samples to remove noise (start at 64 for a final bake); AO noise compresses badly. Ground-contact AO from a floor belongs in the engine, not in the texture: do not include a floor plane.

8. **Bake roughness, metallic and base color from materials.** Roughness: `'ROUGHNESS'`. Base color: `'DIFFUSE'` with color only (the helper turns off the direct and indirect passes, so lighting is not baked in). Metallic has no bake type in Cycles; route the metallic value into an Emission shader on a copy of the material, and bake `'EMIT'`. Check: base color contains no lighting or shadow.

9. **Pack the ORM.** `pack_orm(ao, rough, metal, name, size)` writes R = occlusion, G = roughness, B = metallic into one Non-Color image. Pass `None` for metal on a fully dielectric asset. Check: the ORM image is Non-Color.

10. **Save and check the files.** `save_png(img, path, bits)` writes PNG. It switches the scene to the Standard view transform during the save, because `save_render` applies the view transform (AgX by default) to float sRGB images and changes their values. Normal maps: 16-bit. Others: 8-bit. Then reload each file with `load_for_check(path, non_color)` and read its pixels: a data map loaded as sRGB reads about 0.21 where it stores 0.5, which looks like a bad bake but is a bad check. Check: the reloaded normal map's mean is about (0.5, 0.5, 1.0), every file is the brief's size, and every name follows `T_<Name>_<Descriptor>`.

11. **Hand over.** In the hand-off file, list each texture with its size, bit depth and color space, the ORM channel order, and the normal map convention (OpenGL, so Unreal sets Flip Green Channel). Name the low-poly mesh version the bake belongs to.

## Fixing artifacts

| Symptom | Likely cause | Fix |
|---|---|---|
| Wavy lines on flat faces near hard edges | Averaged normals across a hard edge with no seam | Mark the edge sharp and seam it, or use a cage |
| Black or missing areas | Rays too short, or the high mesh is outside the cage | Increase extrusion; check the high mesh is inside it |
| AO near 0 in bands where high parts meet | Coplanar faces of overlapping high meshes (a board flush with a box face): the ray hits inside the other part | Inset one of the 2 faces by 1 to 2 mm in the high mesh |
| Detail from a neighbouring part | Rays too long, parts too close | Lower max ray distance; explode the bake |
| Visible seam lines | Too little padding, or a seam on a smooth edge | Raise the margin; move seams onto sharp edges |
| Stepping on curved surfaces | 8-bit normal map | Bake to a 16-bit float target |
| Inverted lighting on one side | Mirrored islands baked twice | Shift duplicates +1 U (step 3) |
| Normal map lit from below in Unreal | Green channel convention | OpenGL map with Flip Green Channel on import |

## Review checklist

- [ ] The low mesh is final: triangulated as exported, seams on sharp edges, UVs final, scale applied.
- [ ] High meshes are in `_source`, excluded from export.
- [ ] No overlapping islands at bake time; duplicates shifted +1 U and restored.
- [ ] Extrusion and ray distance were tested at a small size first.
- [ ] Normal map is 16-bit, Non-Color, OpenGL, with a margin of at least size/128 px.
- [ ] Base color has no lighting; ORM is R occlusion, G roughness, B metallic, Non-Color.
- [ ] Files reloaded with the right color space and checked by pixel statistics.
- [ ] The hand-off lists every texture, its color space, and the green-channel convention.

## References

- `references/bake-helpers.md`: load at step 4. Bake target creation, the Cycles bake call with cage and margin settings, saving without a view transform, reloading for checks, ORM packing and normal map statistics, as tested Python.
