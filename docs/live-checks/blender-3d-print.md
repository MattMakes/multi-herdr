# Live check: blender-3d-print

The `blender-3d-print` skill's 2 scripts, `print_check.py` and
`print_fix.py`, run headless on the installed Blender. They share the
standard-library module `threemf.py`, which reads and writes 3MF. Each fixture fails the
check it targets, and the fix makes it pass.

## How to run

```bash
scripts/live/blender-3d-print.sh
```

It needs Blender (`BLENDER_PATH`, or `blender` on PATH, or
`/Applications/Blender.app`) and `python3`. Without Blender, every step is
`SKIP`. It uses no network and starts no model session.

The script writes only under `.worktrees/_scratch/live-blender-3d-print/`.
It builds the fixture meshes in Blender at run time, so git holds no binary
fixture. It keeps the JSON reports in `out/` and deletes the output meshes.

Fixtures (all in mm): a clean 20 mm cube; the cube without its top face; 2
overlapping cubes; an inside-out UV sphere; a 20 x 0.3 x 20 mm wall; a 1 m
cube at object scale 0.05, as STL and as `.blend`; a plate on a stem; a
UV sphere; a 20 x 20 mm open sheet.

## 2026-11-04

| step | claim it proves | tool version | result | evidence |
|---|---|---|---|---|
| blender-version | Blender is found, in `scripts/live/blender-3d-print.sh` | Blender 5.1.2 | PASS | `/Applications/Blender.app/Contents/MacOS/Blender` |
| fixtures | fixtures are built in Blender, not stored | Blender 5.1.2 | PASS | 10 files |
| check-clean-cube | a clean mesh passes every check, `SKILL.md` Checks | Blender 5.1.2 | PASS | exit 0, no WARN |
| check-open-box | `manifold` counts boundary edges | Blender 5.1.2 | PASS | exit 1, failed: manifold, 4 boundary edges |
| fix-open-box | fill holes closes the mesh | Blender 5.1.2 | PASS | exit 0, fix `fill-holes` |
| check-two-cubes | `intersections` finds crossing shells | Blender 5.1.2 | PASS | exit 1, failed: intersections, 2 shells |
| fix-two-cubes | EXACT boolean union joins the shells | Blender 5.1.2 | PASS | exit 0, 1 shell, fix `union-shells` |
| check-flipped-sphere | `normals` finds an inside-out mesh; no false `intersections` at the poles | Blender 5.1.2 | PASS | exit 1, failed: normals only, more than 1000 faces flipped |
| fix-flipped-sphere | recalculate normals | Blender 5.1.2 | PASS | exit 0, volume over 4000 mm3 |
| check-thin-wall | `wall_thickness` ray cast measures 0.3 mm against 0.8 mm | Blender 5.1.2 | PASS | exit 1, failed: wall_thickness, `min_mm` 0.3 |
| fix-thin-wall | `--thicken-mm 0.6` | Blender 5.1.2 | PASS | exit 0, wall 0.9 mm |
| check-metres-scale | `scale` flags a model in metres read as mm | Blender 5.1.2 | PASS | exit 1, failed: scale and wall_thickness, `below_1mm` true |
| check-metres-blend | `.blend` input uses the scene unit | Blender 5.1.2 | PASS | exit 0, 50 mm |
| fix-metres-scale | `--scale 1000` | Blender 5.1.2 | PASS | exit 0, 50 mm |
| check-tee | `overhang` area, report only | Blender 5.1.2 | PASS | exit 0, overhang over 600 mm2 |
| fix-orient | `--orient` turns the overhang down | Blender 5.1.2 | PASS | exit 0, overhang PASS, bed PASS |
| fix-solidify | `--solidify-mm` gives an open sheet a thickness | Blender 5.1.2 | PASS | exit 0, 1.2 mm thick |
| fix-decimate | `--target-tris 1000` | Blender 5.1.2 | PASS | exit 0, at most 1000 triangles |
| fix-hollow | `--process resin --hollow-mm 2 --drain-mm 3` | Blender 5.1.2 | PASS | exit 0, 1 shell, volume under 60 % of the cube |
| fix-voxel-remesh | `--remesh-voxel-mm 0.5` | Blender 5.1.2 | PASS | exit 0, 1 shell |
| fix-refuses-input | `print_fix.py` never writes its input | Blender 5.1.2 | PASS | exit 2, input checksum unchanged |
| export-stl | binary STL export, the reference for 3MF | Blender 5.1.2 | PASS | exit 0, 3968 triangles |
| 3mf-export | `print_fix.py --out x.3mf` needs no add-on: 3 parts, `unit="millimeter"`, vertices, triangles and bounding box match the STL within 0.001 mm, `SKILL.md` Export | Blender 5.1.2 | PASS | 3 parts, 1986 vertices, 3968 triangles |
| check-3mf-input | `print_check.py --in x.3mf` reads the 3MF with `threemf.py` | Blender 5.1.2 | PASS | exit 0, 3968 triangles, units source 3MF |
