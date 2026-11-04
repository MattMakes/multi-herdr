"""Read and write 3MF Core files with the Python standard library only. No Blender, no add-on.

write_3mf(path, shells): `shells` is a list of (vertices, triangles) in mm. Writes 1 object per
shell, 1 parent object that holds them as components, and 1 build item. unit="millimeter".
read_3mf(path): returns a list of (vertices, triangles) in mm, 1 per mesh object that a build item
reaches, with build-item and component transforms applied. Reads files from other slicers too.
"""
import zipfile
from xml.etree import ElementTree as ET

CORE_NS = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02"
REL_NS = "http://schemas.openxmlformats.org/package/2006/relationships"
MODEL_PART = "3D/3dmodel.model"
PARTS = ("[Content_Types].xml", "_rels/.rels", MODEL_PART)
UNIT_MM = {"micron": 0.001, "millimeter": 1.0, "centimeter": 10.0, "inch": 25.4, "foot": 304.8, "meter": 1000.0}

CONTENT_TYPES = (
    '<?xml version="1.0" encoding="UTF-8"?>\n'
    '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
    '<Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>'
    "</Types>"
)
RELS = (
    '<?xml version="1.0" encoding="UTF-8"?>\n'
    f'<Relationships xmlns="{REL_NS}">'
    f'<Relationship Target="/{MODEL_PART}" Id="rel0" '
    'Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>'
    "</Relationships>"
)


class ThreeMFError(Exception):
    pass


def model_xml(shells):
    """The text of 3D/3dmodel.model for `shells`, a list of (vertices, triangles) in mm."""
    out = ['<?xml version="1.0" encoding="UTF-8"?>\n',
           f'<model unit="millimeter" xml:lang="en-US" xmlns="{CORE_NS}"><resources>']
    for i, (verts, tris) in enumerate(shells, start=1):
        out.append(f'<object id="{i}" type="model"><mesh><vertices>')
        out += [f'<vertex x="{x:.6f}" y="{y:.6f}" z="{z:.6f}"/>' for x, y, z in verts]
        out.append("</vertices><triangles>")
        out += [f'<triangle v1="{a}" v2="{b}" v3="{c}"/>' for a, b, c in tris]
        out.append("</triangles></mesh></object>")
    parent = len(shells) + 1
    out.append(f'<object id="{parent}" type="model"><components>')
    out += [f'<component objectid="{i}"/>' for i in range(1, len(shells) + 1)]
    out.append(f'</components></object></resources><build><item objectid="{parent}"/></build></model>')
    return "".join(out)


def write_3mf(path, shells):
    if not shells:
        raise ThreeMFError("no mesh to write")
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr(PARTS[0], CONTENT_TYPES)
        z.writestr(PARTS[1], RELS)
        z.writestr(PARTS[2], model_xml(shells))


def parse_transform(text):
    """A 3MF transform is 12 numbers: a 3x3 matrix by rows, then the move. Returns a 3x4 row list."""
    if not text:
        return [[1, 0, 0], [0, 1, 0], [0, 0, 1], [0, 0, 0]]
    v = [float(x) for x in text.split()]
    if len(v) != 12:
        raise ThreeMFError(f"transform needs 12 numbers, got {len(v)}")
    return [v[0:3], v[3:6], v[6:9], v[9:12]]


def compose(inner, outer):
    """The transform that applies `inner` first, then `outer`."""
    rot = [[sum(inner[r][k] * outer[k][c] for k in range(3)) for c in range(3)] for r in range(3)]
    move = [sum(inner[3][k] * outer[k][c] for k in range(3)) + outer[3][c] for c in range(3)]
    return rot + [move]


def apply(m, p):
    return tuple(sum(p[k] * m[k][c] for k in range(3)) + m[3][c] for c in range(3))


def local(tag):
    return tag.rsplit("}", 1)[-1]


def read_objects(root):
    """Map object id to ('mesh', verts, tris) or ('components', [(id, transform)])."""
    objs = {}
    for res in (e for e in root if local(e.tag) == "resources"):
        for o in (e for e in res if local(e.tag) == "object"):
            for part in o:
                if local(part.tag) == "mesh":
                    verts, tris = [], []
                    for sec in part:
                        if local(sec.tag) == "vertices":
                            verts = [(float(v.get("x")), float(v.get("y")), float(v.get("z"))) for v in sec]
                        elif local(sec.tag) == "triangles":
                            tris = [(int(t.get("v1")), int(t.get("v2")), int(t.get("v3"))) for t in sec]
                    objs[o.get("id")] = ("mesh", verts, tris)
                elif local(part.tag) == "components":
                    objs[o.get("id")] = ("components", [(c.get("objectid"), parse_transform(c.get("transform")))
                                                        for c in part])
    return objs


def flatten(objs, oid, m, depth=0):
    """Yield (verts, tris) in model units for object `oid` under transform `m`."""
    if oid not in objs:
        raise ThreeMFError(f"object {oid} is missing")
    if depth > 8:
        raise ThreeMFError("components nest more than 8 levels")
    kind = objs[oid]
    if kind[0] == "mesh":
        yield [apply(m, v) for v in kind[1]], kind[2]
    else:
        for child, t in kind[1]:
            yield from flatten(objs, child, compose(t, m), depth + 1)


def model_part_name(z):
    """The model part named by the package relationships, else the standard name."""
    if "_rels/.rels" in z.namelist():
        for rel in ET.fromstring(z.read("_rels/.rels")):
            if rel.get("Type", "").endswith("/3dmodel") and rel.get("Target"):
                return rel.get("Target").lstrip("/")
    return MODEL_PART


def read_3mf(path):
    try:
        with zipfile.ZipFile(path) as z:
            root = ET.fromstring(z.read(model_part_name(z)))
    except (zipfile.BadZipFile, KeyError, ET.ParseError) as e:
        raise ThreeMFError(f"not a readable 3MF file: {e}")
    unit = root.get("unit", "millimeter")
    if unit not in UNIT_MM:
        raise ThreeMFError(f"unknown unit {unit!r}")
    scale = UNIT_MM[unit]
    objs = read_objects(root)
    items = [i for b in root if local(b.tag) == "build" for i in b if local(i.tag) == "item"]
    if not items:
        raise ThreeMFError("no build item")
    shells = []
    for item in items:
        for verts, tris in flatten(objs, item.get("objectid"), parse_transform(item.get("transform"))):
            if any(max(t) >= len(verts) or min(t) < 0 for t in tris):
                raise ThreeMFError("a triangle points at a missing vertex")
            shells.append(([tuple(c * scale for c in v) for v in verts], tris))
    if not any(tris for _, tris in shells):
        raise ThreeMFError("no triangles")
    return shells
