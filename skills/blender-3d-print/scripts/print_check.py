"""Check a mesh for FDM or resin 3D printing. Blender headless only:

  "$BLENDER" --background --factory-startup --python-exit-code 2 --python print_check.py -- \
      --in model.stl [--process fdm|resin] [--wall-mm 0.8] [--overhang-deg 45] \
      [--printer-mm 256x256x256] [--units auto|mm|cm|m|in] [--json out.json] [--md out.md]

Prints 1 line per check and a RESULT line. Exit 0: every hard check passes.
Exit 1: a hard check fails. Exit 2: bad arguments, the file does not load, or
the script crashes (Blender returns 2 for that only with --python-exit-code 2).
Reads the file only. print_fix.py imports this module.
"""
import argparse
import json
import math
import os
import sys

import bmesh
import bpy
from mathutils import Matrix
from mathutils.bvhtree import BVHTree

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import threemf  # same folder; the skill is self-contained

UNIT_MM = {"mm": 1.0, "cm": 10.0, "m": 1000.0, "in": 25.4}
WALL_DEFAULT_MM = {"fdm": 0.8, "resin": 0.4}
AREA_EPS_MM2 = 1e-6  # a face below this area is degenerate
EDGE_EPS_MM = 1e-4  # an edge below this length is degenerate
WELD_MM = 1e-5  # vertices this close are 1 vertex (STL stores no shared vertices)
BED_TOL_MM = 0.01  # a vertex this close to the lowest Z touches the bed


class LoadError(Exception):
    pass


def script_args():
    argv = sys.argv
    return argv[argv.index("--") + 1:] if "--" in argv else []


def add_check_args(p):
    p.add_argument("--process", choices=["fdm", "resin"], default="fdm")
    p.add_argument("--wall-mm", type=float, help="minimum wall (default 0.8 fdm, 0.4 resin)")
    p.add_argument("--overhang-deg", type=float, default=45.0)
    p.add_argument("--printer-mm", default="256x256x256", help="build volume X x Y x Z in mm")
    p.add_argument("--units", choices=["auto"] + list(UNIT_MM), default="auto",
                   help="unit of 1 file unit; auto: mm for STL/OBJ/PLY/3MF, m for glTF, scene units for .blend")


def limits(args):
    wall = args.wall_mm if args.wall_mm is not None else WALL_DEFAULT_MM[args.process]
    try:
        printer = [float(x) for x in args.printer_mm.lower().split("x")]
        assert len(printer) == 3
    except (ValueError, AssertionError):
        raise LoadError(f"--printer-mm must be XxYxZ in mm, got {args.printer_mm!r}")
    return {"process": args.process, "wall_mm": wall, "overhang_deg": args.overhang_deg, "printer_mm": printer}


# ---- loading -------------------------------------------------------------

def import_3mf(path):
    """Read a 3MF file with threemf.py (no add-on) into 1 mesh object per shell. Numbers are mm."""
    try:
        shells = threemf.read_3mf(path)
    except threemf.ThreeMFError as e:
        raise LoadError(f"{path}: {e}")
    for i, (verts, tris) in enumerate(shells):
        me = bpy.data.meshes.new(f"shell{i}")
        me.from_pydata(verts, [], tris)
        me.update()
        bpy.context.scene.collection.objects.link(bpy.data.objects.new(f"shell{i}", me))


def load_scene(path):
    """Open or import `path` into a clean scene. Returns the visible mesh objects."""
    if not os.path.isfile(path):
        raise LoadError(f"no such file: {path}")
    ext = os.path.splitext(path)[1].lower()
    if ext == ".blend":
        bpy.ops.wm.open_mainfile(filepath=path, load_ui=False)
    else:
        bpy.ops.wm.read_factory_settings(use_empty=True)
        if ext == ".stl":
            bpy.ops.wm.stl_import(filepath=path)
        elif ext == ".obj":
            bpy.ops.wm.obj_import(filepath=path)
        elif ext == ".ply":
            bpy.ops.wm.ply_import(filepath=path)
        elif ext in (".gltf", ".glb"):
            bpy.ops.import_scene.gltf(filepath=path)
        elif ext == ".3mf":
            import_3mf(path)
        else:
            raise LoadError(f"unsupported file type {ext}: use .stl .obj .ply .3mf .gltf .glb .blend")
    objs = [o for o in bpy.context.scene.objects if o.type == "MESH" and o.visible_get()]
    if not objs:
        raise LoadError(f"no visible mesh object in {path}")
    return objs


