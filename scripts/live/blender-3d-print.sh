#!/usr/bin/env bash
# Live check, blender-3d-print skill. Builds fixture meshes inside Blender (no
# binary fixtures in git), then runs the skill's scripts/print_check.py and
# scripts/print_fix.py headless on each one and checks the exit code and the
# JSON report. Result file: docs/live-checks/blender-3d-print.md.
# Needs Blender (BLENDER_PATH, or `blender` on PATH, or /Applications/Blender.app)
# and python3. No network. Starts no model session.
set -euo pipefail
cd "$(dirname "$0")/../.."
unset ANTHROPIC_API_KEY 2>/dev/null || true

ROOT=$PWD
SCRATCH="$ROOT/.worktrees/_scratch/live-blender-3d-print"
SKILL="$ROOT/skills/blender-3d-print/scripts"
rm -rf "$SCRATCH"
mkdir -p "$SCRATCH"
FAILED=0
pass() { echo "PASS $1"; }
fail() { echo "FAIL $1: $2"; FAILED=1; }
skip() { echo "SKIP $1: $2"; }

# 1. Blender version.
BLENDER=""
for c in "${BLENDER_PATH:-}" "$(command -v blender 2>/dev/null || true)" /Applications/Blender.app/Contents/MacOS/Blender; do
  if [ -n "$c" ] && [ -x "$c" ]; then BLENDER=$c; break; fi
done
if [ -z "$BLENDER" ]; then
  skip "blender-version" "no Blender: set BLENDER_PATH"
  exit 0
fi
pass "blender-version: $("$BLENDER" --version 2>/dev/null | head -1) at $BLENDER"
run_blender() { "$BLENDER" --background --factory-startup --python "$@" 2>&1; }

# 2. Fixtures. Each is 1 STL (or .blend) in $SCRATCH/in.
cat >"$SCRATCH/fixtures.py" <<'PY'
import sys, bpy, bmesh
from mathutils import Matrix
out = sys.argv[sys.argv.index("--") + 1]
T, D = Matrix.Translation, Matrix.Diagonal

def scene(name, build, obj_scale=1.0):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bm = bmesh.new(); build(bm)
    me = bpy.data.meshes.new(name); bm.to_mesh(me)
    o = bpy.data.objects.new(name, me); o.scale = (obj_scale,) * 3
    bpy.context.scene.collection.objects.link(o)

def stl(name, build, obj_scale=1.0):
    scene(name, build, obj_scale)
    bpy.ops.wm.stl_export(filepath=f"{out}/{name}.stl", global_scale=1.0, use_scene_unit=False)

def cube(bm, size=20.0, at=(0, 0, 10)):
    bmesh.ops.create_cube(bm, size=size, matrix=T(at))

def open_box(bm):
    cube(bm)
    top = max(bm.faces, key=lambda f: f.calc_center_median().z)
    bmesh.ops.delete(bm, geom=[top], context="FACES_ONLY")

def two_cubes(bm):
    cube(bm); cube(bm, at=(8, 8, 18))

def flipped_sphere(bm):
    bmesh.ops.create_uvsphere(bm, u_segments=48, v_segments=24, radius=10, matrix=T((0, 0, 10)))
    bmesh.ops.reverse_faces(bm, faces=bm.faces)

def thin_wall(bm):
    bmesh.ops.create_cube(bm, size=1.0, matrix=T((0, 0, 10)) @ D((20, 0.3, 20, 1)))

def tee(bm):
    # A 30 x 30 x 5 mm plate on a 10 mm high stem: the plate underside overhangs.
    bmesh.ops.create_cube(bm, size=1.0, matrix=T((0, 0, 12.5)) @ D((30, 30, 5, 1)))
    bottom = min(bm.faces, key=lambda f: f.calc_center_median().z)
    bmesh.ops.inset_region(bm, faces=[bottom], thickness=10)
    new = bmesh.ops.extrude_face_region(bm, geom=[bottom])["geom"]
    bmesh.ops.translate(bm, verts=[g for g in new if isinstance(g, bmesh.types.BMVert)], vec=(0, 0, -10))
    if bottom.is_valid:
        bmesh.ops.delete(bm, geom=[bottom], context="FACES_ONLY")

def sphere(bm):
    bmesh.ops.create_uvsphere(bm, u_segments=64, v_segments=32, radius=10, matrix=T((0, 0, 10)))

