#!/usr/bin/env bash
# Live check, Blender family (U-15, U-20, U-51, U-55). Re-runs every helper of
# the blender-modeling, blender-rigging and blender-baking skills headless on
# the installed Blender, starts the pinned Blender Lab MCP server as
# teammates/blender-artist.md configures it, builds 1 crate asset end to end
# with the 4 Blender skills, and checks the MCP dependency pin.
# Result file: docs/live-checks/blender.md. Needs Blender (BLENDER_PATH, or
# `blender` on PATH, or /Applications/Blender.app), uv and network for the
# first uvx download. Starts no model session.
# Env: LIVE_BLENDER_SKIP_MCP=1 skips the 2 server steps (no uvx launch).
#      LIVE_BLENDER_CLEAN=1 also deletes the built asset at the end.
set -euo pipefail
cd "$(dirname "$0")/../.."
unset ANTHROPIC_API_KEY 2>/dev/null || true

ROOT=$PWD
SCRATCH="$ROOT/.worktrees/_scratch/live-blender"
mkdir -p "$SCRATCH"
FAILED=0
pass() { echo "PASS $1"; }
fail() { echo "FAIL $1: $2"; FAILED=1; }
skip() { echo "SKIP $1: $2"; }

# The MCP server pin. The teammate file carries it; PIN_SET is the full set
# that uv resolved for it on 2026-10-06 (Python 3.12).
MCP_COMMIT=2cea8d566dde07fbac28a61d698909d69724e853
PIN_SET="annotated-doc==0.0.5 annotated-types==0.8.0 anyio==4.15.1 attrs==26.1.0 certifi==2026.7.22 cffi==2.1.1 click==8.5.0 cryptography==50.0.2 docutils==0.23 h11==0.16.0 httpcore==1.0.9 httpx==0.28.1 httpx-sse==0.4.3 idna==3.20 jsonschema==4.26.0 jsonschema-specifications==2025.9.1 markdown-it-py==4.2.0 mcp==1.30.0 mdurl==0.1.2 pycparser==3.0 pydantic==2.13.5 pydantic-core==2.46.5 pydantic-settings==2.15.0 pygments==2.21.0 pyjwt==2.15.1 python-dotenv==1.2.4 python-multipart==0.0.32 pyyaml==6.0.3 referencing==0.37.0 rich==15.0.0 rpds-py==2026.9.1 shellingham==1.5.4 sse-starlette==3.5.0 starlette==1.7.0 typer==0.27.3 typing-extensions==4.16.0 typing-inspection==0.4.4 uvicorn==0.54.0"

# 1. Blender version.
BLENDER=""
for c in "${BLENDER_PATH:-}" "$(command -v blender 2>/dev/null || true)" /Applications/Blender.app/Contents/MacOS/Blender; do
  if [ -n "$c" ] && [ -x "$c" ]; then BLENDER=$c; break; fi
done
if [ -n "$BLENDER" ]; then
  pass "blender-version: $("$BLENDER" --version 2>/dev/null | head -1) at $BLENDER"
else
  skip "blender-version" "no Blender: set BLENDER_PATH"
fi
run_blender() { "$BLENDER" --background --factory-startup --python "$@" 2>&1; }

# The helpers under test are the skills' own code blocks, not copies.
extract() { awk '/^```python/{f=1;next} /^```/{f=0} f' "$1" >"$2"; }
extract skills/blender-modeling/references/mesh-helpers.md "$SCRATCH/mesh_helpers.py"
extract skills/blender-rigging/references/rig-helpers.md "$SCRATCH/rig_helpers.py"
extract skills/blender-baking/references/bake-helpers.md "$SCRATCH/bake_helpers.py"
awk '/^## Verification snippet/{v=1} v&&/^```python/{f=1;next} f&&/^```/{exit} f' \
  skills/blender-ue-pipeline/references/export-settings.md >"$SCRATCH/verify_snippet.py"

cat >"$SCRATCH/check_mesh.py" <<'PY'
import sys, os, traceback
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bpy, bmesh
res = {}
def step(name, fn):
    try:
        r = fn(); res[name] = r; print(f"HELPER PASS mesh.{name}: {r}")
    except Exception as e:
        print(f"HELPER FAIL mesh.{name}: {type(e).__name__}: {e}"); traceback.print_exc()
try:
    import mesh_helpers as H
except Exception as e:
    print(f"HELPER FAIL mesh.import: {e}"); raise SystemExit(0)
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.mesh.primitive_uv_sphere_add(segments=64, ring_count=32, radius=1.0)
sph = bpy.context.active_object; sph.name = "SM_Sphere"
step("evaluated_tris", lambda: H.evaluated_tris(sph))
step("mesh_problems", lambda: H.mesh_problems(sph))
def uv():
    H.unwrap_smart(sph); n = H.uv_out_of_bounds(sph); assert n == 0, n; return n
step("unwrap_smart+uv_out_of_bounds", uv)
step("texel_density", lambda: round(H.texel_density(sph, 1024), 1))
def lm():
    H.add_lightmap_uv(sph); assert len(sph.data.uv_layers) == 2 and sph.data.uv_layers.active_index == 0
    return H.uv_out_of_bounds(sph, 1)
step("add_lightmap_uv", lm)
def lod():
    l = H.make_lod(sph, 1, 0.5); r, n = H.fit_lod_ratio(l, 2000); assert abs(n-2000) <= 60, n; return (round(r,3), n)
step("make_lod+fit_lod_ratio", lod)
def col():
    c = H.make_convex_collision(sph, 0, max_faces=150); assert H.is_convex(c); assert len(c.data.uv_layers) == 0
    return (c.name, len(c.data.polygons))
step("make_convex_collision+is_convex", col)
def tor():
    bpy.ops.mesh.primitive_torus_add(); t = bpy.context.active_object; assert not H.is_convex(t); return "torus concave"
step("is_convex(torus)", tor)
# Regression cases from the 2026-10-06 live run (docs/live-checks/blender.md).
def data_only_unwrap():
    bpy.ops.wm.read_factory_settings(use_empty=True)   # no active object
    me = bpy.data.meshes.new("SM_DataOnly"); bmd = bmesh.new(); bmesh.ops.create_cube(bmd, size=1.0); bmd.to_mesh(me); bmd.free()
    ob = bpy.data.objects.new("SM_DataOnly", me); bpy.context.scene.collection.objects.link(ob)
    H.unwrap_smart(ob); n = H.uv_out_of_bounds(ob); assert n == 0, n; return "unwrapped with no active object"
