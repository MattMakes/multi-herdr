# How the checks measure and what the fixes change

All numbers are in millimetres. Both scripts first join every visible mesh object into 1 `bmesh`: the evaluated mesh (modifiers applied), times the object's world matrix, times the unit factor. A mirrored object (negative scale) gets its faces reversed so they still point outward. `print_check.py` then welds vertices closer than 0.00001 mm, because STL stores no shared vertices.

## Units

| input | 1 file unit is | why |
| --- | --- | --- |
| `.stl`, `.obj`, `.ply`, `.3mf` | 1 mm | these formats hold no unit, and slicers read mm |
| `.gltf`, `.glb` | 1 m | the glTF specification uses metres |
| `.blend` | `scene.unit_settings.scale_length` x 1000 mm | the scene unit; a file saved by `print_fix.py` uses 0.001, so 1 unit = 1 mm |

A model made in metres and exported to STL without a scale reads as 1000 times too small: a 50 mm part becomes 0.05 mm, and `scale` fails with `below_1mm`. Fix it with `--units m` on the check and `--scale 1000` on the fix. A model in centimetres is 10 times too small.

## Checks

- **manifold.** An edge with 1 face is a boundary (a hole). An edge with more than 2 faces is non-manifold. A vertex where 2 closed fans meet at 1 point (all its edges have 2 faces, but `BMVert.is_manifold` is false) is a non-manifold vertex. Slicers guess the inside of such a mesh, and the guess can be wrong.
- **intersections.** `BVHTree.FromBMesh(bm).overlap(itself)`, without the pairs that share a vertex. BVH overlap reports neighbour triangles at a UV-sphere pole as crossing; skipping shared-vertex pairs removes that false result. A real fold between 2 neighbour faces is not detected.
- **normals.** A copy gets `bmesh.ops.recalc_face_normals`. Each face whose normal points the other way in the copy counts. A whole inside-out mesh fails with every face. On a non-manifold mesh the recalculation is a guess; fix `manifold` first.
- **degenerate and loose.** Face area under 1e-6 mm2, edge length under 1e-4 mm, vertices without edges, edges without faces.
- **interior_faces.** A face whose every edge is shared by more than 2 faces sits inside the solid, usually a wall left between 2 joined parts.
- **shells.** Face islands joined through edges. Several shells print as separate parts in 1 job; intersecting shells are a problem only through `intersections`.
- **through_holes.** For each closed shell, V - E + F = 2 - 2g; g is the number of handles (holes through the part). Report only.
- **scale.** The axis-aligned bounding box against 1 mm and against `--printer-mm`, axis by axis. A part that fits only when rotated fails; rotate it in the slicer or with `--orient`.
- **wall_thickness.** For each face of the outward-normal copy: a ray from the face centre, 0.0001 mm inside, along the inward normal, up to `--wall-mm`. A hit on a face that points along the ray means the ray left the solid: the wall there is thinner than the limit. `thin_area_mm2` sums those faces, and `min_mm` is the thinnest distance. Long sliver triangles sample only their centre, so a check on a coarse mesh can miss a thin spot between centres; `--remesh-voxel-mm` or a denser mesh samples more.
- **thin_features.** The same ray outward: a hit on a face that points back means a gap or slot narrower than `--wall-mm`, which fuses when printed. Also counts shells whose smallest side is under `--wall-mm` (pins, hairs, specks).
- **overhang.** Faces with normal Z below `-sin(overhang_deg)`: their angle from vertical is more than `--overhang-deg`. Faces that lie on the bed are not overhangs.
- **bed.** The lowest Z, and the area of downward faces (normal Z under -0.99) whose vertices are all within 0.01 mm of it.

## Fixes

| fix | call | loses detail |
| --- | --- | --- |
| merge by distance | `bmesh.ops.remove_doubles(dist=--merge-mm)` | only at sizes under `--merge-mm` |
| dissolve degenerate | `bmesh.ops.dissolve_degenerate(dist=1e-4)` | no |
| delete loose | `bmesh.ops.delete` on edges without faces, then vertices without edges | no |
| interior faces | `bmesh.ops.delete(context="FACES_ONLY")` | no |
| fill holes | `bmesh.ops.holes_fill(sides=0)`: 1 n-gon for each boundary loop | the cap is flat |
| solidify | `bmesh.ops.solidify(thickness=--solidify-mm)` | no |
| normals | `bmesh.ops.recalc_face_normals` | no |
| union | Boolean modifier, `UNION`, `EXACT` solver, other shells as a collection operand | no, but it can leave sliver faces; they are dissolved |
| thicken | each vertex moves along its normal by X/2 x `calc_shell_factor()` | concave detail can cross itself; follow with `--remesh-voxel-mm` |
| voxel remesh | Remesh modifier, `VOXEL`, `voxel_size=X`; shells smaller than the wall are deleted after it | yes: everything under X |
| decimate | Decimate modifier, `COLLAPSE`, ratio N / current triangles | yes |
| hollow | duplicate the surface, move it inward by X x `calc_shell_factor()`, reverse it | sharp concave detail can cross the inner shell; the re-check reports it |
| drain holes | Boolean `DIFFERENCE` with 32-sided cylinders through the downward faces within X of the lowest Z, the one nearest the centre and the one farthest from it | no |
| orient | 6 axis directions and the 8 largest face normals, each turned to face down; keeps the least overhang area, then the largest bed contact | no |
| drop to bed | move so X and Y are centred and the lowest Z = 0 | no |

Hollow with a vertex offset works on rounded and boxy parts. On a part with deep concave detail, prefer a voxel remesh first (it gives an even surface), then hollow.

## What the scripts do not do

- They add no supports. The slicer adds supports from the overhang it sees.
- They do not split a part that is larger than the printer. Cut it in Blender (bisect) and add pins or keys by hand.
- They do not repair a wrong shape. A watertight mesh can still be the wrong design.
