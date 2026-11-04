"""Write a print-ready copy of a mesh, then check the copy. Blender headless only:

  "$BLENDER" --background --factory-startup --python print_fix.py -- \
      --in model.obj --out model_print.stl [--scale F] [--merge-mm 0.01] \
      [--remesh-voxel-mm X] [--target-tris N] [--solidify-mm X] [--thicken-mm X] \
      [--hollow-mm X] [--drain-mm 2.5] [--orient] [--json out.json] [--md out.md] \
      [check options of print_check.py: --process --wall-mm --overhang-deg --printer-mm --units]

Never writes to --in. The safe fixes always run, in this order: apply transforms
and modifiers, merge by distance, dissolve degenerate, delete loose, remove interior
faces, fill holes, recalculate normals, union intersecting shells, drop to bed.
The options add the lossy fixes. Then the copy is loaded again and checked.
Exit codes are the same as print_check.py.
"""
import argparse
import os
import sys

import bmesh
import bpy
from mathutils import Matrix, Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import print_check as pc  # noqa: E402  (same folder; the skill is self-contained)

LOG = []


def log(step, detail, warn=False):
    LOG.append({"step": step, "detail": detail})
    print(f"{'WARN' if warn else 'FIX'} {step}: {detail}")


def counts(bm):
    return len(bm.verts), len(bm.edges), len(bm.faces)


def to_object(bm, name):
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    o = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(o)
    return o


def apply_modifier(bm, name, setup):
    """Run 1 modifier on `bm` through a scratch object. Returns a new bmesh."""
    o = to_object(bm, "_fix")
    mod = setup(o)
    dg = bpy.context.evaluated_depsgraph_get()
    ev = o.evaluated_get(dg)
    out = bmesh.new()
    out.from_mesh(ev.to_mesh())
    ev.to_mesh_clear()
    o.modifiers.remove(mod)
    bpy.data.meshes.remove(o.data)
    bm.free()
    return out


def boolean(bm, cutters, operation):
    """EXACT boolean of `bm` with the bmeshes in `cutters`. Returns a new bmesh."""
    coll = bpy.data.collections.new("_cutters")
    bpy.context.scene.collection.children.link(coll)
    objs = []
    for c in cutters:
        o = to_object(c, "_cutter")
        bpy.context.scene.collection.objects.unlink(o)
        coll.objects.link(o)
        objs.append(o)

    def setup(o):
        m = o.modifiers.new("bool", "BOOLEAN")
        m.operation, m.solver, m.operand_type, m.collection = operation, "EXACT", "COLLECTION", coll
        return m

    out = apply_modifier(bm, "bool", setup)
    for o in objs:
        bpy.data.meshes.remove(o.data)
    bpy.data.collections.remove(coll)
    bmesh.ops.dissolve_degenerate(out, dist=pc.EDGE_EPS_MM, edges=out.edges[:])  # booleans leave slivers
    return out


def recalc(bm):
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.normal_update()


def split_shells(bm):
    out = []
    for isl in pc.shells(bm):
        part = bmesh.new()
        vmap = {}
        for f in isl:
            for v in f.verts:
                if v not in vmap:
                    vmap[v] = part.verts.new(v.co)
            part.faces.new([vmap[v] for v in f.verts])
        out.append(part)
    return out