step("unwrap_smart(no active object)", data_only_unwrap)
def chamfer_hull():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    # Two chamfered boxes stacked 1 mm apart (a crate and its lid): coplanar hull points.
    me = bpy.data.meshes.new("SM_Chamfer"); bmc = bmesh.new()
    for size, z0 in (((0.6, 0.6, 0.5), 0.0), ((0.6, 0.6, 0.08), 0.501)):
        t = bmesh.new(); bmesh.ops.create_cube(t, size=1.0)
        for v in t.verts: v.co.x *= size[0]; v.co.y *= size[1]; v.co.z = v.co.z * size[2] + z0 + size[2] / 2
        bmesh.ops.bevel(t, geom=t.edges[:], offset=0.015, segments=3, profile=0.5, affect='EDGES')
        tm = bpy.data.meshes.new("t"); t.to_mesh(tm); t.free(); bmc.from_mesh(tm); bpy.data.meshes.remove(tm)
    bmc.to_mesh(me); bmc.free()
    ob = bpy.data.objects.new("SM_Chamfer", me); bpy.context.scene.collection.objects.link(ob)
    c = H.make_convex_collision(ob, 0, max_faces=40); assert H.is_convex(c); return (c.name, len(c.data.polygons))
step("make_convex_collision(stacked chamfered boxes)", chamfer_hull)
PY
cat >"$SCRATCH/check_rig.py" <<'PY'
import sys, os, traceback
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, HERE)
import bpy
def step(name, fn):
    try:
        r = fn(); print(f"HELPER PASS rig.{name}: {r}")
    except Exception as e:
        print(f"HELPER FAIL rig.{name}: {type(e).__name__}: {e}"); traceback.print_exc()
import rig_helpers as H
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.mesh.primitive_cylinder_add(vertices=8, radius=0.1, depth=2.0, location=(0,0,1))
tube = bpy.context.active_object; tube.name = "SK_Tube"
bpy.ops.object.mode_set(mode='EDIT'); bpy.ops.mesh.select_all(action='SELECT')
bpy.ops.mesh.subdivide(number_cuts=6); bpy.ops.object.mode_set(mode='OBJECT')
bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
st = {}
def build():
    st['arm'] = H.build_armature("Tube", [("lower","root",(0,0,0),(0,0,1),True),("upper","lower",(0,0,1),(0,0,2),True)])
    return [b.name for b in st['arm'].data.bones]
step("build_armature", build)
def skin():
    H.skin_auto(tube, st['arm']); return len(tube.vertex_groups)
step("skin_auto", skin)
def wp_before():
    # force unnormalized weights on some verts
    g = tube.vertex_groups["lower"]
    for v in tube.data.vertices[:32]: g.add([v.index], 0.9, 'ADD')
    p = H.weight_problems(tube, st['arm']); assert p["not_normalized"] > 0, p; return p
step("weight_problems(before)", wp_before)
def clean():
    H.clean_weights(tube); p = H.weight_problems(tube, st['arm'])
    assert p["not_normalized"] == 0 and p["unweighted_verts"] == 0 and p["armature_modifier"], p; return p
step("clean_weights+weight_problems", clean)
def pose():
    dg = bpy.context.evaluated_depsgraph_get()
    def top():
        ev = tube.evaluated_get(bpy.context.evaluated_depsgraph_get()); me = ev.to_mesh()
        z = max([tube.matrix_world @ v.co for v in me.vertices], key=lambda c: c.z).copy(); ev.to_mesh_clear(); return z
    a = top(); restore = H.pose_test(st['arm'], "upper", 'X', 45.0)
    try: b = top()
    finally: restore()
    c = top(); assert (a-b).length > 0.1 and (a-c).length < 1e-4, (a,b,c); return round((a-b).length,3)
step("pose_test", pose)
def act():
    arm = st['arm']; arm.animation_data_create(); a = bpy.data.actions.new("AS_Tube_Bend"); arm.animation_data.action = a
    pb = arm.pose.bones["upper"]; pb.rotation_mode = 'XYZ'
    for f, d in ((1,0),(12,45),(24,0)):
        pb.rotation_euler.x = d*0.01745; pb.keyframe_insert("rotation_euler", frame=f)
    a.use_fake_user = True; r = H.actions_for_export(arm); assert r[0]["named_ok"] and r[0]["frames"] == (1,24), r; return r
step("actions_for_export", act)
def fbx():
    p = os.path.join(HERE, "rig_check.fbx")
    bpy.context.scene.frame_start, bpy.context.scene.frame_end = 1, 24
    bpy.ops.object.select_all(action='DESELECT'); tube.select_set(True); st['arm'].select_set(True)
    bpy.ops.export_scene.fbx(filepath=p, use_selection=True, add_leaf_bones=False, primary_bone_axis='Y', secondary_bone_axis='X',
        use_armature_deform_only=False, bake_anim=True, bake_anim_use_all_actions=False, bake_anim_use_nla_strips=False)
    bpy.ops.wm.read_factory_settings(use_empty=True); bpy.ops.import_scene.fbx(filepath=p)
    arms = [o for o in bpy.data.objects if o.type == 'ARMATURE']; bones = sorted(b.name for b in arms[0].data.bones)
    fr = [tuple(int(x) for x in a.frame_range) for a in bpy.data.actions]
    assert set(bones) >= {"root","lower","upper"}, bones; assert any(e-s+1 in (24,25) for s,e in fr), fr; return (bones, fr)
step("fbx_roundtrip", fbx)
PY
cat >"$SCRATCH/check_bake.py" <<'PY'
import sys, os, traceback
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, HERE)
import bpy
def step(name, fn):
    try:
        r = fn(); print(f"HELPER PASS bake.{name}: {r}")
    except Exception as e:
        print(f"HELPER FAIL bake.{name}: {type(e).__name__}: {e}"); traceback.print_exc()
import bake_helpers as H
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.mesh.primitive_uv_sphere_add(segments=16, ring_count=8, radius=1.0)
low = bpy.context.active_object; low.name = "SM_Low"
bpy.ops.object.mode_set(mode='EDIT'); bpy.ops.mesh.select_all(action='SELECT')
bpy.ops.uv.smart_project(island_margin=0.01); bpy.ops.object.mode_set(mode='OBJECT')
bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=6, radius=1.0)
high = bpy.context.active_object; high.name = "High"
tex = bpy.data.textures.new("n", 'CLOUDS'); tex.noise_scale = 0.2
d = high.modifiers.new("D", 'DISPLACE'); d.texture = tex; d.strength = 0.05
S = 128; st = {}
def nb():
    st['n'] = H.new_bake_image("T_Low_N", S, True, True); st['ao'] = H.new_bake_image("ao", S, True); st['r'] = H.new_bake_image("r", S, True)
    st['bc'] = H.new_bake_image("T_Low_BC", S, False); return st['n'].colorspace_settings.name