stl("clean_cube", cube)
stl("open_box", open_box)
stl("two_cubes", two_cubes)
stl("flipped_sphere", flipped_sphere)
stl("thin_wall", thin_wall)
stl("tee", tee)
stl("sphere", sphere)
stl("sheet", lambda bm: bmesh.ops.create_grid(bm, x_segments=1, y_segments=1, size=10))
# A 1 m cube at object scale 0.05, built in metres: as STL it reads as 0.05 mm.
stl("metres_005", lambda bm: cube(bm, size=1.0, at=(0, 0, 0.5)), obj_scale=0.05)
# The same model saved as .blend: the scene unit (1 unit = 1 m) gives 50 mm.
scene("metres_005", lambda bm: cube(bm, size=1.0, at=(0, 0, 0.5)), obj_scale=0.05)
bpy.ops.wm.save_as_mainfile(filepath=f"{out}/metres_005.blend")
print("FIXTURES DONE")
PY
mkdir -p "$SCRATCH/in" "$SCRATCH/out"
if run_blender "$SCRATCH/fixtures.py" -- "$SCRATCH/in" | grep -q '^FIXTURES DONE'; then
  pass "fixtures: $(find "$SCRATCH/in" -type f | wc -l | tr -d ' ') files"
else
  fail "fixtures" "the fixture script did not finish"
  exit 1
fi

# json_ok <file> <python expression on r>: r is the JSON report, c maps a check name to its entry.
json_ok() {
  python3 -I -c 'import json,sys; r=json.load(open(sys.argv[1])); c={x["name"]: x for x in r["checks"]}; sys.exit(0 if eval(sys.argv[2]) else 1)' "$1" "$2"
}

# check <step> <fixture> <expected exit> <JSON condition> [extra args]
check() {
  local step=$1 in=$2 want=$3 cond=$4; shift 4
  local js="$SCRATCH/out/$step.json" got out
  out=$(run_blender "$SKILL/print_check.py" -- --in "$SCRATCH/in/$in" --json "$js" "$@"; echo "EXIT $?")
  got=$(sed -n 's/^EXIT //p' <<<"$out")
  out=$(grep -E '^(RESULT|ERROR)' <<<"$out" | head -1)
  if [ "$got" != "$want" ]; then fail "$step" "exit $got, want $want ($out)"
  elif ! json_ok "$js" "$cond"; then fail "$step" "report does not match: $cond"
  else pass "$step: exit $got, $out"; fi
}

# fix <step> <fixture> <expected exit> <JSON condition> [extra args]
fix() {
  local step=$1 in=$2 want=$3 cond=$4; shift 4
  local js="$SCRATCH/out/$step.json" got out
  out=$(run_blender "$SKILL/print_fix.py" -- --in "$SCRATCH/in/$in" --out "$SCRATCH/out/$step.stl" --json "$js" "$@"; echo "EXIT $?")
  got=$(sed -n 's/^EXIT //p' <<<"$out")
  local res; res=$(grep -E '^(RESULT|ERROR)' <<<"$out" | head -1)
  if [ "$got" != "$want" ]; then fail "$step" "exit $got, want $want ($res)"
  elif ! json_ok "$js" "$cond"; then fail "$step" "report does not match: $cond"
  else pass "$step: exit $got, $res; fixes: $(grep -E '^(FIX|WARN) ' <<<"$out" | grep -vE ': 0 |^FIX (export|apply-transforms|drop-to-bed)' | cut -d: -f1 | sed 's/^[A-Z]* //' | tr '\n' ' ')"; fi
}

# 3. Each fixture fails the check it targets, and the fix makes it pass.
check check-clean-cube clean_cube.stl 0 'r["result"]=="PASS" and not [x for x in r["checks"] if x["status"] not in ("PASS","INFO")]'
check check-open-box open_box.stl 1 'r["failed"]==["manifold"] and c["manifold"]["value"]["boundary_edges"]==4'
fix   fix-open-box open_box.stl 0 'any(f["step"]=="fill-holes" for f in r["fixes"])'
check check-two-cubes two_cubes.stl 1 '"intersections" in r["failed"] and c["shells"]["value"]["shells"]==2'
fix   fix-two-cubes two_cubes.stl 0 'r["stats"]["shells"]==1 and any(f["step"]=="union-shells" for f in r["fixes"])'
check check-flipped-sphere flipped_sphere.stl 1 'r["failed"]==["normals"] and c["normals"]["value"]["flipped_faces"]>1000'
fix   fix-flipped-sphere flipped_sphere.stl 0 'r["stats"]["volume_mm3"]>4000'
check check-thin-wall thin_wall.stl 1 'r["failed"]==["wall_thickness"] and abs(c["wall_thickness"]["value"]["min_mm"]-0.3)<0.01'
fix   fix-thin-wall thin_wall.stl 0 'abs(r["stats"]["bbox_mm"][1]-0.9)<0.01' --thicken-mm 0.6
check check-metres-scale metres_005.stl 1 '"scale" in r["failed"] and c["scale"]["value"]["below_1mm"]'
check check-metres-blend metres_005.blend 0 'abs(r["stats"]["bbox_mm"][0]-50)<0.01'
fix   fix-metres-scale metres_005.stl 0 'abs(r["stats"]["bbox_mm"][0]-50)<0.01' --scale 1000