def safe_fixes(bm, args):
    v0 = len(bm.verts)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=args.merge_mm)
    log("merge-by-distance", f"{v0 - len(bm.verts)} vertices merged within {args.merge_mm:g} mm")

    before = counts(bm)
    bmesh.ops.dissolve_degenerate(bm, dist=pc.EDGE_EPS_MM, edges=bm.edges[:])
    after = counts(bm)
    log("dissolve-degenerate", f"{before[2] - after[2]} faces and {before[1] - after[1]} edges removed")

    loose_e = [e for e in bm.edges if not e.link_faces]
    bmesh.ops.delete(bm, geom=loose_e, context="EDGES")
    loose_v = [v for v in bm.verts if not v.link_edges]
    bmesh.ops.delete(bm, geom=loose_v, context="VERTS")
    log("delete-loose", f"{len(loose_e)} edges, {len(loose_v)} vertices")

    interior = [f for f in bm.faces if all(len(e.link_faces) > 2 for e in f.edges)]
    bmesh.ops.delete(bm, geom=interior, context="FACES_ONLY")
    log("remove-interior-faces", f"{len(interior)} faces")

    open_edges = [e for e in bm.edges if len(e.link_faces) == 1]
    if open_edges and args.solidify_mm:
        # An open sheet: give it a thickness. Capping it would make a zero-volume shell.
        bmesh.ops.solidify(bm, geom=bm.faces[:], thickness=args.solidify_mm)
        log("solidify", f"open surface with {len(open_edges)} boundary edges, {args.solidify_mm:g} mm thick")
    elif open_edges:
        res = bmesh.ops.holes_fill(bm, edges=open_edges, sides=0)
        log("fill-holes", f"{len(res['faces'])} faces fill {len(open_edges)} boundary edges")

    normals = [f.normal.copy() for f in bm.faces]
    recalc(bm)
    flipped = sum(1 for n, f in zip(normals, bm.faces) if n.dot(f.normal) < 0)
    log("recalc-normals", f"{flipped} faces flipped outward")

    pairs = pc.intersect_pairs(bm)
    parts = pc.shells(bm)
    if pairs and len(parts) > 1:
        pieces = split_shells(bm)
        bm.free()
        bm = boolean(pieces[0], pieces[1:], "UNION")
        for p in pieces[1:]:
            p.free()
        recalc(bm)
        log("union-shells", f"{len(parts)} shells with {len(pairs)} intersecting face pairs -> "
                            f"{len(pc.shells(bm))} shells")
    elif pairs:
        log("union-shells", f"1 shell intersects itself ({len(pairs)} face pairs); "
                            "use --remesh-voxel-mm to rebuild it", warn=True)
    return bm


def thicken(bm, mm):
    """Move every vertex outward by mm/2: each wall grows by mm, and the outer size grows by mm."""
    bm.normal_update()
    moves = [(v, v.normal * (mm / 2) * v.calc_shell_factor()) for v in bm.verts]
    for v, d in moves:
        v.co += d
    log("thicken", f"surface offset outward by {mm / 2:g} mm; every wall is {mm:g} mm thicker, "
                   f"bbox grows by {mm:g} mm", warn=True)


def hollow(bm, wall, drain):
    """Add an inner shell `wall` mm inside, then cut 2 drain holes of `drain` mm through the lowest wall."""
    bm.normal_update()
    zmin = min(v.co.z for v in bm.verts)
    low = [f for f in bm.faces if f.normal.z < -0.5 and f.calc_center_median().z - zmin <= wall]
    centers = [f.calc_center_median() for f in low]
    res = bmesh.ops.duplicate(bm, geom=bm.verts[:] + bm.edges[:] + bm.faces[:])
    inner_v = [g for g in res["geom"] if isinstance(g, bmesh.types.BMVert)]
    inner_f = [g for g in res["geom"] if isinstance(g, bmesh.types.BMFace)]
    moves = [(v, v.normal * wall * v.calc_shell_factor()) for v in inner_v]
    for v, d in moves:
        v.co -= d
    bmesh.ops.reverse_faces(bm, faces=inner_f)
    bm.normal_update()
    log("hollow", f"inner shell {wall:g} mm inside; sharp concave detail can make it intersect", warn=True)
    if not centers:
        log("drain-holes", "no downward face near the bottom; add drain holes by hand", warn=True)
        return bm
    mid = sum(centers, Vector()) / len(centers)
    first = min(centers, key=lambda c: (c - mid).length)
    second = max(centers, key=lambda c: (c - first).length)
    picks = [first] + ([second] if (second - first).length > 3 * drain else [])
    cutters = []
    for c in picks:
        cut = bmesh.new()
        m = Matrix.Translation(c + Vector((0, 0, wall / 2)))
        bmesh.ops.create_cone(cut, cap_ends=True, segments=32, radius1=drain / 2, radius2=drain / 2,
                              depth=wall + 2, matrix=m)
        cutters.append(cut)
    bm = boolean(bm, cutters, "DIFFERENCE")
    for c in cutters:
        c.free()
    recalc(bm)
    log("drain-holes", f"{len(picks)} holes of {drain:g} mm through the bottom wall at "
                       + ", ".join(f"({c.x:.1f}, {c.y:.1f})" for c in picks))
    return bm