def mm_per_unit(path, units):
    """Millimetres in 1 Blender unit after load, and where that number came from."""
    if units != "auto":
        return UNIT_MM[units], f"--units {units}"
    ext = os.path.splitext(path)[1].lower()
    if ext in (".gltf", ".glb"):
        return 1000.0, "glTF is in metres"
    if ext == ".blend":
        s = bpy.context.scene.unit_settings
        return s.scale_length * 1000.0, f"scene unit scale {s.scale_length:g} m"
    return 1.0, f"{ext[1:].upper()} numbers read as mm"


def build_bmesh(objs, mm):
    """1 bmesh in mm of all objects, with transforms and modifiers applied (evaluated mesh)."""
    dg = bpy.context.evaluated_depsgraph_get()
    out = bmesh.new()
    scratch = bpy.data.meshes.new("_print_scratch")
    for o in objs:
        ev = o.evaluated_get(dg)
        tmp = bmesh.new()
        tmp.from_mesh(ev.to_mesh())
        ev.to_mesh_clear()
        m = Matrix.Scale(mm, 4) @ o.matrix_world
        tmp.transform(m)
        if m.determinant() < 0:  # a mirrored object: keep the faces outward
            bmesh.ops.reverse_faces(tmp, faces=tmp.faces)
        tmp.to_mesh(scratch)
        tmp.free()
        out.from_mesh(scratch)
    bpy.data.meshes.remove(scratch)
    return out


# ---- checks --------------------------------------------------------------

def shells(bm):
    """Face islands, linked through shared edges. Returns a list of face lists."""
    seen, islands = set(), []
    for f in bm.faces:
        if f.index in seen:
            continue
        stack, island = [f], []
        seen.add(f.index)
        while stack:
            cur = stack.pop()
            island.append(cur)
            for e in cur.edges:
                for g in e.link_faces:
                    if g.index not in seen:
                        seen.add(g.index)
                        stack.append(g)
        islands.append(island)
    return islands


def intersect_pairs(bm):
    """Pairs of faces that cross. Faces that share a vertex are skipped: BVH overlap reports
    near-coplanar neighbours (a UV sphere pole) as crossing."""
    bm.faces.index_update()
    bm.faces.ensure_lookup_table()
    tree = BVHTree.FromBMesh(bm)
    faces = bm.faces
    return [(a, b) for a, b in tree.overlap(tree)
            if not set(faces[a].verts).intersection(faces[b].verts)]


def ray_hits(tree, faces, outward, limit):
    """Faces whose ray along -normal (wall) or +normal (gap) meets the opposite surface within `limit` mm.
    Returns (face count, area mm2, smallest distance)."""
    n_hit, area, low = 0, 0.0, None
    for f in faces:
        n = f.normal if outward else -f.normal
        origin = f.calc_center_median() + n * 1e-4
        loc, hit_n, idx, dist = tree.ray_cast(origin, n, limit)
        if loc is None or idx == f.index or dist <= 1e-6:
            continue
        # A wall ray leaves the solid through a face that points along it.
        # A gap ray reaches a face that points back at it.
        if (hit_n.dot(n) > 0) != outward:
            n_hit += 1
            area += f.calc_area()
            low = dist if low is None else min(low, dist)
    return n_hit, area, low


def bed_faces(bm, zmin):
    return [f for f in bm.faces if f.normal.z < -0.99 and all(v.co.z - zmin <= BED_TOL_MM for v in f.verts)]


def overhang(bm, deg, zmin):
    """Downward faces steeper than `deg` from vertical, without the bed faces. Returns area mm2."""
    lim = -math.sin(math.radians(deg))
    bed = {f.index for f in bed_faces(bm, zmin)}
    return sum(f.calc_area() for f in bm.faces if f.normal.z < lim and f.index not in bed)


def r(x, n=3):
    return None if x is None else round(x, n)


