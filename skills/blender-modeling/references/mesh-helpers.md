# Mesh helpers

Load at step 1. Paste the functions you need at the top of your task script, or keep them in `Scripts/Blender/mesh_helpers.py` and import it. Every function was run against the `bpy` 5.0.1 module in a background process: a 64x32 UV sphere was measured, unwrapped (0 UVs out of bounds), given a lightmap channel, decimated to a 2,000-triangle LOD1 (1,984 after fitting) and given a 148-face convex hull that `is_convex` accepted; a torus was correctly reported concave. On 2026-10-06 they ran again on Blender 3.5.1 (`docs/live-checks/blender.md`): the same checks passed, and a crate asset found 2 bugs that are now fixed: `unwrap_smart` failed when no object was active (an object made with `bpy.data.objects.new`), and `make_convex_collision` failed on 2 stacked chamfered boxes, where the hull's interior and unused lists share a vertex.

Run the code in object mode. `unwrap_smart` and `add_lightmap_uv` enter and leave edit mode themselves.

```python
import bpy
import bmesh
import math


def evaluated_tris(ob):
    """Triangle count after modifiers, as the exporter writes it."""
    dg = bpy.context.evaluated_depsgraph_get()
    ev = ob.evaluated_get(dg)
    me = ev.to_mesh()
    try:
        me.calc_loop_triangles()
        return len(me.loop_triangles)
    finally:
        ev.to_mesh_clear()


def mesh_problems(ob, area_eps=1e-8):
    """Topology problems that break baking, export or collision."""
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    try:
        bm.edges.ensure_lookup_table()
        out = {
            "non_manifold_edges": sum(1 for e in bm.edges if not e.is_manifold and not e.is_boundary),
            "boundary_edges": sum(1 for e in bm.edges if e.is_boundary),
            "loose_verts": sum(1 for v in bm.verts if not v.link_edges),
            "loose_edges": sum(1 for e in bm.edges if not e.link_faces),
            "zero_area_faces": sum(1 for f in bm.faces if f.calc_area() < area_eps),
            "ngons": sum(1 for f in bm.faces if len(f.verts) > 4),
            "faces": len(bm.faces),
        }
        return out
    finally:
        bm.free()


def make_lod(src, level, ratio):
    """Copy src as <base>_LOD<level> with a Decimate (collapse) modifier.

    Shares no mesh data with src. UVs survive a collapse decimate; check the
    UV layout after, and keep materials in the same slot order.
    """
    base = src.name.rsplit("_LOD", 1)[0]
    lod = src.copy()
    lod.data = src.data.copy()
    lod.name = lod.data.name = f"{base}_LOD{level}"
    for coll in src.users_collection:
        coll.objects.link(lod)
    mod = lod.modifiers.new("LOD_Decimate", 'DECIMATE')
    mod.decimate_type = 'COLLAPSE'
    mod.ratio = ratio
    mod.use_collapse_triangulate = True
    # Keep the Triangulate modifier last, as blender-ue-pipeline rule 7 says.
    tri = next((m for m in lod.modifiers if m.type == 'TRIANGULATE'), None)
    if tri is not None:
        bpy.context.view_layer.objects.active = lod
        bpy.ops.object.modifier_move_to_index(modifier=tri.name, index=len(lod.modifiers) - 1)
    return lod


def fit_lod_ratio(lod, target_tris, tol=0.03, steps=12):
    """Binary-search the Decimate ratio until the evaluated count is within tol."""
    mod = lod.modifiers["LOD_Decimate"]
    lo, hi = 0.0, 1.0
    for _ in range(steps):
        mod.ratio = (lo + hi) / 2
        n = evaluated_tris(lod)
        if abs(n - target_tris) <= tol * target_tris:
            break
        if n > target_tris:
            hi = mod.ratio
        else:
            lo = mod.ratio
    return mod.ratio, evaluated_tris(lod)


def unwrap_smart(ob, angle_deg=66.0, margin=0.004):
    """UV channel 0 by Smart UV Project, then pack. Starts in object mode.

    margin is a fraction of the UV square: pixels / texture size, so 0.004 is
    8 px at 2048 and 4 px at 1024. margin_method='FRACTION' makes it exact;
    the default 'SCALED' scales the margin with the islands.
    """
    # Set the active object first: mode_set fails with no active object, which
    # is the state after bpy.data.objects.new.
    if bpy.context.object is not None and bpy.context.object.mode != 'OBJECT':
        bpy.ops.object.mode_set(mode='OBJECT')
    bpy.ops.object.select_all(action='DESELECT')
    ob.select_set(True)
    bpy.context.view_layer.objects.active = ob
    if not ob.data.uv_layers:
        ob.data.uv_layers.new(name="UVMap")
    ob.data.uv_layers.active_index = 0
    bpy.ops.object.mode_set(mode='EDIT')
    bpy.ops.mesh.select_all(action='SELECT')
    bpy.ops.uv.smart_project(angle_limit=math.radians(angle_deg), island_margin=margin,
                             margin_method='FRACTION')
    bpy.ops.uv.pack_islands(margin=margin, margin_method='FRACTION')
    bpy.ops.object.mode_set(mode='OBJECT')


def add_lightmap_uv(ob, margin=0.01):
    """Non-overlapping UV channel 1 for baked lighting (blender-ue-pipeline rule 9)."""
    me = ob.data
    if len(me.uv_layers) < 2:
        me.uv_layers.new(name="Lightmap")
    me.uv_layers.active_index = 1
    bpy.ops.object.select_all(action='DESELECT')
    ob.select_set(True)
    bpy.context.view_layer.objects.active = ob
    bpy.ops.object.mode_set(mode='EDIT')
    bpy.ops.mesh.select_all(action='SELECT')
    bpy.ops.uv.smart_project(angle_limit=math.radians(66.0), island_margin=margin,
                             margin_method='FRACTION')
    bpy.ops.uv.pack_islands(margin=margin, rotate=True, margin_method='FRACTION')
    bpy.ops.object.mode_set(mode='OBJECT')
    me.uv_layers.active_index = 0


def texel_density(ob, texture_px, uv_index=0):
    """Pixels per metre on UV channel uv_index, for a square texture of texture_px.

    Uses world-space area, so apply scale first or the number is wrong.
    """
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    bm.transform(ob.matrix_world)
    try:
        uv = bm.loops.layers.uv[uv_index]
        area_3d = sum(f.calc_area() for f in bm.faces)
        area_uv = 0.0
        for f in bm.faces:
            pts = [l[uv].uv for l in f.loops]
            s = 0.0
            for i in range(len(pts)):
                a, b = pts[i], pts[(i + 1) % len(pts)]
                s += a.x * b.y - b.x * a.y
            area_uv += abs(s) / 2
        if area_3d == 0:
            return 0.0
        return math.sqrt(area_uv / area_3d) * texture_px
    finally:
        bm.free()


def uv_out_of_bounds(ob, uv_index=0):
    """Count UV loops outside 0..1 (wrong for a unique bake layout)."""
    uv = ob.data.uv_layers[uv_index].data
    return sum(1 for d in uv if not (0.0 <= d.uv.x <= 1.0 and 0.0 <= d.uv.y <= 1.0))


def make_convex_collision(render_ob, index=0, max_faces=None):
    """UCX_<render>_<NN>: a convex hull of the render mesh, parented to it.

    No material, no UVs. For a concave shape, split the render mesh into
    convex parts first and call this once per part (index 0, 1, ...).
    max_faces simplifies the hull: dissolve near-planar regions at a growing
    angle, then hull the remaining vertices again, so it stays convex.
    """
    def hull(points):
        bm = bmesh.new()  # vertices only, so no source faces survive
        for co in points:
            bm.verts.new(co)
        res = bmesh.ops.convex_hull(bm, input=bm.verts[:])
        # The two lists can share a vertex (coplanar points, as on a box);
        # delete rejects duplicates, so pass each element once.
        drop = list(dict.fromkeys(res["geom_interior"] + res["geom_unused"]))
        bmesh.ops.delete(bm, geom=drop, context='VERTS')
        return bm

    bm = hull([v.co.copy() for v in render_ob.data.vertices])
    angle = 5.0
    while max_faces is not None and len(bm.faces) > max_faces and angle <= 60.0:
        bmesh.ops.dissolve_limit(bm, angle_limit=math.radians(angle),
                                 verts=bm.verts[:], edges=bm.edges[:])
        points = [v.co.copy() for v in bm.verts]
        bm.free()
        bm = hull(points)
        angle += 5.0
    bmesh.ops.triangulate(bm, faces=bm.faces[:])
    name = f"UCX_{render_ob.name}_{index:02d}"
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    col = bpy.data.objects.new(name, me)
    for coll in render_ob.users_collection:
        coll.objects.link(col)
    col.parent = render_ob
    col.matrix_parent_inverse = render_ob.matrix_world.inverted()
    col.display_type = 'WIRE'
    return col


def is_convex(ob, eps=1e-5):
    """True when every vertex lies on or behind every face plane."""
    me = ob.data
    verts = [v.co for v in me.vertices]
    for poly in me.polygons:
        n, c = poly.normal, poly.center
        if any((v - c).dot(n) > eps for v in verts):
            return False
    return True
```

## Notes

- `evaluated_tris` is the number to compare with a budget: it includes the Decimate and Triangulate modifiers, as the FBX exporter applies them.
- `mesh_problems` counts boundary edges separately from non-manifold edges. A closed asset has 0 of both; a deliberately open mesh (a flag, a leaf card) has boundary edges only.
- `make_lod` copies the mesh data, so editing a LOD never changes LOD0, and moves an existing Triangulate modifier back to the end of the stack.
- `fit_lod_ratio` binary-searches the Decimate ratio; 12 steps reach a 3 % tolerance on any mesh with more than a few hundred triangles.
- `texel_density` measures in world space, so apply scale first. The result is pixels per metre; divide by 100 for pixels per centimetre.
- `make_convex_collision` hulls a vertex-only bmesh and deletes each interior or unused vertex once; `bmesh.ops.delete` rejects a list that holds an element twice. `bmesh.ops.convex_hull` on the full mesh would keep the source faces beside the hull. It also builds the mesh without UVs instead of removing them, because removing a UV layer from a new mesh can fail on an internal layer in Blender 5.0.
- `is_convex` tests every vertex against every face plane: O(vertices x faces). It is fine for collision hulls (tens of faces), not for render meshes.