def orient(bm, deg):
    """Rotate so the least overhang area points down. Tries the 6 axes and the 8 largest face normals."""
    bm.normal_update()
    dirs = [Vector(d) for d in ((0, 0, -1), (0, 0, 1), (1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0))]
    dirs += [f.normal.copy() for f in sorted(bm.faces, key=lambda f: -f.calc_area())[:8]]
    best = None
    for d in dirs:
        if d.length < 0.5:
            continue
        rot = d.rotation_difference(Vector((0, 0, -1))).to_matrix().to_4x4()
        test = bm.copy()
        test.transform(rot)
        test.normal_update()
        zmin = min(v.co.z for v in test.verts)
        score = (round(pc.overhang(test, deg, zmin), 2),
                 -sum(f.calc_area() for f in pc.bed_faces(test, zmin)))
        test.free()
        if best is None or score < best[0]:
            best = (score, rot, d)
    bm.transform(best[1])
    log("orient", f"face direction {tuple(round(x, 3) for x in best[2])} down; "
                  f"overhang {best[0][0]} mm2, bed contact {-best[0][1]:.1f} mm2")


def drop_to_bed(bm):
    lo = [min(v.co[i] for v in bm.verts) for i in range(3)]
    hi = [max(v.co[i] for v in bm.verts) for i in range(3)]
    shift = Vector(((lo[0] + hi[0]) / -2, (lo[1] + hi[1]) / -2, -lo[2]))
    bm.transform(Matrix.Translation(shift))
    log("drop-to-bed", f"moved by {tuple(round(x, 3) for x in shift)} mm: centred on X/Y, lowest Z = 0")


def export(bm, path, exporter):
    """Write `bm` (mm) to `path` with 1 file unit = 1 mm. `exporter` is the 3MF operator, or None for STL."""
    bpy.ops.wm.read_factory_settings(use_empty=True)
    us = bpy.context.scene.unit_settings
    us.system, us.scale_length, us.length_unit = "METRIC", 0.001, "MILLIMETERS"
    o = to_object(bm, os.path.splitext(os.path.basename(path))[0])
    o.select_set(True)
    bpy.context.view_layer.objects.active = o
    if exporter is None:
        bpy.ops.wm.stl_export(filepath=path, ascii_format=False, export_selected_objects=True,
                              global_scale=1.0, use_scene_unit=False, apply_modifiers=True)
        log("export", f"{path} (binary STL, 1 unit = 1 mm)")
    else:
        exporter(filepath=path)
        log("export", f"{path} (3MF, add-on exporter)")


