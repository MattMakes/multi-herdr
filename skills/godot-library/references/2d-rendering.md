# 2D rendering: canvas, batching, tiles, lights, pixels

Target engine: Godot 4.7. How the canvas renderer draws, what makes 2D slow, and the less-known nodes and settings that fix it (tiles at scale, SDF, screen reads, pixel-perfect output).

## Use this shelf when

- A 2D scene has many sprites, tiles, lights or lines and the frame cost must go down.
- You pick between TileMapLayer, MultiMesh, custom `_draw()` and plain nodes for many objects.
- You need 2D lights, shadows, masks, group opacity or a screen-reading effect and want its cost.
- Pixel art must stay crisp under scaling, camera smoothing and odd window sizes.
- A tile map is edited at runtime, or collision hits must map back to exact cells.

## How the engine works here

- Every 2D node is a `CanvasItem`. `_draw()` runs once; its commands are cached on the server and replayed each frame until `queue_redraw()`. A static node costs no script time after that.
- Order is: canvas layer number, then `z_index`, then tree order (later = on top). `CanvasLayer` is its own layer with its own transform; it does not depend on tree position.
- Transform chain: item local -> canvas (`get_global_transform()`) -> viewport canvas transform (`Camera2D` writes `Viewport.canvas_transform`) -> stretch transform -> window. Scrolling by the canvas transform is cheaper than moving a root node with the whole level under it.
- All three renderers batch 2D: consecutive similar items go out as one draw call. It was Compatibility-only from 4.0; Forward+ and Mobile got it in 4.4. The engine team names the winners: text, repeated sprites sharing a texture, tile maps, bullet hells. Neither docs nor posts list the break rules. In practice a batch ends when texture, material, blend mode or clip state changes between neighbours in draw order (not documented; confirm with the draw-call counter below). Order matters, not node count.
- Batching is a trade: much less CPU work, slightly more GPU work. A scene that cannot batch pays the cost and gains nothing. 4.4 was slower than 4.3 on some GPU-bound low-end phones for this reason; 4.6 redesigned the 2D renderer and measured 1.1x to 7x in GPU-bound cases.
- 2D lights are applied in a single pass inside the item shader (3.x drew one pass per light), at viewport resolution, not texel resolution. Shadows from `LightOccluder2D` polygons and light textures share one atlas (`rendering/2d/shadow_atlas/size`, default 2048).
- A 2D signed distance field of all occluders with `sdf_collision` on is built only when a shader or a `GPUParticles2D` collision asks for it. Unused, it costs nothing.
- 2D has its own buffer and no depth buffer. FXAA does not touch 2D.
- Canvas pipelines are not precompiled: the first draw of a 2D item with a new shader can hitch (the "Canvas" pipeline monitor counts these). See `shaders-advanced`.

## Advanced techniques

### Count canvas draw calls before and after

`Viewport.get_render_info()` with `RENDER_INFO_TYPE_CANVAS` gives 2D-only numbers. Use it to prove a batching change.

```gdscript
extends Node2D

func _process(_delta: float) -> void:
	var vp: Viewport = get_viewport()
	var calls: int = vp.get_render_info(
			Viewport.RENDER_INFO_TYPE_CANVAS, Viewport.RENDER_INFO_DRAW_CALLS_IN_FRAME)
	if Engine.get_frames_drawn() % 60 == 0:
		print("canvas draw calls: ", calls)
```

### Keep batches long

Pack sprites that draw near each other into one atlas, share one material, and keep items with a special shader together in draw order instead of interleaved. Use `instance uniform` values (`CanvasItem.set_instance_shader_parameter()`, canvas shaders since 4.4) instead of one material copy per sprite: the release notes state this does not break batching. If the game shows many unique textures per frame, raise `rendering/2d/batching/uniform_set_cache_size` (default 4096); `rendering/2d/batching/item_buffer_size` (16384) caps commands per batch.

### TileMapLayer chunking

