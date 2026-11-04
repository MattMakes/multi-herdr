---
name: blender-3d-print
description: Use when a mesh must become printable on an FDM or resin 3D printer - an STL, OBJ, PLY, 3MF, glTF or .blend that is not watertight, intersects itself, has flipped normals, is at the wrong scale, has thin walls or overhangs, or needs hollowing, drain holes, orientation or a triangle budget. Headless Blender scripts check the mesh with numeric pass rules, write a fixed copy and a JSON and Markdown report.
---

# Blender 3D print preparation

Take any mesh and make a copy that a slicer prints without repair: closed, outward-facing, no self-intersections, at its real size in millimetres, with walls a nozzle or a resin layer can build. Two scripts do the work headless and never change the input file:

- `scripts/print_check.py`: reads a file and runs every check. Exit 0 when all hard checks pass, 1 when 1 fails, 2 when the arguments or the file are bad.
- `scripts/print_fix.py`: writes a fixed copy, then loads that copy again and runs the same checks. Same exit codes.

The scripts use only `bpy`, `bmesh` and `mathutils`. They import nothing from other skills and do not need the 3D-Print Toolbox add-on. Every check and fix ran on Blender 5.1.2 (`scripts/live/blender-3d-print.sh`, results in `docs/live-checks/blender-3d-print.md`). How each check measures and what each fix changes is in `references/checks-and-fixes.md`. The report fields are in `references/report-format.md`.

## When to use

- A model comes from a game asset, a sculpt, a scan, a CAD export or a download, and it is to be printed.
- A slicer reports non-manifold edges, holes, inverted normals or a wrong size.
- A print failed: a thin wall did not print, an overhang drooped, a resin part cracked or trapped resin.
- QA needs a pass or fail record for a printable file.
- Not for: modeling the shape itself, UVs, textures or game export.

## Run

```bash
BLENDER="${BLENDER_PATH:-/Applications/Blender.app/Contents/MacOS/Blender}"
S=<this skill>/scripts
"$BLENDER" --background --factory-startup --python "$S/print_check.py" -- \
    --in part.stl --json part.check.json --md part.check.md
"$BLENDER" --background --factory-startup --python "$S/print_fix.py" -- \
    --in part.stl --out part_print.stl --json part.fix.json --md part.fix.md
```

Run headless only, with `--factory-startup`, so no user add-on or startup file changes the result. Read the exit code, then the `CHECK` and `RESULT` lines. Each `FIX` line names a change; a `WARN` line names a change that loses detail or that needs a human look.

Options of both scripts:

| option | default | meaning |
| --- | --- | --- |
| `--process fdm\|resin` | `fdm` | sets the default wall |
| `--wall-mm X` | 0.8 fdm, 0.4 resin | minimum wall and minimum gap |
| `--overhang-deg X` | 45 | downward faces steeper than this from vertical are overhangs |
| `--printer-mm XxYxZ` | `256x256x256` | build volume; set it to the target printer |
| `--units auto\|mm\|cm\|m\|in` | `auto` | size of 1 file unit: mm for STL, OBJ, PLY and 3MF; m for glTF; the scene unit scale for `.blend` |

Options of `print_fix.py` only; each is off unless given:

| option | effect |
| --- | --- |
| `--scale F` | multiply the size, for example `1000` for a model made in metres |
| `--merge-mm X` | merge distance, default 0.01 |
| `--solidify-mm X` | give an open sheet (a leaf, a sign) this thickness instead of capping its border |
| `--thicken-mm X` | move the surface outward by X/2: every wall grows by X, and the size grows by X |
| `--remesh-voxel-mm X` | voxel remesh at X mm; last resort, loses all detail under X |
| `--target-tris N` | decimate to at most N triangles |
| `--hollow-mm X` | resin: inner shell X mm inside and 2 drain holes through the bottom wall (1 when the bottom is smaller than 3 hole diameters) |
| `--drain-mm D` | drain hole diameter for `--hollow-mm`, default 2.5 |
| `--orient` | rotate so the least overhang area faces down |

## Workflow

1. **Import.** Get the file, the process (FDM or resin), the printer volume and the minimum wall from the brief. If the brief gives no printer, use the default and say so in the report.
2. **Measure.** Run `print_check.py`. Read `bbox_mm` first: a part under 1 mm, or 1000 times too large, is a unit problem, not a mesh problem. Fix the unit with `--units` or `--scale` before anything else.
3. **Check.** Read every `FAIL`. The hard checks decide the exit code. The `WARN` checks tell the slicer or the human what to expect.
4. **Fix.** Run `print_fix.py` with no options first: it runs only the safe fixes. Add a lossy option only for the check it fixes, 1 at a time, and record why.
5. **Re-check.** `print_fix.py` re-checks its own output. Run `print_check.py` on the output too when another tool changed it.
6. **Export.** Binary STL in millimetres is the default and always works. Write 3MF only when an exporter exists (see Export).
7. **Report.** Give the JSON and the Markdown report to the QA teammate or the orchestrator, with every `WARN` line and every lossy option you used.