def run_checks(bm, lim):
    """Run every check on `bm` (mm). Does not change `bm`. Returns the report dict."""
    bm = bm.copy()
    before = len(bm.verts)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=WELD_MM)
    welded = before - len(bm.verts)
    for seq in (bm.verts, bm.edges, bm.faces):
        seq.index_update()
        seq.ensure_lookup_table()
    bm.normal_update()
    checks = []

    def add(name, status, value, rule, hard):
        checks.append({"name": name, "status": status, "hard": hard, "value": value, "rule": rule})

    def hard(name, ok, value, rule):
        add(name, "PASS" if ok else "FAIL", value, rule, True)

    # Topology.
    boundary = sum(1 for e in bm.edges if len(e.link_faces) == 1)
    multi = sum(1 for e in bm.edges if len(e.link_faces) > 2)
    bowtie = sum(1 for v in bm.verts if v.link_faces and not v.is_manifold
                 and all(len(e.link_faces) == 2 for e in v.link_edges))
    hard("manifold", boundary == multi == bowtie == 0,
         {"boundary_edges": boundary, "non_manifold_edges": multi, "non_manifold_verts": bowtie},
         "boundary edges = 0, edges with more than 2 faces = 0, non-manifold vertices = 0")

    pairs = intersect_pairs(bm)
    hard("intersections", not pairs,
         {"face_pairs": len(pairs), "faces": len({i for p in pairs for i in p})},
         "self-intersecting face pairs (BVH overlap) = 0")

    fixed = bm.copy()  # the same mesh with recalculated outward normals
    bmesh.ops.recalc_face_normals(fixed, faces=fixed.faces)
    fixed.normal_update()
    for seq in (fixed.verts, fixed.faces):
        seq.ensure_lookup_table()
    flipped = sum(1 for a, b in zip(bm.faces, fixed.faces) if a.normal.dot(b.normal) < 0)
    hard("normals", flipped == 0, {"flipped_faces": flipped},
         "faces that point inward (differ from recalculated outward normals) = 0")

    small_f = sum(1 for f in bm.faces if f.calc_area() < AREA_EPS_MM2)
    short_e = sum(1 for e in bm.edges if e.calc_length() < EDGE_EPS_MM)
    hard("degenerate", small_f == short_e == 0, {"zero_area_faces": small_f, "zero_length_edges": short_e},
         f"faces under {AREA_EPS_MM2:g} mm2 = 0, edges under {EDGE_EPS_MM:g} mm = 0")

    loose_v = sum(1 for v in bm.verts if not v.link_edges)
    loose_e = sum(1 for e in bm.edges if not e.link_faces)
    hard("loose", loose_v == loose_e == 0, {"loose_verts": loose_v, "loose_edges": loose_e},
         "vertices without edges = 0, edges without faces = 0")

    interior = sum(1 for f in bm.faces if all(len(e.link_faces) > 2 for e in f.edges))
    hard("interior_faces", interior == 0, {"interior_faces": interior},
         "faces whose every edge has more than 2 faces = 0")

    islands = shells(bm)
    holes = 0  # through-holes (genus) of closed shells, from V - E + F = 2 - 2g
    for isl in islands:
        es = {e for f in isl for e in f.edges}
        if all(len(e.link_faces) == 2 for e in es):
            vs = {v for f in isl for v in f.verts}
            holes += max(0, (2 - (len(vs) - len(es) + len(isl))) // 2)
    add("shells", "INFO", {"shells": len(islands)}, "count of separate face islands; report only", False)

    # Size.
    if bm.verts:
        lo = [min(v.co[i] for v in bm.verts) for i in range(3)]
        hi = [max(v.co[i] for v in bm.verts) for i in range(3)]
    else:
        lo = hi = [0.0, 0.0, 0.0]
    dims = [b - a for a, b in zip(lo, hi)]
    too_small = max(dims) < 1.0
    too_big = [ax for ax, d, p in zip("XYZ", dims, lim["printer_mm"]) if d > p]
    hard("scale", not too_small and not too_big,
         {"bbox_mm": [r(d) for d in dims], "below_1mm": too_small, "over_printer_axes": too_big,
          "printer_mm": lim["printer_mm"]},
         "largest bbox side >= 1 mm and every side <= the printer volume; "
         "a part under 1 mm is often a model in m or cm read as mm")

    # Geometry, on the outward-normal copy.
    tree = BVHTree.FromBMesh(fixed)
    t_n, t_area, t_min = ray_hits(tree, fixed.faces, False, lim["wall_mm"])
    hard("wall_thickness", t_n == 0,
         {"thin_faces": t_n, "thin_area_mm2": r(t_area, 2), "min_mm": r(t_min), "wall_mm": lim["wall_mm"]},
         f"faces whose inward ray leaves the solid within {lim['wall_mm']:g} mm = 0")

    g_n, g_area, g_min = ray_hits(tree, fixed.faces, True, lim["wall_mm"])
    tiny = sum(1 for isl in islands if isl and min(
        max(v.co[i] for f in isl for v in f.verts) - min(v.co[i] for f in isl for v in f.verts)
        for i in range(3)) < lim["wall_mm"])
    add("thin_features", "WARN" if (g_n or tiny) else "PASS",
        {"gap_faces": g_n, "gap_area_mm2": r(g_area, 2), "min_gap_mm": r(g_min),
         "shells_under_wall": tiny, "through_holes": holes},
        f"gaps and holes narrower than {lim['wall_mm']:g} mm fuse when printed; "
        f"shells thinner than {lim['wall_mm']:g} mm do not print; report only", False)

    zmin = lo[2]
    o_area = overhang(fixed, lim["overhang_deg"], zmin)
    add("overhang", "WARN" if o_area > 0 else "PASS",
        {"area_mm2": r(o_area, 2), "overhang_deg": lim["overhang_deg"]},
        f"downward area steeper than {lim['overhang_deg']:g} deg from vertical needs supports; report only", False)

    contact = sum(f.calc_area() for f in bed_faces(fixed, zmin))
    on_bed = abs(zmin) <= BED_TOL_MM
    add("bed", "PASS" if on_bed and contact > 0 else "WARN",
        {"z_min_mm": r(zmin), "contact_area_mm2": r(contact, 2)},
        "lowest Z = 0 and a flat bed contact area > 0; report only", False)

    stats = {"verts": len(bm.verts), "tris": sum(len(f.verts) - 2 for f in bm.faces), "shells": len(islands),
             "bbox_mm": [r(d) for d in dims], "volume_mm3": r(abs(fixed.calc_volume(signed=True)), 2),
             "welded_verts": welded}
    fixed.free()
    bm.free()
    failed = [c["name"] for c in checks if c["hard"] and c["status"] == "FAIL"]
    return {"limits": lim, "stats": stats, "checks": checks, "failed": failed,
            "result": "FAIL" if failed else "PASS"}


# ---- output --------------------------------------------------------------

def fmt(value):
    return " ".join(f"{k}={v}" for k, v in value.items())


def print_report(rep):
    for c in rep["checks"]:
        print(f"CHECK {c['name']} {c['status']} {fmt(c['value'])}")
    print(f"RESULT {rep['result']}" + (f" failed: {', '.join(rep['failed'])}" if rep["failed"] else ""))


def markdown(rep):
    s = rep["stats"]
    lines = [f"# Print check: {os.path.basename(rep['input'])}", "",
             (f"Result: **{rep['result']}**. Process {rep['limits']['process']}, wall {rep['limits']['wall_mm']:g} mm, "
              f"overhang {rep['limits']['overhang_deg']:g} deg. Units: {rep['units_source']}."), "",
             (f"Size {' x '.join(str(d) for d in s['bbox_mm'])} mm, {s['tris']} triangles, {s['shells']} shells, "
              f"volume {s['volume_mm3']} mm3."), ""]
    for f in rep.get("fixes", []):
        lines.append(f"- fix `{f['step']}`: {f['detail']}")
    if rep.get("fixes"):
        lines.append("")
    lines += ["| check | status | value | rule |", "| --- | --- | --- | --- |"]
    lines += [f"| {c['name']} | {c['status']} | {fmt(c['value'])} | {c['rule']} |" for c in rep["checks"]]
    return "\n".join(lines) + "\n"


def write_outputs(rep, json_path, md_path):
    if json_path:
        with open(json_path, "w") as fh:
            json.dump(rep, fh, indent=2)
    if md_path:
        with open(md_path, "w") as fh:
            fh.write(markdown(rep))


def check_file(path, args):
    """Load `path`, check it and return the report."""
    lim = limits(args)
    objs = load_scene(path)
    mm, src = mm_per_unit(path, args.units)
    bm = build_bmesh(objs, mm)
    rep = {"tool": "print_check", "input": os.path.abspath(path), "blender": bpy.app.version_string,
           "mm_per_unit": mm, "units_source": src}
    rep.update(run_checks(bm, lim))
    bm.free()
    return rep


def main():
    p = argparse.ArgumentParser(prog="print_check.py")
    p.add_argument("--in", dest="inp", required=True)
    p.add_argument("--json")
    p.add_argument("--md")
    add_check_args(p)
    try:
        args = p.parse_args(script_args())
        rep = check_file(args.inp, args)
    except LoadError as e:
        print(f"ERROR {e}")
        sys.exit(2)
    except SystemExit as e:  # argparse error
        sys.exit(2 if e.code else 0)
    print_report(rep)
    write_outputs(rep, args.json, args.md)
    sys.exit(0 if rep["result"] == "PASS" else 1)


if __name__ == "__main__":
    main()