step("new_bake_image", nb)
def bn():
    H.bake(low, [high], 'NORMAL', st['n'], S, extrusion=0.08, samples=1)
    m = H.normal_map_stats(st['n']); assert abs(m[0]-0.5)<0.05 and abs(m[1]-0.5)<0.05 and m[2]>0.8, m; return m
step("set_bake_target+bake(NORMAL)+normal_map_stats", bn)
def covered(img):
    import numpy as np; px = np.empty(S*S*4, dtype=np.float32); img.pixels.foreach_get(px)
    return (px[0::4] + px[1::4] + px[2::4]) > 0   # texels the bake wrote; use_clear leaves the rest black
def cmean(img):
    import numpy as np; px = np.empty(S*S*4, dtype=np.float32); img.pixels.foreach_get(px); m = covered(img)
    return tuple(round(float(px[i::4][m].mean()),3) for i in range(3))
def ao():
    high.hide_render = True
    H.bake(low, [], 'AO', st['ao'], S, samples=4)
    m = cmean(st['ao']); assert m[0] > 0.8, m; return m
step("bake(AO)", ao)
def rg():
    H.bake(low, [], 'ROUGHNESS', st['r'], S, samples=1)
    m = cmean(st['r']); assert abs(m[0]-0.5) < 0.02, m; return m
step("bake(ROUGHNESS)", rg)
def bc():
    mat = low.material_slots[0].material; bsdf = mat.node_tree.nodes.get("Principled BSDF")
    bsdf.inputs["Base Color"].default_value = (0.5, 0.2, 0.1, 1)
    H.bake(low, [], 'DIFFUSE', st['bc'], S, samples=1)
    m = cmean(st['bc'])  # 8-bit sRGB stores (0.5, 0.2, 0.1) linear as (0.735, 0.484, 0.349)
    assert all(abs(a-b) < 0.02 for a, b in zip(m, (0.735, 0.484, 0.349))), m; return m
step("bake(DIFFUSE color)", bc)
def orm():
    st['orm'] = H.pack_orm(st['ao'], st['r'], None, "T_Low_ORM", S); return st['orm'].size[:]
step("pack_orm", orm)
def sv():
    out = {}
    for k, img, nc, bits in (('n', st['n'], True, 16), ('orm', st['orm'], True, 8), ('bc', st['bc'], False, 8)):
        p = os.path.join(HERE, f"bake_check_{k}.png"); before = H.normal_map_stats(img); H.save_png(img, p, bits)
        r = H.load_for_check(p, nc); after = H.normal_map_stats(r)
        assert all(abs(a-b) < 0.01 for a, b in zip(before, after)), (k, before, after); out[k] = after
    return out
step("save_png+load_for_check", sv)
PY
cat >"$SCRATCH/build_crate.py" <<'PY'
"""Build SM_Crate / SK_Crate end to end with the 4 blender skills' helpers.

Run: Blender --background --factory-startup --python build_crate.py -- <out_dir>
Prints one 'ASSET <step> <json>' line per skill step check; raises on a failed check.
"""
import bpy, bmesh, json, math, os, sys
from mathutils import Vector

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import mesh_helpers as MH
import rig_helpers as RH
import bake_helpers as BH

OUT = os.path.abspath(sys.argv[sys.argv.index("--") + 1])
os.makedirs(OUT, exist_ok=True)
TEX = 1024                       # brief: 1024 textures
BODY = (0.60, 0.60, 0.50)        # brief: 60 x 60 x 50 cm body, lid 8 cm: 60 x 60 x 58 cm
LID_H = 0.08
GAP = 0.001                      # lid sits 1 mm above the body: touching corners weld in remove_doubles
LID_Z = BODY[2] + GAP
BUDGET = {0: 1200}               # LOD0 triangle cap; LOD1 and LOD2 are 50 % and 25 % of the measured LOD0 (modeling step 9 default)
TEXEL = (360.0, 440.0)           # brief: 400 px/m (4 px/cm) +-10 %. 500 px/m does not fit: full packing of 2.9 m2 at 1024 gives 423


def log(step, **kw):
    print("ASSET", step, json.dumps(kw, default=str))


def check(cond, step, **kw):
    log(step, ok=bool(cond), **kw)
    if not cond:
        raise SystemExit(f"ASSET FAIL {step}")


def box_bm(bm, size, z0, chamfer):
    """Add a chamfered box (bottom at z0) to bm; returns its new faces."""
    sx, sy, sz = size
    tmp = bmesh.new()
    bmesh.ops.create_cube(tmp, size=1.0)
    for v in tmp.verts:
        v.co = Vector((v.co.x * sx, v.co.y * sy, v.co.z * sz + z0 + sz / 2))
    if chamfer:
        bmesh.ops.bevel(tmp, geom=tmp.edges[:], offset=chamfer, segments=3, profile=0.5, affect='EDGES')
    me = bpy.data.meshes.new("tmp"); tmp.to_mesh(me); tmp.free()
    bm.from_mesh(me); bpy.data.meshes.remove(me)


# ---- blender-ue-pipeline step 1-2: tools and mode ----
bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
scene.unit_settings.system = 'METRIC'; scene.unit_settings.scale_length = 1.0
scene.render.fps = 30
log("pipeline.1-2 tools+mode", blender=bpy.app.version_string, mode="headless --background")

# ---- blender-modeling step 2: build the shape (low poly, LOD0) ----
bm = bmesh.new()
box_bm(bm, BODY, 0.0, 0.015)
box_bm(bm, (BODY[0], BODY[1], LID_H), LID_Z, 0.015)
me = bpy.data.meshes.new("SM_Crate_LOD0"); bm.to_mesh(me); bm.free()
low = bpy.data.objects.new("SM_Crate_LOD0", me)
scene.collection.objects.link(low)
dims = tuple(round(d, 4) for d in low.dimensions)
check(all(abs(a - b) <= 0.01 * b for a, b in zip(dims, (0.6, 0.6, 0.581))), "modeling.2 shape", dims_m=dims)

# ---- modeling step 3: clean topology ----
bm = bmesh.new(); bm.from_mesh(me)
bmesh.ops.remove_doubles(bm, verts=bm.verts[:], dist=0.0001)
bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
bm.to_mesh(me); bm.free()
p = MH.mesh_problems(low)
check(p["non_manifold_edges"] == p["boundary_edges"] == p["loose_verts"] == p["zero_area_faces"] == p["ngons"] == 0,
      "modeling.3 topology", **p)