`rendering_quadrant_size` (default 16) groups 16 x 16 cells into one canvas item. Bigger quadrants mean fewer items but a bigger rebuild when one cell changes. A Y-sorted layer ignores quadrants and groups by row instead, so keep Y-sort to the one layer that needs it. Before 4.5 every solid tile was its own physics body, which the engine team called extremely wasteful. Since 4.5, `physics_quadrant_size` (default 16) merges collision shapes of similar cells in a quadrant. Trap: `get_coords_for_body_rid()` then returns the quadrant, not the cell. Set the size to 1 for exact cells (this turns chunking off), or convert the hit position with `local_to_map()` instead.

### Runtime tile overrides without new tiles

`_use_tile_data_runtime_update()` and `_tile_data_runtime_update()` let a script change a `TileData` per cell (modulate, collision, custom data) when the cell is built. Return `true` only for cells that need it: every such cell pays extra. Sub-resources of the passed `TileData` are shared with the TileSet; duplicate before editing. Call `notify_runtime_tile_data_update()` when your answer changes. Cell edits are batched to end of frame; `update_internals()` forces them now and is expensive.

```gdscript
extends TileMapLayer

var burnt: Dictionary[Vector2i, bool] = {}

func _use_tile_data_runtime_update(coords: Vector2i) -> bool:
	return burnt.has(coords)

func _tile_data_runtime_update(_coords: Vector2i, tile_data: TileData) -> void:
	tile_data.modulate = Color(0.3, 0.3, 0.3)

func burn(coords: Vector2i) -> void:
	burnt[coords] = true
	notify_runtime_tile_data_update()
```

### Scene tiles are nodes, not tiles

A `TileSetScenesCollectionSource` instantiates one scene per placed cell, deferred to end of frame. Use it for a few interactive objects; the placed scenes are not editable nodes. Since 4.6 scene tiles rotate in 90 degree steps. For hundreds of decorations use atlas tiles, tile animation (`TileSetAtlasSource.set_tile_animation_mode()` with `TILE_ANIMATION_MODE_RANDOM_START_TIMES` avoids synced flicker) or a MultiMesh. Since 4.7 the 2D editor's Scene Paint Mode scatters real nodes off the grid, with no TileSet setup.

### Cut overdraw with 2D meshes

A sprite draws its whole quad, transparent pixels included. For large or stacked images (parallax layers, foliage) use Sprite2D > Convert to MeshInstance2D so only the opaque outline is rasterized. `MultiMeshInstance2D` does the same for thousands of copies in one item: fill rate goes down, vertex count goes up.

### Custom drawing at scale

One `draw_multiline()` or `draw_polyline()` call beats many `draw_line()` calls. `antialiased` lines build extra geometry, so they cost per redraw, not per frame. Redraw only on change. For a looping background animation, `draw_animation_slice()` lets the server switch command ranges over time with no redraw. `RenderingServer.canvas_item_set_custom_rect()` gives a custom culling rect and lowers CPU cost when one item draws a lot.

### Screen reads and BackBufferCopy

The first item in draw order that samples a `hint_screen_texture` uniform triggers a full-screen copy. Later items reuse that copy, so two overlapping screen-reading effects do not see each other. Put a `BackBufferCopy` between them. `COPY_MODE_RECT` copies only its rect and is the cheap choice; reading outside the copied rect returns garbage. A mipmap filter on the uniform makes the engine build blur mips for you: handy for frosted glass, not free.

### Group opacity and masks

`CanvasGroup` draws its children to the back buffer and then as one item, so overlapping translucent parts fade as one. Cost: a buffer copy sized by the children rect plus `fit_margin` and `clear_margin`; keep both small and leave `use_mipmaps` off. `clip_children` masks children by the parent's alpha through the same path. Neither can nest inside the other. For a plain rectangle clip, `Control.clip_contents` (a scissor) is far cheaper.

### 2D SDF for effects, not only shadows

