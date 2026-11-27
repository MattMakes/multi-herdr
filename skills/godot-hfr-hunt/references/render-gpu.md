# Render thread and GPU

Use this page when the hot spot is the render thread or the GPU at 200 to
300 fps (5.00, 4.17 and 3.33 ms budgets). The rank is the expected gain when
the named bottleneck exists. The source report read the Godot 4.7
documentation and source with `4.7.2.stable.official.ed1daf0bf`. No project
GPU benchmark was run.

## Ranked techniques

### 1. Choose the cheapest renderer that supplies the required look

- **What:** Benchmark Forward+, Mobile and Compatibility with the real scene. Ship the cheapest renderer that still gives every required feature.
- **Why/thread:** Render thread and GPU. The renderer changes lighting, the depth prepass, post-processing and the shader architecture. Mobile avoids clustered compute and the depth prepass. Compatibility targets older hardware and omits many high-end effects, but shadowed lights scale badly because they need extra passes.
- **Measure:** Run the same fixed camera path, resolution and quality settings under each renderer. Compare Visual Profiler render CPU and GPU categories, total viewport GPU time, draw calls and p50, p95 and p99 frame time. Log `RenderingServer.get_current_rendering_method()` and `get_current_rendering_driver_name()`.
- **Version:** Godot 4.7. The available features differ per renderer.
- **Source:** [Renderer overview](https://docs.godotengine.org/en/4.7/tutorials/rendering/renderers.html) and [internal rendering architecture](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#rendering-methods).
- **Evidence:** Verified from the official 4.7 documentation.
- **Trap:** A cheaper renderer can need material rework or remove required GI or post effects. Compatibility is not always fastest on a modern GPU. The web export requires it.

### 2. Lower the 3D render scale and choose the upscaler by net cost

- **What:** Set `Viewport.scaling_3d_scale` below `1.0` and choose bilinear, FSR 1 or FSR 2 by its quality and its own GPU cost on the target hardware.
- **Why/thread:** GPU. Fewer shaded pixels lower fill, lighting, GI and most post-effect cost. FSR 2.2 gives higher quality and temporal antialiasing, but costs more than FSR 1 or bilinear. A very slow GPU can gain more from a simpler mode.
- **Measure:** Sweep scales such as `1.0`, `0.85`, `0.75` and `0.67` at a fixed output resolution. Compare Visual Profiler GPU categories and summed viewport GPU time. A large gain shows a pixel-bound scene.
- **Version:** Godot 4.7. FSR 2.2 and FSR 1 are Forward+ only. Bilinear works in every renderer.
- **Source:** [Resolution scaling](https://docs.godotengine.org/en/4.7/tutorials/3d/resolution_scaling.html) and [Viewport scaling modes](https://docs.godotengine.org/en/4.7/classes/class_viewport.html#enum-viewport-scaling3dmode).
- **Evidence:** Verified from the official documentation.
- **Trap:** At low resolutions the upscaler cost can erase the saving. FSR 2 owns temporal AA, so a separate TAA setting is ignored, and motion-vector errors cause ghosting. Godot 4.7 has no built-in dynamic resolution controller.

### 3. Prefer LightmapGI for mostly static scenes

- **What:** Bake static direct and indirect light with `LightmapGI` instead of paying for SDFGI or VoxelGI every frame.
- **Why/thread:** GPU. Lightmaps move GI work out of the run time and give the lowest run-time GI cost in Godot. Baked direct light can also remove real-time light and shadow work.
- **Measure:** Capture the GI and shadow categories in the Visual Profiler before and after, plus viewport GPU time and shadow draw calls. Check that the image matches on the same camera path.
- **Version:** Godot 4.7. Lightmaps render in every renderer. Baking needs Forward+ or Mobile and the editor.
- **Source:** [LightmapGI class](https://docs.godotengine.org/en/4.7/classes/class_lightmapgi.html) and [using LightmapGI](https://docs.godotengine.org/en/4.7/tutorials/3d/global_illumination/using_lightmap_gi.html).
- **Evidence:** Verified from the official documentation.
- **Trap:** Light and level geometry become static. Baking needs UV2 and storage. A level generated at run time cannot be baked in an export.

### 4. Remove or constrain real-time shadow casters first

- **What:** Turn off shadows on lights and geometry that do not visibly need them, use light distance fade, and shorten the directional shadow distance.
- **Why/thread:** Render thread and GPU. Each shadow-casting light renders its geometry into shadow maps and samples them later. This costs render CPU, draw calls, vertex work, bandwidth and GPU filtering. Compatibility can add passes for shadowed lights.
- **Measure:** Compare `Viewport.RENDER_INFO_TYPE_SHADOW` draw calls, the Visual Profiler shadow CPU and GPU categories and viewport GPU time. Sweep `DirectionalLight3D.directional_shadow_max_distance` and the visible shadowed-light count.
- **Version:** Godot 4.7. Light limits and the shadow method differ per renderer.
- **Source:** [3D lights and shadows](https://docs.godotengine.org/en/4.7/tutorials/3d/lights_and_shadows.html) and [GPU optimization](https://docs.godotengine.org/en/4.7/tutorials/performance/gpu_optimization.html#shadows).
- **Evidence:** Verified from the official documentation.
- **Trap:** A removed caster can leak light or detach characters from the scene. A long fade range can keep almost the same cost.

### 5. Disable expensive post effects before tuning their quality

- **What:** Remove SSAO, SSIL, SSR, glow, depth of field, volumetric fog and TAA where they are not needed. Then lower resolution, quality, samples and blur passes for the effects that stay.
- **Why/thread:** GPU. These are extra full-screen or half-screen passes whose cost grows with pixel count. SSAO, SSIL and SSR use half resolution by default in Forward+. TAA needs motion vectors, which run the previous frame's vertex transforms.
- **Measure:** Toggle 1 effect per A/B run and read its Visual Profiler GPU category, total viewport GPU time and its response to the render scale. Measure combinations too, because GPU tasks marked `(Parallel)` can overlap.
- **Version:** Godot 4.7. Forward+ has the full set, and Mobile and Compatibility have subsets. Compatibility got a simplified SSAO in 4.6.
- **Source:** [Environment and post-processing](https://docs.godotengine.org/en/4.7/tutorials/3d/environment_and_post_processing.html) and [screen-space effects and TAA internals](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#screen-space-effects-ssao-ssil-ssr-sss).
- **Evidence:** Verified from the official documentation and the engine source links on the architecture page.
- **Trap:** GPU passes can overlap, so category times do not always add. Without TAA the aliasing changes. SSR reflects only opaque content that is on screen.

### 6. Use imported mesh LOD and tune its screen-space threshold

- **What:** Generate mesh LODs at scene import. Raise `Viewport.mesh_lod_threshold` or lower the per-object `lod_bias` where the quality allows it.
- **Why/thread:** GPU. Lower-detail surfaces lower vertex throughput, shadow geometry and sometimes memory bandwidth, and keep per-instance culling.
- **Measure:** On a fixed camera pullback, graph `Viewport.RENDER_INFO_PRIMITIVES_IN_FRAME`, shadow primitives, Visual Profiler geometry and shadow GPU time and total viewport GPU time. Check the transitions in wireframe.
- **Version:** Godot 4.7. Imported scene formats support generated LODs. Import an OBJ as a scene to get them.
- **Source:** [Mesh LOD](https://docs.godotengine.org/en/4.7/tutorials/3d/mesh_lod.html) and [automatic mesh LOD](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#automatic-mesh-lod).
- **Evidence:** Verified from the official documentation, which points to `scene/resources/3d/importer_mesh.cpp` and `servers/rendering/renderer_scene_cull.cpp`.
- **Trap:** LOD does not lower the draw-call count. An aggressive threshold loses silhouettes. A CPU-bound scene can show no frame-time gain.

### 7. Build HLOD with visibility ranges

- **What:** Replace many distant objects with 1 merged low-detail mesh or impostor, with `GeometryInstance3D` visibility begin and end ranges and dependencies.
- **Why/thread:** Render thread and GPU. HLOD lowers object processing, draw calls, vertex count and material work at distance, and keeps fine culling nearby.
- **Measure:** Drive across the transition distances and compare visible object, draw-call and primitive counts and render CPU and GPU time. Use the disabled fade mode with margins to measure hysteresis without transparency cost.
- **Version:** Godot 4.7.
- **Source:** [Visibility ranges](https://docs.godotengine.org/en/4.7/tutorials/3d/visibility_ranges.html).
- **Evidence:** Verified from the official documentation.
- **Trap:** The alpha fade modes render transparent objects for a short time and can cost more than an instant hysteresis switch. 1 giant merged mesh breaks nearby frustum and occlusion culling.

### 8. Add occlusion culling only where occluders reject much work

- **What:** Bake or author `OccluderInstance3D` geometry for indoor or blocked-view scenes, and enable occlusion culling on the viewport.
- **Why/thread:** Render thread and GPU. The CPU rasterizes a low-resolution occlusion buffer. Hidden instances then cost no render CPU, draw calls, vertices or GPU shading.
- **Measure:** Compare visible objects, draw calls, primitives, frame setup and render CPU and GPU time with occlusion on and off along the same camera path. Inspect rejected instances in the editor occlusion debug view.
- **Version:** Godot 4.7. The implementation uses Embree and does not support truly dynamic occluder geometry.
- **Source:** [Occlusion culling](https://docs.godotengine.org/en/4.7/tutorials/3d/occlusion_culling.html) and [occlusion architecture](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#occlusion-culling).
- **Evidence:** Verified from the official documentation and its source pointers to `servers/rendering/renderer_scene_occlusion_cull.*`.
- **Trap:** In open scenes or with weak occluders, culling can lose CPU time. Forward+ already has a depth prepass, so its shading gain is smaller than on Mobile.

### 9. Use MultiMesh for many copies, split into spatial chunks

- **What:** Submit repeated meshes through `MultiMesh`, update moving instances with 1 whole buffer, and split wide populations into cullable spatial cells.
- **Why/thread:** Main thread and render thread. 1 MultiMesh cuts submissions and draw calls. A bulk buffer update lowers API overhead and allows parallel preparation.
- **Measure:** Compare visible draw calls, objects, frame setup CPU, viewport render CPU and GPU time against individual nodes. Sweep the chunk size and the camera position.
- **Version:** Godot 4.7.
- **Source:** [Using MultiMesh](https://docs.godotengine.org/en/4.7/tutorials/performance/using_multimesh.html) and [MultiMesh class](https://docs.godotengine.org/en/4.7/classes/class_multimesh.html).
- **Evidence:** Verified from the official documentation.
- **Trap:** A MultiMesh is culled as 1 object, so a world-sized batch draws every instance. Instances have no per-node scripts or collision and need data-oriented game state.

### 10. Merge compatible static meshes and material surfaces selectively

- **What:** Combine nearby static meshes that are always visible together and share a material. Collapse avoidable material surfaces in the DCC tool or the import pipeline.
- **Why/thread:** Render thread. A static merge can beat automatic Vulkan instancing because it lowers objects, state changes and draw calls. Shared materials lower pipeline and material churn.
- **Measure:** Compare draw calls, visible objects, primitives, frame setup CPU and viewport CPU and GPU time before and after each merge region. Test from viewpoints that see only part of the region.
- **Version:** Godot 4.7. Forward+ instances identical opaque and alpha-tested objects automatically, but not alpha-blended objects.
- **Source:** [Batching and instancing](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#batching-and-instancing) and [GPU optimization](https://docs.godotengine.org/en/4.7/tutorials/performance/gpu_optimization.html#draw-calls).
- **Evidence:** Verified from the official documentation.
- **Trap:** Too much merging adds overdraw and destroys frustum, occlusion and HLOD granularity. Different materials still need separate surfaces and draws.

### 11. Keep one material and vary instances with instance uniforms

- **What:** Declare `instance uniform` values and set them on the `GeometryInstance3D` or `CanvasItem`. Do not duplicate `ShaderMaterial` resources.
- **Why/thread:** Render thread and GPU. Material reuse lowers material changes and unique GPU state, and keeps per-object color or scalar variation.
- **Measure:** Compare the material resource count, pipeline-compilation monitor deltas, draw calls, frame setup and render CPU and GPU time with duplicated materials against instance uniforms.
- **Version:** Godot 4.7. A shader has at most 16 instance uniforms, with index rules for multi-material meshes. Compatibility cannot index sampler arrays dynamically with instance uniforms.
- **Source:** [Per-instance uniforms](https://docs.godotengine.org/en/4.7/tutorials/shaders/shader_reference/shading_language.html#per-instance-uniforms) and [ShaderMaterial](https://docs.godotengine.org/en/4.7/classes/class_shadermaterial.html).
- **Evidence:** Verified from the official documentation.
- **Trap:** Instance uniforms do not give arbitrary per-instance textures. A multi-surface mesh needs the same uniform names, types and indices on each surface.

### 12. Preserve automatic 2D batches and reduce transparent fill

- **What:** Keep compatible sprites, glyphs and canvas primitives consecutive with shared textures and materials. Atlas the textures, and convert large mostly transparent sprites to tight `MeshInstance2D` geometry.
- **Why/thread:** Render thread and GPU. Godot 4.7 batches compatible 2D work in every renderer, which lowers submissions and draw calls. Tight geometry does not shade transparent pixels, which saves GPU fill.
- **Measure:** The Visual Profiler 2D categories and the viewport visible draw calls, at fixed output resolutions. If the cost follows the resolution but not the draw calls, fill is the likely limit.
- **Version:** Automatic batching in the RD backend arrived in Godot 4.4 and was redesigned in 4.6. This page targets 4.7.
- **Source:** [2D rendering techniques](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#d-rendering-techniques), [2D meshes](https://docs.godotengine.org/en/4.7/tutorials/2d/2d_meshes.html) and the [4.6 renderer optimization announcement](https://godotengine.org/article/dev-snapshot-godot-4-6-dev-5/).
- **Evidence:** Verified from the official documentation and the release announcement.
- **Trap:** Texture, material, primitive, clip and order changes split batches. `CanvasGroup` is not the automatic batcher and can add backbuffer cost. Extra mesh triangles can lose when transparent fill was not the bottleneck.

### 13. Tune shadow map size, atlas allocation, cascades and filters

- **What:** Lower directional and positional shadow resolution, choose the atlas quadrant subdivision for the visible light sizes, use fewer directional cascades and lower the soft-shadow filter.
- **Why/thread:** GPU. Smaller maps lower raster bandwidth, sampling bandwidth and VRAM. Fewer cascades mean fewer shadow draws. Cheaper filters take fewer fragment samples. 16-bit maps can be faster on some devices.
- **Measure:** Read shadow draw calls and Visual Profiler shadow GPU time while you sweep 1 setting. Check atlas occupancy and quality with near and far cameras. Compare the orthogonal, 2-split and 4-split directional modes.
- **Version:** Godot 4.7. PCSS from a nonzero directional angular distance is Forward+ only.
- **Source:** [Shadow atlas](https://docs.godotengine.org/en/4.7/tutorials/3d/lights_and_shadows.html#shadow-atlas), [DirectionalLight3D modes](https://docs.godotengine.org/en/4.7/classes/class_directionallight3d.html) and [Light3D angular distance](https://docs.godotengine.org/en/4.7/classes/class_light3d.html#class-light3d-property-light-angular-distance).
- **Evidence:** Verified from the official documentation.
- **Trap:** A small or over-subdivided atlas blurs shadows or makes them pop. 4 cascades and split blending look better but cost more. Soft PCSS can cost too much for a 3.33 ms budget.

### 14. If real-time GI is mandatory, lower its resolution and update rate

- **What:** Enable half-resolution GI. Tune SDFGI cascades, cell size, rays and update rate, or VoxelGI subdivision and quality, to the lowest acceptable level.
- **Why/thread:** GPU. SDFGI and VoxelGI are among the heaviest GPU features in Godot. Half resolution gives a quarter of the GI buffer pixels. Lower spatial, sample and update quality lowers compute and bandwidth.
- **Measure:** Visual Profiler SDFGI and VoxelGI GPU categories and viewport GPU time, in static scenes and with moving lights. Compare half and full resolution and each quality step alone.
- **Version:** Godot 4.7. Both are Forward+ only.
- **Source:** [SDFGI](https://docs.godotengine.org/en/4.7/tutorials/3d/global_illumination/using_sdfgi.html), [VoxelGI](https://docs.godotengine.org/en/4.7/classes/class_voxelgi.html) and [GI architecture](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#global-illumination).
- **Evidence:** Verified from the official documentation.
- **Trap:** Half resolution aliases thin details. SDFGI is poor on low-end GPUs and has no dynamic occluders. VoxelGI needs baked static geometry coverage and can stall at high subdivision.

### 15. Simplify shaders and reduce transparent overdraw

- **What:** Remove unused material features, texture samples, dynamic branches and layered alpha. Use opaque or alpha-tested geometry when the look allows it.
- **Why/thread:** GPU. A fragment shader runs per covered sample, so its cost multiplies with resolution and overdraw. A dynamic branch can raise register use even when all pixels take the same branch. Alpha-blended surfaces cannot use normal opaque instancing and must shade each overlapping layer.
- **Measure:** Isolate the material in the Visual Profiler, compare viewport GPU time at 2 resolutions, and inspect a RenderDoc frame for draw count, shader duration, texture fetches and overdraw. Confirm per pass with `--gpu-profile`.
- **Version:** Godot 4.7. Shader and renderer features differ.
- **Source:** [Shaders and materials](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#shaders-and-materials) and [transparency and blending](https://docs.godotengine.org/en/4.7/tutorials/performance/gpu_optimization.html#transparency-and-blending).
- **Evidence:** Verified from the official documentation and the linked core shader source.
- **Trap:** Compile-time variants in place of run-time branches can multiply pipelines and compilation work. Alpha scissor can alias, and alpha hash or dither can add noise.

### 16. Make pipeline precompilation see every run-time feature early

- **What:** Instantiate each dynamic 3D effect once during loading, expose rendering features before bulk asset loading, watch the pipeline compilation monitors, and enable the export shader baker where it is supported.
- **Why/thread:** Render thread. Ubershaders give a usable pipeline at once while optimized specializations compile in the background. Early instancing moves unavoidable surface and pipeline work out of gameplay. The shader baker skips the source-to-intermediate compile.
- **Measure:** Watch the Canvas, Mesh, Surface, Draw and Specialization pipeline-compilation monitors while you replay a cold first-use path. Count p99, worst frames and frames over the hitch threshold. A `Draw` increment during play is a defect to remove.
- **Version:** Ubershaders and pipeline precompilation are Godot 4.4 and later on Forward+ and Mobile. The shader baker is 4.5 and later on Forward+ and Mobile. Compatibility has no ubershaders and needs a real 1-frame draw.
- **Source:** [Pipeline compilations](https://docs.godotengine.org/en/4.7/tutorials/performance/pipeline_compilations.html) and [jitter and stutter](https://docs.godotengine.org/en/4.7/tutorials/rendering/jitter_stutter.html#stutter).
- **Evidence:** Verified from the official documentation.
- **Trap:** `preload()` alone does not expose every node and material-override pipeline. The shader baker cannot bake the final hardware and driver pipeline, and it mostly helps startup. Hidden warm-up nodes must not run gameplay scripts.

### 17. Treat the separate render-thread model as an experimental A/B option

- **What:** Test `rendering/driver/threads/thread_model = 2` (`Separate`) only when render setup blocks the main thread and the target platforms pass a full stress matrix.
- **Why/thread:** Main thread and render thread. A separate rendering thread can overlap some main-thread and render work. Synchronization can add jitter, and the GPU cost stays the same.
- **Measure:** Compare process and main-thread time, `RenderingServer.get_frame_setup_time_cpu()`, summed viewport render CPU, p95 and p99 frame time, and crash results. Stress particles, window resize, scene churn and graphics-setting changes.
- **Version:** Godot 4.7. The project setting is still experimental, with documented known bugs.
- **Source:** [ProjectSettings thread model](https://docs.godotengine.org/en/4.7/classes/class_projectsettings.html#class-projectsettings-property-rendering-driver-threads-thread-model) and [thread-safe rendering APIs](https://docs.godotengine.org/en/4.7/tutorials/performance/thread_safe_apis.html#rendering).
- **Evidence:** Verified from the official documentation.
- **Trap:** The setting can crash, most of all around particles or resize, and synchronization can make frame pacing worse. Never call GPU APIs from arbitrary worker threads, because they can force a RenderingServer synchronization.

### 18. Use a layered GPU timing workflow that includes all viewports

- **What:** Start with the Visual Profiler, automate CPU and GPU viewport timings, and use RenderDoc only to inspect the pass or draw that you already found to be expensive.
- **Why/thread:** Render thread and GPU. Measurement saves no time itself, but it stops work on main or physics code when the renderer or GPU owns the budget. `get_frame_setup_time_cpu()` captures global render setup, and the viewport methods capture each viewport.
- **Measure:** Call `RenderingServer.viewport_set_measure_render_time(rid, true)` for each viewport. Render CPU is `RenderingServer.get_frame_setup_time_cpu()` plus `viewport_get_measured_render_time_cpu()` for every drawn viewport. Sum the measured GPU time of every viewport apart from that. Correlate with the Visual Profiler and an uncapped release-export wall-time sample. Launch with `--gpu-profile`. For source-level RenderDoc shader debugging on a debug build, add `--generate-spirv-debug-info`.
- **Version:** Godot 4.7. OpenGL and headless runs have no `RenderingDevice`, and GPU timers can be missing on a backend or platform. The Visual Profiler is not available for Compatibility on macOS.
- **Source:** [Visual Profiler](https://docs.godotengine.org/en/4.7/tutorials/scripting/debug/debugger_panel.html#visual-profiler), [RenderingServer timing](https://docs.godotengine.org/en/4.7/classes/class_renderingserver.html#class-renderingserver-method-get-frame-setup-time-cpu), [command-line GPU options](https://docs.godotengine.org/en/4.7/tutorials/editor/command_line_tutorial.html) and [RenderDoc](https://renderdoc.org/docs/).
- **Evidence:** Verified from the official Godot and RenderDoc documentation.
- **Trap:** The Visual Profiler and GPU capture add overhead. GPU pass times can overlap, so do not sum them blindly. A capped GPU can downclock and report misleading GPU time. A root-only measurement misses SubViewport work.