# ---- modeling step 5-6: sharp edges = seams, UV channel 0 ----
for poly in me.polygons:
    poly.use_smooth = True
me.use_auto_smooth = True; me.auto_smooth_angle = math.radians(180)
bm = bmesh.new(); bm.from_mesh(me)
for e in bm.edges:
    if e.is_manifold and e.calc_face_angle() > math.radians(40):
        e.smooth = False                         # marked sharp
bm.to_mesh(me); bm.free()
# Smart UV Project at 40 deg splits islands exactly at the >40 deg (sharp) edges.
MH.unwrap_smart(low, angle_deg=40.0, margin=8 / TEX)
# Seam = sharp (modeling step 5). uv.seams_from_islands marks nothing in a 3.5.1
# background run, so mark the seams from the sharp flags, then prove the UVs split there.
for e in me.edges:
    e.use_seam = e.use_edge_sharp
bm = bmesh.new(); bm.from_mesh(me); uvl = bm.loops.layers.uv[0]
def uv_split(e):
    if len(e.link_loops) != 2: return True
    a, b = e.link_loops
    # loop a runs v1->v2 on one face; on the other face the same vertex is b.link_loop_next
    return (a[uvl].uv - b.link_loop_next[uvl].uv).length > 1e-5 or (a.link_loop_next[uvl].uv - b[uvl].uv).length > 1e-5
sharp_not_seam = sum(1 for e in bm.edges if not e.smooth and not uv_split(e))
bm.free()
check(sharp_not_seam == 0 and MH.uv_out_of_bounds(low) == 0, "modeling.5-6 seams+uv0",
      sharp=sum(e.use_edge_sharp for e in me.edges), sharp_not_seam=sharp_not_seam,
      uv_oob=MH.uv_out_of_bounds(low), margin_px=8)

# ---- modeling step 7: texel density ----
td = MH.texel_density(low, TEX)
check(TEXEL[0] <= td <= TEXEL[1], "modeling.7 texel_density", px_per_m=round(td, 1), target=TEXEL)

# ---- modeling step 8: lightmap UV ----
MH.add_lightmap_uv(low)
check(len(me.uv_layers) == 2 and me.uv_layers.active_index == 0 and MH.uv_out_of_bounds(low, 1) == 0,
      "modeling.8 lightmap_uv", layers=[l.name for l in me.uv_layers])

# ---- pipeline rule 7: Triangulate modifier last ----
tri = low.modifiers.new("Triangulate", 'TRIANGULATE'); tri.keep_custom_normals = True
mat = bpy.data.materials.new("M_Crate"); mat.use_nodes = True
mat.node_tree.nodes["Principled BSDF"].inputs["Roughness"].default_value = 0.7
me.materials.append(mat)
lod0_tris = MH.evaluated_tris(low)
check(lod0_tris <= BUDGET[0], "modeling.4 lod0_budget", tris=lod0_tris, budget=BUDGET[0])
BUDGET[1], BUDGET[2] = lod0_tris // 2, lod0_tris // 4

# ---- source collection: the high-poly mesh for the bake (modeling rule 5) ----
src = bpy.data.collections.new("_source"); scene.collection.children.link(src)
hb = bmesh.new()
# Body inset 2 mm in Z, so no board face is coplanar with a body face (coplanar overlap bakes AO = 0).
box_bm(hb, (BODY[0], BODY[1], BODY[2] - 0.004), 0.002, 0.0)
box_bm(hb, (BODY[0], BODY[1], LID_H), LID_Z, 0.0)
# Crate framing: 6 cm boards on every body edge and a cross brace, 1 cm proud.
W, D = 0.06, 0.01
sx, sy, sz = BODY
for x in (-1, 1):
    for y in (-1, 1):
        box_bm(hb, (W, W, sz), 0.0, 0.0)
        for v in hb.verts[-8:]:
            v.co.x += x * (sx / 2 - W / 2 + D / 2); v.co.y += y * (sy / 2 - W / 2 + D / 2)
for z in (0.0, sz - W):
    for axis in (0, 1):
        for sgn in (-1, 1):
            size = (sx + 2 * D, W, W) if axis == 0 else (W, sy + 2 * D, W)
            box_bm(hb, size, z, 0.0)
            for v in hb.verts[-8:]:
                if axis == 0: v.co.y += sgn * (sy / 2 - W / 2 + D / 2)
                else: v.co.x += sgn * (sx / 2 - W / 2 + D / 2)
hme = bpy.data.meshes.new("Crate_high"); hb.to_mesh(hme); hb.free()
high = bpy.data.objects.new("Crate_high", hme); src.objects.link(high)
bev = high.modifiers.new("Bevel", 'BEVEL'); bev.width = 0.006; bev.segments = 3; bev.limit_method = 'ANGLE'
for poly in hme.polygons: poly.use_smooth = True
hme.use_auto_smooth = True; hme.auto_smooth_angle = math.radians(30)
log("modeling.rule5 source", high=high.name, high_tris=MH.evaluated_tris(high), collection="_source")

# ---- modeling step 9: LODs ----
lods = {0: low}
for lvl, ratio in ((1, 0.5), (2, 0.25)):
    lod = MH.make_lod(low, lvl, ratio)
    r, n = MH.fit_lod_ratio(lod, BUDGET[lvl], tol=0.03)
    lods[lvl] = lod
    slots_ok = [s.material for s in lod.material_slots] == [s.material for s in low.material_slots]
    log("modeling.9 lod", level=lvl, ratio=round(r, 4), tris=n, budget=BUDGET[lvl], slots_ok=slots_ok,
        uv_layers=len(lod.data.uv_layers), last_modifier=lod.modifiers[-1].type)
    check(abs(n - BUDGET[lvl]) <= BUDGET[lvl] * 0.03 and slots_ok and len(lod.data.uv_layers) == 2, f"modeling.9 lod{lvl}_check", tris=n)

# ---- modeling step 10: collision (crate + lid is one convex box) ----
ucx = MH.make_convex_collision(low, 0, max_faces=40)
pc = MH.mesh_problems(ucx)
check(MH.is_convex(ucx) and pc["non_manifold_edges"] == pc["boundary_edges"] == 0 and not ucx.data.uv_layers
      and not ucx.data.materials and ucx.name == "UCX_SM_Crate_LOD0_00", "modeling.10 collision",
      name=ucx.name, faces=len(ucx.data.polygons))