# 4. The opt-in fixes.
check check-tee tee.stl 0 'c["overhang"]["value"]["area_mm2"]>600'
fix   fix-orient tee.stl 0 'c["overhang"]["status"]=="PASS" and c["bed"]["status"]=="PASS"' --orient
fix   fix-solidify sheet.stl 0 'abs(r["stats"]["bbox_mm"][2]-1.2)<0.01' --solidify-mm 1.2
fix   fix-decimate sphere.stl 0 'r["stats"]["tris"]<=1000' --target-tris 1000
fix   fix-hollow clean_cube.stl 0 'r["stats"]["volume_mm3"]<8000*0.6 and r["stats"]["shells"]==1' --process resin --hollow-mm 2 --drain-mm 3
fix   fix-voxel-remesh two_cubes.stl 0 'r["stats"]["shells"]==1' --remesh-voxel-mm 0.5

# 5. The fix never writes to its input.
cp "$SCRATCH/in/clean_cube.stl" "$SCRATCH/out/same.stl"
before=$(shasum "$SCRATCH/out/same.stl")
out=$(run_blender "$SKILL/print_fix.py" -- --in "$SCRATCH/out/same.stl" --out "$SCRATCH/out/same.stl"; echo "EXIT $?")
if grep -q '^EXIT 2' <<<"$out" && [ "$before" = "$(shasum "$SCRATCH/out/same.stl")" ]; then
  pass "fix-refuses-input: exit 2, input unchanged"
else
  fail "fix-refuses-input" "$(grep -E '^(ERROR|EXIT)' <<<"$out" | tr '\n' ' ')"
fi
# 6. 3MF needs no add-on: write it, check the 3 parts, read it again, compare with the STL export.
fix   export-stl sphere.stl 0 'r["stats"]["tris"]>100'
js3="$SCRATCH/out/export-3mf.json"
out=$(run_blender "$SKILL/print_fix.py" -- --in "$SCRATCH/in/sphere.stl" --out "$SCRATCH/out/sphere.3mf" --json "$js3"; echo "EXIT $?")
if ! grep -q '^EXIT 0' <<<"$out"; then
  fail "3mf-export" "$(grep -E '^(ERROR|EXIT)' <<<"$out" | tr '\n' ' ')"
else
  cmp=$(python3 -I - "$SKILL" "$SCRATCH/out/export-stl.stl" "$SCRATCH/out/sphere.3mf" <<'PY'
import struct, sys, zipfile
sys.path.insert(0, sys.argv[1])
import threemf
stl, mf = sys.argv[2], sys.argv[3]
names = set(zipfile.ZipFile(mf).namelist())
assert set(threemf.PARTS) <= names, f"parts: {sorted(names)}"
assert 'unit="millimeter"' in zipfile.ZipFile(mf).read(threemf.MODEL_PART).decode(), "unit"
data = open(stl, "rb").read()
n = struct.unpack("<I", data[80:84])[0]
welded, tris = {}, 0
lo, hi = [1e30] * 3, [-1e30] * 3
for i in range(n):
    rec = struct.unpack("<12fH", data[84 + 50 * i:134 + 50 * i])
    tris += 1
    for k in range(3):
        v = tuple(round(c, 5) for c in rec[3 + 3 * k:6 + 3 * k])
        welded.setdefault(v, len(welded))
        lo = [min(a, b) for a, b in zip(lo, v)]
        hi = [max(a, b) for a, b in zip(hi, v)]
shells = threemf.read_3mf(mf)
verts = [v for vs, _ in shells for v in vs]
t3 = sum(len(t) for _, t in shells)
lo3 = [min(v[i] for v in verts) for i in range(3)]
hi3 = [max(v[i] for v in verts) for i in range(3)]
assert t3 == tris, f"triangles {t3} vs STL {tris}"
assert len(verts) == len(welded), f"vertices {len(verts)} vs STL {len(welded)}"
for a, b in zip(lo + hi, lo3 + hi3):
    assert abs(a - b) <= 0.001, f"bbox {lo3} {hi3} vs STL {lo} {hi}"
print(f"3MF: 3 parts, {len(verts)} vertices, {t3} triangles, bbox within 0.001 mm of the STL")
PY
  ) || true
  if ! [[ $cmp == 3MF:* ]]; then
    fail "3mf-export" "compare with STL: $cmp"
  else
    pass "3mf-export: $cmp"
    cp "$SCRATCH/out/sphere.3mf" "$SCRATCH/in/sphere.3mf"
    tris=$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["stats"]["tris"])' "$SCRATCH/out/export-stl.json")
    check check-3mf-input sphere.3mf 0 "r[\"stats\"][\"tris\"]==$tris and r[\"units_source\"].startswith(\"3MF\")"
  fi
fi

rm -rf "$SCRATCH"/out/*.stl "$SCRATCH"/out/*.3mf
exit $FAILED