`texture_sdf()`, `texture_sdf_normal()`, `sdf_to_screen_uv()` and `screen_uv_to_sdf()` work in `fragment()` and `light()` of canvas_item shaders. Uses: soft contact glow, finite-length directional shadows (the built-in ones are always infinite), cheap 2D ambient occlusion, and `GPUParticles2D` collision. Tune `rendering/2d/sdf/scale` (default half resolution) and `rendering/2d/sdf/oversize` (raise it if particles fall through occluders near screen edges). Distances are in global canvas units. The core pattern is circle tracing: step along a direction by the distance just read, so empty space costs few samples. This gives long drop shadows and light shafts that a blur shader could not afford.

```gdshader
shader_type canvas_item;
render_mode unshaded;
uniform vec2 dir = vec2(0.7, 0.7);
uniform float max_dist = 300.0;

void fragment() {
	vec2 at = screen_uv_to_sdf(SCREEN_UV);
	float travelled = 0.0;
	for (int i = 0; i < 48 && travelled < max_dist; i++) {
		float d = texture_sdf(at);
		if (d < 0.01) {
			break;
		}
		travelled += d;
		at += d * normalize(dir);
	}
	COLOR = vec4(0.0, 0.0, 0.0, 0.6 * (1.0 - min(1.0, travelled / max_dist)));
}
```

### Lights that cost less

Cost grows with pixels covered: `PointLight2D.texture_scale` matters more than light count. `SHADOW_FILTER_PCF13` is for a few lights only. Limit reach with `range_item_cull_mask`, `shadow_item_cull_mask` and `DirectionalLight2D.max_distance`. For bullets and explosions use a Sprite2D with a `CanvasItemMaterial` in Add blend mode: no shadows, no normal maps, wrong in full darkness, but it skips the light path. For blocky pixel-art lighting, snap `LIGHT_VERTEX` and `SHADOW_VERTEX` to a grid in `fragment()`; nearest filtering does not do this.

### Pixel-perfect output

- `display/window/stretch/mode = viewport` renders at the base size and scales the result; `canvas_items` renders at window resolution (sharp text, sub-pixel sprite motion).
- `display/window/stretch/scale_mode = integer` floors the scale to a whole number.
- `rendering/2d/snap/snap_2d_transforms_to_pixel` rounds item positions; `snap_2d_vertices_to_pixel` rounds only final vertices. Both trade smooth motion for crispness and fight `Camera2D` position smoothing.
- Set `rendering/textures/canvas_textures/default_texture_filter` to Nearest once, not per node.

### Parallax2D rules

`Parallax2D` (4.3) replaces ParallaxBackground and ParallaxLayer; the old pair's context menu converts them. It is a plain `Node2D`, not a `CanvasLayer`, so it can sit in the same layer as a screen-reading mask, inside a `CanvasGroup` or after a `BackBufferCopy`, and it survives camera rotation. Put it in a `CanvasLayer` only if it must ignore zoom, and then make `follow_viewport` match the layer's `follow_viewport_enabled`. Textures must start at (0, 0) and extend right and down; `repeat_size` loops that canvas. Use `repeat_times` only to cover camera zoom-out; it repeats every child, so for one small sprite use `texture_repeat` plus a larger `region_rect` instead. `autoscroll` is free when unused. For split screen, clone the parallax nodes per `SubViewport` and separate them with `CanvasItem.visibility_layer` on a common parent and `Viewport.canvas_cull_mask`: a parent outside the mask hides all its children.

## Niche but useful