# ---- baking: before-you-bake + steps 1-10 ----
# Step 2 explode: the lid sits on the body, so bake on a copy with the lid pair moved +1 m in Z.
def explode_copy(ob, name, zsplit, coll):
    c = ob.copy(); c.data = ob.data.copy(); c.name = name
    coll.objects.link(c)
    for v in c.data.vertices:
        if v.co.z > zsplit - 0.0005:
            v.co.z += 1.0
    return c
bake_coll = bpy.data.collections.new("_bake_tmp"); scene.collection.children.link(bake_coll)
low_x = explode_copy(low, "bake_low", LID_Z, bake_coll)
high_x = explode_copy(high, "bake_high", LID_Z, bake_coll)
low.hide_render = high.hide_render = True
for o in (lods[1], lods[2], ucx): o.hide_render = True
low_x.data.materials.clear(); low_x.data.materials.append(mat)
check(all(m.type != 'DECIMATE' for m in low_x.modifiers) and low_x.modifiers[-1].type == 'TRIANGULATE',
      "baking.before triangulated+uv final", modifiers=[m.type for m in low_x.modifiers])

img_n = BH.new_bake_image("T_Crate_N", TEX, non_color=True, high_bit=True)
img_ao = BH.new_bake_image("T_Crate_AO_tmp", TEX, non_color=True)
img_r = BH.new_bake_image("T_Crate_R_tmp", TEX, non_color=True)
check(img_n.colorspace_settings.name == img_ao.colorspace_settings.name == 'Non-Color', "baking.4 targets",
      normal="Non-Color 16-bit", ao="Non-Color 8-bit", size=TEX)

EXT, RAY = 0.02, 0.03   # step 5: boards are 1 cm proud + 6 mm bevel; 2 cm encloses them
BH.bake(low_x, [high_x], 'NORMAL', img_n, TEX, extrusion=EXT, max_ray=RAY, samples=1)
ns = BH.normal_map_stats(img_n)
check(abs(ns[0] - 0.5) < 0.05 and abs(ns[1] - 0.5) < 0.05 and ns[2] > 0.75, "baking.6 normal", mean=ns, extrusion=EXT, max_ray=RAY)

import numpy as np
def written(img):
    px = np.empty(TEX * TEX * 4, dtype=np.float32); img.pixels.foreach_get(px)
    return px
pxn = written(img_n); r, g = pxn[0::4], pxn[1::4]
cover = pxn[2::4] > 0
detail = float(np.std(r[cover])) + float(np.std(g[cover]))
check(detail > 0.01, "baking.6 normal detail", std_rg=round(detail, 4))

BH.bake(low_x, [high_x], 'AO', img_ao, TEX, extrusion=EXT, max_ray=RAY, samples=64)
BH.bake(low_x, [], 'ROUGHNESS', img_r, TEX, samples=1)
wrote = written(img_r)[0::4] > 0          # every texel inside the islands and their margin
ao = written(img_ao)[0::4][wrote]
pct = [round(float(np.percentile(ao, q)), 3) for q in (5, 25, 50, 75, 95)]
check(0.3 < float(ao.mean()) <= 1.0 and pct[0] < pct[-1], "baking.7 ao", mean=round(float(ao.mean()), 3),
      percentiles_5_25_50_75_95=pct, samples=64)
rg = written(img_r)[0::4][wrote]
check(abs(float(rg.mean()) - 0.7) < 0.02, "baking.8 roughness", mean=round(float(rg.mean()), 3))
orm = BH.pack_orm(img_ao, img_r, None, "T_Crate_ORM", TEX)
check(orm.colorspace_settings.name == 'Non-Color', "baking.9 orm", channels="R=AO G=roughness B=metallic(0)")

# Discard the explode copy and its temporary images; restore render visibility.
for o in list(bake_coll.objects): bpy.data.objects.remove(o)
bpy.data.collections.remove(bake_coll)
low.hide_render = False
for o in (lods[1], lods[2], ucx): o.hide_render = False
nodes = mat.node_tree.nodes
if nodes.get("BakeTarget"): nodes.remove(nodes["BakeTarget"])   # keep it out of the exported material

files = {}
for img, nc, bits in ((img_n, True, 16), (orm, True, 8)):
    path = os.path.join(OUT, img.name + ".png"); BH.save_png(img, path, bits)
    back = BH.load_for_check(path, nc)
    files[img.name] = dict(path=path, size=tuple(back.size), bits=bits, mean=BH.normal_map_stats(back))
bpy.data.images.remove(img_ao); bpy.data.images.remove(img_r)
nb = files["T_Crate_N"]
check(nb["size"] == (TEX, TEX) and abs(nb["mean"][2] - ns[2]) < 0.01 and files["T_Crate_ORM"]["size"] == (TEX, TEX),
      "baking.10 saved+reloaded", **{k: (v["size"], v["bits"], v["mean"]) for k, v in files.items()})

# ---- rigging: SK_Crate, a copy of LOD0 with a lid hinge ----
sk_me = low.data.copy(); sk_me.name = "SK_Crate"
sk = bpy.data.objects.new("SK_Crate", sk_me); scene.collection.objects.link(sk)
tri_sk = sk.modifiers.new("Triangulate", 'TRIANGULATE'); tri_sk.keep_custom_normals = True
# Hinge on the back top edge: front faces Blender +X (pipeline rule 3), so the back is -X.
HINGE = (-BODY[0] / 2, 0.0, BODY[2])
arm = RH.build_armature("Crate", [
    ("body", "root", (0, 0, 0), (0, 0, BODY[2]), True),
    ("lid", "body", HINGE, (HINGE[0], 0.3, HINGE[2]), True),   # along +Y: rotate about local Y opens it
])
check([b.name for b in arm.data.bones] == ["root", "body", "lid"] and arm.name == "Armature", "rigging.2 armature",
      bones=[b.name for b in arm.data.bones])
RH.skin_auto(sk, arm)
RH.clean_weights(sk, max_influences=4)
wp = RH.weight_problems(sk, arm)
# A crate is rigid: the lid island must follow 'lid' only, the body island 'body' only.
def mixed(ob):
    gi = {g.name: g.index for g in ob.vertex_groups}; bad = 0
    for v in ob.data.vertices:
        want = gi["lid"] if v.co.z > LID_Z - 0.0005 else gi["body"]
        ws = {g.group: g.weight for g in v.groups if g.weight > 1e-4}
        if abs(ws.get(want, 0.0) - 1.0) > 1e-3: bad += 1
    return bad