def main():
    p = argparse.ArgumentParser(prog="print_fix.py")
    p.add_argument("--in", dest="inp", required=True)
    p.add_argument("--out", required=True, help=".stl, or .3mf when a 3MF exporter is enabled")
    p.add_argument("--json")
    p.add_argument("--md")
    p.add_argument("--scale", type=float, help="multiply the size, for example 1000 for a model in m read as mm")
    p.add_argument("--merge-mm", type=float, default=0.01)
    p.add_argument("--remesh-voxel-mm", type=float, help="last resort: voxel remesh; loses detail below X")
    p.add_argument("--target-tris", type=int, help="decimate to at most N triangles")
    p.add_argument("--solidify-mm", type=float,
                   help="give an open sheet (a leaf, a cloth) this thickness instead of capping it")
    p.add_argument("--thicken-mm", type=float, help="grow every wall by X; the outer size grows by X")
    p.add_argument("--hollow-mm", type=float, help="resin: hollow with this wall and cut 2 drain holes")
    p.add_argument("--drain-mm", type=float, default=2.5, help="drain hole diameter for --hollow-mm")
    p.add_argument("--orient", action="store_true", help="rotate for the least overhang area")
    pc.add_check_args(p)
    try:
        args = p.parse_args(pc.script_args())
    except SystemExit as e:
        sys.exit(2 if e.code else 0)
    try:
        src, out = os.path.realpath(args.inp), os.path.realpath(args.out)
        if src == out:
            raise pc.LoadError("--out is the input file; the fix never overwrites its input")
        ext = os.path.splitext(out)[1].lower()
        exporter = None
        if ext == ".3mf":
            exporter = pc.find_3mf_op("export")
            if exporter is None:
                raise pc.LoadError(f"Blender {bpy.app.version_string} has no 3MF exporter: use --out <file>.stl")
        elif ext != ".stl":
            raise pc.LoadError("--out must end in .stl or .3mf")
        pc.limits(args)
        objs = pc.load_scene(src)
        mm, unit_src = pc.mm_per_unit(src, args.units)
        bm = pc.build_bmesh(objs, mm)
    except pc.LoadError as e:
        print(f"ERROR {e}")
        sys.exit(2)
    log("apply-transforms", f"{len(objs)} objects joined with transforms and modifiers applied; "
                            f"1 unit = {mm:g} mm ({unit_src})")
    if args.scale:
        bm.transform(Matrix.Scale(args.scale, 4))
        log("scale", f"size x {args.scale:g}")
    bm = safe_fixes(bm, args)
    if args.thicken_mm:
        thicken(bm, args.thicken_mm)
    if args.remesh_voxel_mm:
        def setup(o):
            m = o.modifiers.new("remesh", "REMESH")
            m.mode, m.voxel_size = "VOXEL", args.remesh_voxel_mm
            return m
        tris = sum(len(f.verts) - 2 for f in bm.faces)
        bm = apply_modifier(bm, "remesh", setup)
        bmesh.ops.dissolve_degenerate(bm, dist=pc.EDGE_EPS_MM, edges=bm.edges[:])
        specks = [isl for isl in pc.shells(bm) if min(
            max(v.co[i] for f in isl for v in f.verts) - min(v.co[i] for f in isl for v in f.verts)
            for i in range(3)) < pc.limits(args)["wall_mm"]]
        bmesh.ops.delete(bm, geom=[f for isl in specks for f in isl], context="FACES")
        recalc(bm)
        log("voxel-remesh", f"voxel {args.remesh_voxel_mm:g} mm, {tris} -> "
                            f"{sum(len(f.verts) - 2 for f in bm.faces)} tris, {len(specks)} shells under the wall "
                            "deleted; detail under the voxel size is lost",
            warn=True)
    if args.target_tris:
        tris = sum(len(f.verts) - 2 for f in bm.faces)
        if tris > args.target_tris:
            def setup(o):
                m = o.modifiers.new("decimate", "DECIMATE")
                m.decimate_type, m.ratio, m.use_collapse_triangulate = "COLLAPSE", args.target_tris / tris, True
                return m
            bm = apply_modifier(bm, "decimate", setup)
            recalc(bm)
        log("decimate", f"{tris} -> {sum(len(f.verts) - 2 for f in bm.faces)} tris (budget {args.target_tris})")
    if args.hollow_mm:
        bm = hollow(bm, args.hollow_mm, args.drain_mm)
    if args.orient:
        orient(bm, args.overhang_deg)
    drop_to_bed(bm)
    export(bm, out, exporter)
    bm.free()

    print("RECHECK", out)
    rep = pc.check_file(out, argparse.Namespace(**{**vars(args), "units": "mm"}))
    rep["tool"], rep["source"], rep["fixes"] = "print_fix", src, LOG
    pc.print_report(rep)
    pc.write_outputs(rep, args.json, args.md)
    sys.exit(0 if rep["result"] == "PASS" else 1)


if __name__ == "__main__":
    main()