- `CanvasTexture`: diffuse + normal + specular in one resource, for any 2D node, with its own filter and repeat override.
- `Viewport.use_hdr_2d`: RGBA16 linear 2D buffer for glow over 1.0. Not on Compatibility.
- `Light2D.BLEND_MODE_SUB` negative lights; `light_only` render mode shows a sprite only where lit.
- `TileSetAtlasSource.use_texture_padding` (on by default) is what stops seams between tiles.
- `TileMapLayer.tile_map_data`: the whole layer as bytes, for fast save and copy.
- `DrawableTexture2D` (4.7): blit textures into a texture on the GPU for paint, trails, fog of war or damage masks, without a SubViewport.
- `draw_ellipse()` and `draw_ellipse_arc()` (4.6); `GradientTexture2D.FILL_CONIC` (4.7) for radial sweeps and faux-3D shading ramps.
- `_update_cells()` (4.4) tells a TileMapLayer script which cells changed.
- `VisibleOnScreenEnabler2D` stops processing of off-screen subtrees in big worlds.
- `REGION_RECT` in canvas shaders: `UV` covers the whole atlas, not the sprite region.

## Traps

- MSAA 2D only smooths geometry edges and texture borders; not pixel art interiors, not shader aliasing, not fonts. It does not exist on Compatibility.
- The background clear color receives no 2D light. Add a sprite to light.
- TileMapLayer built-in navigation gives poor paths at scale; bake a `NavigationRegion2D`.
- Overriding `_update_cells()` slows every layer update.

## Version notes

- 4.3: `Parallax2D`.
- 4.4: 2D batching on Forward+ and Mobile; canvas `instance uniform`; `_update_cells()`; several occluder polygons per tile; `TileMapLayer.occlusion_enabled`.
- 4.5: TileMapLayer physics chunking on by default; `get_coords_for_body_rid()` changed meaning.
- 4.6: 2D renderer redesign for GPU cost (1.1x to 7x, GPU-bound); rotatable scene tiles; ellipse drawing.
- 4.7: `DrawableTexture2D`; Scene Paint Mode; Polygon2D fast path; line drawing no longer adds an antialiasing feather, so old lines look thinner. New projects default to stretch mode `canvas_items` and aspect `expand`.

## Related skills

- `godot-2d-essentials`: step-by-step TileMapLayer, parallax, lights and drawing setup.
- `godot-optimization`: profiler use, MultiMesh and server-side canvas items.
- `godot-perf-measure`: trustworthy A/B frame-cost numbers and pixel-art camera judder.
- `godot-pixel-art-pipeline`: making the art itself to an exact grid and palette.
- Sibling shelves: `shaders-advanced`, `particles-and-vfx`, `rendering-pipeline`.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/2d/2d_transforms.html
- https://docs.godotengine.org/en/stable/tutorials/2d/custom_drawing_in_2d.html
- https://docs.godotengine.org/en/stable/tutorials/2d/2d_lights_and_shadows.html
- https://docs.godotengine.org/en/stable/tutorials/2d/2d_antialiasing.html
- https://docs.godotengine.org/en/stable/tutorials/2d/2d_parallax.html
- https://docs.godotengine.org/en/stable/tutorials/2d/using_tilemaps.html
- https://docs.godotengine.org/en/stable/tutorials/shaders/screen-reading_shaders.html
- https://docs.godotengine.org/en/stable/tutorials/shaders/shader_reference/canvas_item_shader.html
- https://docs.godotengine.org/en/stable/tutorials/performance/pipeline_compilations.html
- https://docs.godotengine.org/en/stable/engine_details/architecture/internal_rendering_architecture.html
- https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.5.html
- https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.7.html
- https://docs.godotengine.org/en/stable/classes/class_tilemaplayer.html
- https://godotengine.org/releases/4.4/ , https://godotengine.org/releases/4.5/ , https://godotengine.org/releases/4.6/ , https://godotengine.org/releases/4.7/
- https://godotengine.org/article/dev-snapshot-godot-4-4-dev-3/ (batching on RD renderers)
- https://godotengine.org/article/dev-snapshot-godot-4-6-dev-5/ (2D renderer cost numbers)
- https://godotengine.org/article/dev-snapshot-godot-4-5-dev-1/ (chunk tilemap physics)
- https://godotengine.org/article/parallax-progress-report/
- https://godotengine.org/article/godots-2d-engine-gets-several-improvements-upcoming-40/