auto_mixed = mixed(sk)
log("rigging.3-4 skin_auto+clean", problems=wp, rigid_violations_after_auto=auto_mixed)
if auto_mixed:
    # rigging step 5: fix weights in the script by region, not by hand.
    gb, gl = sk.vertex_groups["body"], sk.vertex_groups["lid"]
    allv = [v.index for v in sk_me.vertices]
    gb.remove(allv); gl.remove(allv)
    lid_v = [v.index for v in sk_me.vertices if v.co.z > LID_Z - 0.0005]
    gl.add(lid_v, 1.0, 'REPLACE'); gb.add([i for i in allv if i not in set(lid_v)], 1.0, 'REPLACE')
wp = RH.weight_problems(sk, arm)
check(all(wp[k] == 0 for k in ("unweighted_verts", "not_normalized", "over_influence", "weights_on_root"))
      and not wp["groups_without_bone"] and not wp["deform_bones_without_group"] and wp["armature_modifier"]
      and mixed(sk) == 0, "rigging.4 weights", **wp)

FRONT = [v.index for v in sk_me.vertices if v.co.x > 0.25 and v.co.z > LID_Z]   # rest-pose lid front
def lid_front_z():
    ev = sk.evaluated_get(bpy.context.evaluated_depsgraph_get()); m = ev.to_mesh()
    z = max((sk.matrix_world @ m.vertices[i].co).z for i in FRONT); ev.to_mesh_clear(); return z
z0 = lid_front_z(); restore = RH.pose_test(arm, "lid", 'Y', -60.0)
try: z1 = lid_front_z()
finally: restore()
z2 = lid_front_z()
check(z1 > z0 + 0.3 and abs(z2 - z0) < 1e-5, "rigging.5 pose_test", rest_z=round(z0, 3), posed_z=round(z1, 3), restored=round(z2, 3))

arm.animation_data_create()
act = bpy.data.actions.new("AS_Crate_LidOpen"); act.use_fake_user = True; arm.animation_data.action = act
pb = arm.pose.bones["lid"]; pb.rotation_mode = 'XYZ'
for f, deg in ((1, 0.0), (20, -100.0), (30, -95.0)):
    pb.rotation_euler = (0.0, math.radians(deg), 0.0); pb.keyframe_insert("rotation_euler", frame=f)
pb.rotation_euler = (0, 0, 0)
acts = RH.actions_for_export(arm)
check(len(acts) == 1 and acts[0]["named_ok"] and acts[0]["frames"] == (1, 30) and acts[0]["fake_user"], "rigging.7 action", actions=acts)

# ---- pipeline step 5: prepare (transforms, names) and save ----
for ob in [low, lods[1], lods[2], sk, arm, high]:
    check(tuple(ob.scale) == (1, 1, 1) and tuple(ob.rotation_euler) == (0, 0, 0) and tuple(ob.location) == (0, 0, 0),
          "pipeline.5 transforms", ob=ob.name)
blend = os.path.join(OUT, "SM_Crate.blend")
bpy.context.preferences.filepaths.save_version = 0   # no .blend1 backup (pipeline: never commit .blend1)
bpy.ops.wm.save_mainfile(filepath=blend, compress=False)

# ---- pipeline step 6: export, one asset per file ----
COMMON = dict(apply_unit_scale=True, apply_scale_options='FBX_SCALE_ALL', global_scale=1.0, axis_forward='-Z',
              axis_up='Y', use_space_transform=True, bake_space_transform=False, use_mesh_modifiers=True,
              mesh_smooth_type='FACE', use_tspace=True, use_triangles=True, use_custom_props=False,
              path_mode='AUTO', embed_textures=False, use_selection=True)
def select(*obs):
    bpy.ops.object.select_all(action='DESELECT')
    for o in obs: o.select_set(True)
    bpy.context.view_layer.objects.active = obs[0]
exports = {}
def fbx(name, obs, **kw):
    path = os.path.join(OUT, name + ".fbx"); select(*obs)
    bpy.ops.export_scene.fbx(filepath=path, **{**COMMON, **kw}); exports[name] = os.path.getsize(path)
fbx("SM_Crate_LOD0", [low, ucx], object_types={'MESH', 'EMPTY'})
fbx("SM_Crate_LOD1", [lods[1]], object_types={'MESH', 'EMPTY'})
fbx("SM_Crate_LOD2", [lods[2]], object_types={'MESH', 'EMPTY'})
SK = dict(object_types={'ARMATURE', 'MESH'}, add_leaf_bones=False, primary_bone_axis='Y', secondary_bone_axis='X',
          use_armature_deform_only=True, armature_nodetype='NULL', bake_anim_use_all_actions=False,
          bake_anim_use_nla_strips=False)
arm.animation_data.action = None
fbx("SK_Crate", [arm, sk], bake_anim=False, **SK)
arm.animation_data.action = act
scene.frame_start, scene.frame_end = 1, 30
fbx("AS_Crate_LidOpen", [arm, sk], bake_anim=True, bake_anim_simplify_factor=0.0, **SK)
check(all(s > 0 for s in exports.values()) and len(exports) == 5, "pipeline.6 export", files=exports)
# ---- pipeline step 8: the hand-off file beside the exports ----
tris = {k: MH.evaluated_tris(v) for k, v in lods.items()}
open(os.path.join(OUT, "SM_Crate.handoff.md"), "w").write(f"""# SM_Crate hand-off

Made with Blender {bpy.app.version_string} by build_crate.py (scripts/live/blender.sh). Import with ue-editor-scripting.

| file | type | notes |
|---|---|---|
| SM_Crate_LOD0.fbx | static mesh LOD0 | {tris[0]} tris; collision UCX_SM_Crate_LOD0_00 ({len(ucx.data.polygons)} faces, convex) |
| SM_Crate_LOD1.fbx | static mesh LOD1 | {tris[1]} tris |
| SM_Crate_LOD2.fbx | static mesh LOD2 | {tris[2]} tris |
| SK_Crate.fbx | skeletal mesh | skeleton SKEL_Crate: root, body, lid (3 bones, 1 influence per vertex); no morph targets |
| AS_Crate_LidOpen.fbx | animation | frames 1-30 at 30 fps, no loop, no root motion |
| T_Crate_N.png | normal | {TEX}x{TEX}, 16-bit, Non-Color, OpenGL (green up): set Flip Green Channel |
| T_Crate_ORM.png | ORM | {TEX}x{TEX}, 8-bit, Non-Color: R occlusion, G roughness (0.7), B metallic (0) |

- Bounds: 60 x 60 x 58.1 cm (X x Y x Z). Pivot at the base centre. Front faces Blender +X (Unreal +X).
- UV channel 0: texture, {td:.0f} px/m at {TEX} ({td / 100:.1f} px/cm). UV channel 1: lightmap, non-overlapping.
- Material slot 0: M_Crate on every LOD. Smoothing: Face. Triangulated on export.
- The bake belongs to the LOD0 mesh in SM_Crate.blend of this run.
""")
print("ASSET DONE", json.dumps({"blend": blend, "exports": list(exports), "tris": {k: MH.evaluated_tris(v) for k, v in lods.items()},
                                "texel_px_per_m": round(td, 1), "ucx_faces": len(ucx.data.polygons)}))