## Checks

Hard checks set the exit code. Report-only checks never fail the run.

| check | pass rule | kind |
| --- | --- | --- |
| `manifold` | boundary edges = 0, edges with more than 2 faces = 0, non-manifold vertices = 0 | hard |
| `intersections` | crossing face pairs (BVH overlap, faces with no shared vertex) = 0 | hard |
| `normals` | faces that differ from the recalculated outward normals = 0 | hard |
| `degenerate` | faces under 1e-6 mm2 = 0, edges under 1e-4 mm = 0 | hard |
| `loose` | vertices without edges = 0, edges without faces = 0 | hard |
| `interior_faces` | faces whose every edge has more than 2 faces = 0 | hard |
| `scale` | largest side >= 1 mm, every side <= the printer volume | hard |
| `wall_thickness` | faces whose inward ray leaves the solid within `--wall-mm` = 0 | hard |
| `shells` | count of separate parts | report |
| `thin_features` | gaps narrower than `--wall-mm`, parts thinner than `--wall-mm`, through-holes | report |
| `overhang` | area in mm2 steeper than `--overhang-deg`, without the bed faces | report |
| `bed` | lowest Z = 0 and a flat contact area > 0 | report |

A model with a sharp, acute rim (a blade, an ear) fails `wall_thickness` near the rim, because the rim is thinner than the wall there. That is a true result: round or thicken the rim, or accept it in the report with the `thin_area_mm2` value.

## Fixes, in order

`print_fix.py` runs these in this order. The safe fixes always run; each changes only what is broken.

1. Apply transforms and modifiers: all visible mesh objects become 1 mesh in mm.
2. `--scale`, when given.
3. Merge by distance, dissolve degenerate faces and edges, delete loose vertices and edges, delete interior faces.
4. Fill holes, or `--solidify-mm` for an open sheet. Capping a sheet makes a shell with no volume.
5. Recalculate normals outward.
6. Boolean union (EXACT solver) of shells that intersect. A single shell that crosses itself gets a `WARN`; use the voxel remesh.
7. `--thicken-mm`, then `--remesh-voxel-mm` (it also cleans up crossings that thicken makes), then `--target-tris`.
8. `--hollow-mm` with drain holes (resin), then `--orient`.
9. Drop to bed: centre on X and Y, lowest Z = 0.

Never use voxel remesh as the first answer: it replaces every face, and lettering or fine texture under the voxel size is lost. Try the safe fixes, then fix the source model, then remesh.

## Process rules

- **FDM.** Wall at least 2 nozzle widths (0.8 mm for 0.4 mm). Overhangs over 45 degrees need supports; `--orient` reduces them. A large flat bed contact area holds the part down.
- **Resin.** Wall at least 0.4 mm for detail, 1.5 to 2.5 mm for a hollowed shell. Hollow parts larger than a few cm to save resin and reduce suction forces. A hollow part needs drain holes, at least 2, of 2 to 3.5 mm. A sealed hollow traps liquid resin and can crack. Orientation and supports for resin are usually set in the slicer; report the overhang area.

## The 3D-Print Toolbox add-on

The 3D-Print Toolbox is an extension from extensions.blender.org since Blender 4.2. It is not in a factory Blender 5.1.2. It helps a human in the viewport: it selects the faces of each failed check and has a "Make Manifold" button. Install it only when a person inspects the model interactively. The scripts in this skill do not use it, and their numbers are the record.

## Export

- STL: `print_fix.py` writes binary STL with 1 unit = 1 mm (`bpy.ops.wm.stl_export(ascii_format=False, global_scale=1.0, use_scene_unit=False)`). STL holds no unit; every slicer reads mm.
- 3MF: Blender 5.1.2 has no built-in 3MF importer or exporter. `print_fix.py --out part.3mf` uses the operator of an enabled 3MF add-on, and exits 2 with `no 3MF exporter` when there is none. To check by hand: `[o for o in dir(bpy.ops.export_mesh) + dir(bpy.ops.wm) if "3mf" in o.lower()]`. Without an exporter, give STL and let the slicer save 3MF.
- The output is a new file. `print_fix.py` exits 2 when `--out` is the input file.

## Report

`--json` writes the full record and `--md` writes a short table for a human. Both scripts write the same shape; `print_fix.py` adds `source` and the `fixes` list. The fields are in `references/report-format.md`. A QA teammate decides from `result`, `failed` and the `WARN` checks, and repeats the run with the same options to confirm.
