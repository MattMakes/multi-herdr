# Bake helpers

Load at step 4. Paste the functions you need at the top of your task script, or keep them in `Scripts/Blender/bake_helpers.py` and import it. Every function was run with Cycles on CPU against the `bpy` 5.0.1 module in a background process: a 16x8 sphere received a normal map from a displaced level-6 icosphere (mean (0.501, 0.501, 0.999), standard deviation 0.021 in red and green, so detail was captured), AO and roughness were baked and packed into an ORM, base color was baked with color only, and every PNG was saved and reloaded with matching values. On 2026-10-06 they ran again on Blender 3.5.1 (`docs/live-checks/blender.md`), with the same results, and baked a crate's 1024 normal map and ORM.

```python
import bpy
import numpy as np


def new_bake_image(name, size, non_color, high_bit=False):
    """Blank image for a bake target. Linear data maps are Non-Color.
    high_bit=True gives a float buffer, for a 16-bit normal map."""
    img = bpy.data.images.get(name)
    if img is not None:
        bpy.data.images.remove(img)
    img = bpy.data.images.new(name, width=size, height=size, alpha=False,
                              float_buffer=high_bit, is_data=non_color)
    img.colorspace_settings.name = 'Non-Color' if non_color else 'sRGB'
    return img


def set_bake_target(low_ob, img):
    """Make an Image Texture node holding img the active node in every
    material slot of low_ob. Cycles bakes into the active image node."""
    if not low_ob.material_slots:
        mat = bpy.data.materials.new(f"M_{low_ob.name}")
        low_ob.data.materials.append(mat)
    for slot in low_ob.material_slots:
        mat = slot.material
        mat.use_nodes = True
        nodes = mat.node_tree.nodes
        node = nodes.get("BakeTarget") or nodes.new('ShaderNodeTexImage')
        node.name = node.label = "BakeTarget"
        node.image = img
        node.select = True
        nodes.active = node


def bake(low_ob, high_obs, kind, img, size_px, extrusion=0.02, max_ray=0.0,
         cage_ob=None, samples=None):
    """Bake one map into img with Cycles.

    kind: 'NORMAL' (tangent space, OpenGL / green up), 'AO', 'ROUGHNESS',
    'EMIT', 'DIFFUSE' (color only) ... high_obs empty = bake low onto itself.
    extrusion and max_ray are in metres. Margin scales with resolution.
    """
    scene = bpy.context.scene
    scene.render.engine = 'CYCLES'
    scene.cycles.device = 'CPU'
    if samples is not None:
        scene.cycles.samples = samples
    b = scene.render.bake
    b.use_selected_to_active = bool(high_obs)
    b.cage_extrusion = extrusion
    b.max_ray_distance = max_ray
    b.use_cage = cage_ob is not None
    if cage_ob is not None:
        b.cage_object = cage_ob
    b.margin = max(4, size_px // 128)       # 16 px at 2048
    b.margin_type = 'EXTEND'
    b.target = 'IMAGE_TEXTURES'
    b.use_clear = True
    b.normal_space = 'TANGENT'
    b.normal_r, b.normal_g, b.normal_b = 'POS_X', 'POS_Y', 'POS_Z'  # OpenGL
    if kind == 'DIFFUSE':
        b.use_pass_direct = False
        b.use_pass_indirect = False
        b.use_pass_color = True

    set_bake_target(low_ob, img)
    bpy.ops.object.select_all(action='DESELECT')
    for h in high_obs:
        h.select_set(True)
    low_ob.select_set(True)
    bpy.context.view_layer.objects.active = low_ob
    bpy.ops.object.bake(type=kind)


def save_png(img, path, bits=8):
    """Write img to path as an RGB PNG, pixel values unchanged.

    save_render applies the scene view transform (AgX by default) to a float
    sRGB image, which changes base color values. Switch to Standard for the
    save and restore after. Non-Color images are written as stored.
    """
    scene = bpy.context.scene
    vs = scene.view_settings
    old = (vs.view_transform, vs.look, vs.exposure, vs.gamma)
    vs.view_transform, vs.look, vs.exposure, vs.gamma = 'Standard', 'None', 0.0, 1.0
    s = scene.render.image_settings
    s.file_format = 'PNG'
    s.color_mode = 'RGB'
    s.color_depth = str(bits)
    try:
        img.save_render(path, scene=scene)
    finally:
        vs.view_transform, vs.look, vs.exposure, vs.gamma = old


def load_for_check(path, non_color):
    """Reload a saved map with the right color space before reading pixels.
    A data map loaded as sRGB reads about 0.21 where it stores 0.5."""
    img = bpy.data.images.load(path, check_existing=False)
    img.colorspace_settings.name = 'Non-Color' if non_color else 'sRGB'
    return img


def pack_orm(ao_img, rough_img, metal_img, name, size):
    """T_<Name>_ORM: R = occlusion, G = roughness, B = metallic. All linear.

    metal_img may be None for a fully dielectric asset (B = 0).
    """
    def channel(img):
        px = np.empty(size * size * 4, dtype=np.float32)
        img.pixels.foreach_get(px)
        return px[0::4]
    out = np.ones(size * size * 4, dtype=np.float32)
    out[0::4] = channel(ao_img)
    out[1::4] = channel(rough_img)
    out[2::4] = channel(metal_img) if metal_img is not None else 0.0
    img = new_bake_image(name, size, non_color=True)
    img.pixels.foreach_set(out)
    img.update()
    return img


def normal_map_stats(img):
    """Mean of each channel. A flat tangent normal map is about (0.5, 0.5, 1.0);
    a mean far from that, or a blue channel under 0.5, means a bad bake."""
    n = img.size[0] * img.size[1]
    px = np.empty(n * 4, dtype=np.float32)
    img.pixels.foreach_get(px)
    return tuple(round(float(px[i::4].mean()), 3) for i in range(3))
```

## Notes

- Cycles bakes into the active Image Texture node of each material on the low mesh. `set_bake_target` creates that node (named `BakeTarget`) and makes it active. Remove it, or keep it out of the exported material, after the bake.
- `bake` sets `normal_r/g/b` to `POS_X`, `POS_Y`, `POS_Z`: tangent space, OpenGL (green up), which is what `blender-ue-pipeline` rule 8 hands to Unreal with Flip Green Channel.
- The margin is `max(4, size // 128)` pixels with the Extend type: 16 px at 2048, 32 px at 4096.
- Cycles has no metallic bake type (the types are COMBINED, AO, SHADOW, POSITION, NORMAL, UV, ROUGHNESS, EMIT, ENVIRONMENT, DIFFUSE, GLOSSY, TRANSMISSION). Route metallic into an Emission shader on a material copy and bake `EMIT`.
- `save_png` sets the view transform to Standard during the save. With the default AgX, a float sRGB image holding 0.5 was written as 0.4. Non-Color images and 8-bit sRGB images kept their values either way, but the helper always switches, so the result does not depend on the image type.
- `load_for_check` sets the color space before you read pixels. A 16-bit normal map loaded with the default sRGB space read a mean of 0.216 for red and green, though the file stores 0.5.
- A 16-bit PNG loads back as a float buffer (`depth` 96 for RGB). That is expected.