PY
cat >"$SCRATCH/verify_crate.py" <<'PY'
"""Pipeline step 7: verify SM_Crate.blend and the FBX exports in a new process."""
import bpy, json, os, sys
OUT = os.path.abspath(sys.argv[sys.argv.index("--") + 1]); HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE); import mesh_helpers as MH, rig_helpers as RH
fails = []
def check(cond, step, **kw):
    print("VERIFY", step, json.dumps({"ok": bool(cond), **kw}, default=str))
    if not cond: fails.append(step)
bpy.ops.wm.open_mainfile(filepath=os.path.join(OUT, "SM_Crate.blend"))
ns = {}; exec(open(os.path.join(HERE, "verify_snippet.py")).read(), ns)
check(not ns["result"]["problems"], "export-settings snippet", **ns["result"])
o = bpy.data.objects
check([MH.evaluated_tris(o[f"SM_Crate_LOD{i}"]) for i in range(3)] == [376, 188, 94], "tris per LOD",
      tris=[MH.evaluated_tris(o[f"SM_Crate_LOD{i}"]) for i in range(3)])
check(MH.is_convex(o["UCX_SM_Crate_LOD0_00"]) and o["UCX_SM_Crate_LOD0_00"].parent == o["SM_Crate_LOD0"], "collision")
wp = RH.weight_problems(o["SK_Crate"], o["Armature"])
check(wp["unweighted_verts"] == wp["not_normalized"] == wp["weights_on_root"] == 0, "weights", **wp)
check(RH.actions_for_export(o["Armature"]) == [{"action": "AS_Crate_LidOpen", "frames": (1, 30), "named_ok": True, "fake_user": True}], "actions")
check("_source" in bpy.data.collections and "Crate_high" in bpy.data.collections["_source"].objects, "source kept")
def reimport(name):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.fbx(filepath=os.path.join(OUT, name + ".fbx"))
    return bpy.data.objects
def bounds_cm(ob):
    from mathutils import Vector
    pts = [ob.matrix_world @ Vector(c) for c in ob.bound_box]
    return [round((max(p[i] for p in pts) - min(p[i] for p in pts)) * 100, 1) for i in range(3)]
for i in range(3):
    obs = reimport(f"SM_Crate_LOD{i}"); meshes = sorted(x.name for x in obs if x.type == 'MESH')
    m = obs[f"SM_Crate_LOD{i}"]
    want = ["SM_Crate_LOD0", "UCX_SM_Crate_LOD0_00"] if i == 0 else [f"SM_Crate_LOD{i}"]
    check(meshes == want and all(abs(a - b) <= 0.5 for a, b in zip(bounds_cm(m), (60.0, 60.0, 58.1))) and MH.evaluated_tris(m) in (376, 188, 94),
          f"fbx SM_Crate_LOD{i}", objects=meshes, bounds_cm=bounds_cm(m), tris=MH.evaluated_tris(m),
          uv=[l.name for l in m.data.uv_layers])
obs = reimport("SK_Crate"); arm = next(x for x in obs if x.type == 'ARMATURE')
check(sorted(b.name for b in arm.data.bones) == ["body", "lid", "root"] and not bpy.data.actions, "fbx SK_Crate",
      bones=[b.name for b in arm.data.bones], actions=len(bpy.data.actions))
obs = reimport("AS_Crate_LidOpen"); fr = [tuple(int(x) for x in a.frame_range) for a in bpy.data.actions]
check(len(fr) == 1 and fr[0][1] - fr[0][0] + 1 in (30, 31), "fbx AS_Crate_LidOpen", frame_ranges=fr)
print("VERIFY RESULT", "PASS" if not fails else "FAIL " + ",".join(fails))
PY

# 2. U-51: every helper, headless, on this Blender. One line per helper.
if [ -n "$BLENDER" ]; then
  for kind in mesh rig bake; do
    out=$(cd "$SCRATCH" && run_blender "check_$kind.py" || true)
    lines=$(grep -E '^HELPER (PASS|FAIL) ' <<<"$out" || true)
    if [ -z "$lines" ]; then fail "helpers-$kind" "no HELPER lines; Blender output: $(tail -3 <<<"$out" | tr '\n' ' ')"; continue; fi
    while IFS= read -r l; do
      name=${l#HELPER * }; name=${name%%:*}
      case $l in
        "HELPER PASS "*) pass "helper $name" ;;
        *) fail "helper $name" "${l#*: }" ;;
      esac
    done <<<"$lines"
  done
else
  skip "helpers" "no Blender"
fi

# 3. U-20: the pinned MCP server, launched as the teammate file says.
cat >"$SCRATCH/mcp.py" <<'PY'
# MCP stdio client: launch the server exactly as the teammate's mcp_servers
# line says, list its tools, then make 3 read-only calls. Prints MCP lines.
import json, os, re, subprocess, sys
teammate, probe = sys.argv[1], sys.argv[2]
line = next(l for l in open(teammate) if re.match(r"\s+blender:\s*\{", l))
spec = json.loads(line.split(":", 1)[1])
env = dict(os.environ)
env.pop("ANTHROPIC_API_KEY", None)
env.update(spec.get("env", {}))
p = subprocess.Popen([spec["command"], *spec["args"]], stdin=subprocess.PIPE,
                     stdout=subprocess.PIPE, stderr=sys.stderr, env=env, text=True)
def send(m):
    p.stdin.write(json.dumps(m) + "\n"); p.stdin.flush()
def recv(i):
    while True:
        l = p.stdout.readline()
        if not l:
            sys.exit("server closed stdout")
        try:
            m = json.loads(l)
        except ValueError:
            continue
        if m.get("id") == i:
            return m
send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
    "protocolVersion": "2025-06-18", "capabilities": {},
    "clientInfo": {"name": "live-blender", "version": "1"}}})
