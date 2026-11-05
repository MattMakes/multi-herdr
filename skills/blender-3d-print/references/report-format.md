# Report format

Both scripts print 1 line per check, then 1 result line:

```text
CHECK manifold PASS boundary_edges=0 non_manifold_edges=0 non_manifold_verts=0
CHECK wall_thickness FAIL thin_faces=4 thin_area_mm2=800.0 min_mm=0.3 wall_mm=0.8
CHECK overhang WARN area_mm2=648.53 overhang_deg=45.0
RESULT FAIL failed: wall_thickness
```

`print_fix.py` prints a `FIX <step>: <detail>` or `WARN <step>: <detail>` line for each fix before the checks, and `ERROR <reason>` with exit 2 when it cannot start.

## JSON (`--json`)

```json
{
  "tool": "print_check",
  "input": "/abs/path/part.stl",
  "blender": "5.1.2",
  "mm_per_unit": 1.0,
  "units_source": "STL numbers read as mm",
  "limits": {"process": "fdm", "wall_mm": 0.8, "overhang_deg": 45.0, "printer_mm": [256.0, 256.0, 256.0]},
  "stats": {"verts": 8, "tris": 12, "shells": 1, "bbox_mm": [20.0, 20.0, 20.0],
            "volume_mm3": 8000.0, "welded_verts": 0},
  "checks": [
    {"name": "manifold", "status": "PASS", "hard": true,
     "value": {"boundary_edges": 0, "non_manifold_edges": 0, "non_manifold_verts": 0},
     "rule": "boundary edges = 0, edges with more than 2 faces = 0, non-manifold vertices = 0"}
  ],
  "failed": [],
  "result": "PASS"
}
```

- `status` is `PASS` or `FAIL` for a hard check, and `PASS`, `WARN` or `INFO` for a report-only check.
- `failed` lists the hard checks that failed. `result` is `FAIL` when `failed` is not empty.
- `print_fix.py` writes the report of the re-check of its output, with `"tool": "print_fix"`, `"source"` (the input file) and `"fixes"`: a list of `{"step", "detail"}` in the order they ran. Its `input` is the output file, read with `--units mm`.

## Markdown (`--md`)

A title, 1 line with the result, the process, the wall, the overhang angle and the unit source, 1 line with the size, triangles, shells and volume, 1 bullet per fix, then a table with 1 row per check: `check | status | value | rule`.

## For the QA teammate

1. Read `result` and `failed`. Any hard failure blocks the print.
2. Read every `WARN`: `overhang` area says how much support the slicer adds; `thin_features` says what can fuse or break; `bed` says if the part stands flat.
3. Read `fixes` for lossy steps: `voxel-remesh`, `decimate`, `thicken`, `hollow`. Each one needs a reason in the hand-off.
4. Re-run `print_check.py` on the output with the same `--process`, `--wall-mm`, `--overhang-deg` and `--printer-mm`. The result must match.