info = recv(1)["result"].get("serverInfo", {})
print("MCP server %s %s" % (info.get("name"), info.get("version")))
send({"jsonrpc": "2.0", "method": "notifications/initialized"})
send({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
tools = sorted(t["name"] for t in recv(2)["result"]["tools"])
print("MCP tools %d %s" % (len(tools), " ".join(tools)))
calls = [("search_api_docs", {"query": "export_scene fbx", "max_results": 1}),
         ("get_blendfile_summary_datablocks_for_cli", {"blend_file": probe}),
         ("get_objects_summary", {})]   # live: needs the add-on on localhost:9876
for n, (name, args) in enumerate(calls, start=3):
    send({"jsonrpc": "2.0", "id": n, "method": "tools/call", "params": {"name": name, "arguments": args}})
    r = recv(n)
    res = r.get("result") or {}
    err = "error" in r or bool(res.get("isError"))
    text = "".join(c.get("text", "") for c in res.get("content", [])) or json.dumps(r.get("error"))
    print("MCP call %s isError=%s %s" % (name, err, text[:200].replace("\n", " ")))
p.stdin.close(); p.terminate()
PY
if [ -n "${LIVE_BLENDER_SKIP_MCP:-}" ]; then
  skip "mcp-headless" "LIVE_BLENDER_SKIP_MCP is set"
  skip "mcp-live" "LIVE_BLENDER_SKIP_MCP is set"
elif ! command -v uvx >/dev/null; then
  skip "mcp-headless" "uvx not found"
  skip "mcp-live" "uvx not found"
elif [ -z "$BLENDER" ]; then
  skip "mcp-headless" "no Blender"
  skip "mcp-live" "no Blender"
else
  "$BLENDER" --background --factory-startup --python-expr \
    "import bpy; bpy.ops.mesh.primitive_cube_add(); bpy.ops.wm.save_mainfile(filepath='$SCRATCH/mcp_probe.blend')" >/dev/null 2>&1
  if out=$(BLENDER_PATH=$BLENDER python3 -I "$SCRATCH/mcp.py" "$ROOT/teammates/blender-artist.md" "$SCRATCH/mcp_probe.blend" 2>"$SCRATCH/mcp_stderr.log"); then
    echo "$out" | sed 's/^/  /'
    if grep -q '^MCP call search_api_docs isError=False' <<<"$out" \
       && grep -q '^MCP call get_blendfile_summary_datablocks_for_cli isError=False' <<<"$out"; then
      pass "mcp-headless: $(grep '^MCP tools' <<<"$out" | cut -d' ' -f3) tools; search_api_docs and get_blendfile_summary_datablocks_for_cli answer"
    else
      fail "mcp-headless" "a read-only call failed"
    fi
    if grep -q '^MCP call get_objects_summary isError=False' <<<"$out"; then
      pass "mcp-live: get_objects_summary answers on ${BLENDER_MCP_PORT:-9876}"
    else
      skip "mcp-live" "no add-on on localhost:${BLENDER_MCP_PORT:-9876}. Operator: install Blender 5.1+, install the Blender Lab MCP add-on from projects.blender.org/lab/blender_mcp, start its server, then re-run this script"
    fi
  else
    fail "mcp-headless" "server did not start: $(tail -3 "$SCRATCH/mcp_stderr.log" | tr '\n' ' ')"
    skip "mcp-live" "server did not start"
  fi
fi

# 4. U-15: 1 crate asset end to end with the 4 Blender skills, then a check in a new process.
if [ -n "$BLENDER" ]; then
  rm -rf "$SCRATCH/asset"
  out=$(cd "$SCRATCH" && run_blender build_crate.py -- asset || true)
  grep -E '^ASSET ' <<<"$out" | sed 's/^/  /'
  if grep -q '^ASSET DONE' <<<"$out"; then
    pass "asset-build: $(grep -c '"ok": true' <<<"$out") skill-step checks pass"
    vout=$(cd "$SCRATCH" && run_blender verify_crate.py -- asset || true)
    grep -E '^VERIFY ' <<<"$vout" | sed 's/^/  /'
    if grep -q '^VERIFY RESULT PASS' <<<"$vout"; then pass "asset-verify"; else fail "asset-verify" "$(grep '^VERIFY RESULT' <<<"$vout" || tail -3 <<<"$vout")"; fi
  else
    fail "asset-build" "$(grep -E '^ASSET FAIL|Error' <<<"$out" | head -3 | tr '\n' ' ')"
    skip "asset-verify" "no asset"
  fi
else
  skip "asset-build" "no Blender"
fi

# 5. U-55: the pinned flags in the teammate file resolve to PIN_SET.
if command -v uv >/dev/null && command -v git >/dev/null; then
  ARGS=$(python3 -I -c 'import json,re,sys; l=next(l for l in open(sys.argv[1]) if re.match(r"\s+blender:\s*\{", l)); a=json.loads(l.split(":",1)[1])["args"]; print(" ".join(a[:a.index("--from")]))' teammates/blender-artist.md)
  PY_VER=$(sed -n 's/.*--python \([^ ]*\).*/\1/p' <<<"$ARGS")
  EXCL=$(sed -n 's/.*--exclude-newer \([^ ]*\).*/\1/p' <<<"$ARGS")
  if [ -z "$PY_VER" ] || [ -z "$EXCL" ]; then
    fail "mcp-pin" "teammate args lack --python or --exclude-newer: $ARGS"
  else
    [ -d "$SCRATCH/src/.git" ] || git clone -q https://projects.blender.org/lab/blender_mcp.git "$SCRATCH/src"
    git -C "$SCRATCH/src" checkout -q "$MCP_COMMIT"
    tr ' ' '\n' <<<"$ARGS" | sed -n '/^--with$/{n;p;}' >"$SCRATCH/with.txt"
    got=$(uv pip compile "$SCRATCH/src/mcp/pyproject.toml" "$SCRATCH/with.txt" --python-version "$PY_VER" \
          --exclude-newer "$EXCL" --no-header --no-annotate -q 2>&1 | tr '\n' ' ' | sed 's/ $//')
    if [ "$got" = "$PIN_SET" ]; then pass "mcp-pin: $(wc -w <<<"$got" | tr -d ' ') packages, mcp==1.30.0, exclude-newer $EXCL"
    else fail "mcp-pin" "resolved set differs: $got"; fi
  fi
else
  skip "mcp-pin" "uv or git not found"
fi

# Clean up: the server clone and the check files. Keep the asset unless asked.
rm -rf "$SCRATCH/src" "$SCRATCH"/*.fbx "$SCRATCH"/bake_check_*.png "$SCRATCH"/mcp_probe.blend*
[ -n "${LIVE_BLENDER_CLEAN:-}" ] && rm -rf "$SCRATCH/asset"
exit $FAILED
