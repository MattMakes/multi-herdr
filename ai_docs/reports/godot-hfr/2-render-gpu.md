# Godot 4.7 high-frame-rate research: rendering and GPU

## Scope and basis

This report covers render-thread and GPU work for a 200–300 fps target. The total frame budget is 5.00 ms at 200 fps, 4.17 ms at 240 fps, and 3.33 ms at 300 fps. Rankings are expected impact when the named bottleneck exists, not universal promises.

Revision examined: repository commit `39363ae342f0eae0c41c346aa12239f7381b686c`; installed engine `4.7.2.stable.official.ed1daf0bf`. Evidence marked **verified** means the Godot 4.7 official documentation, class reference, or linked engine-source path was read. No project-specific GPU benchmark was run because this task has no game project or representative scene.

Existing skills already cover uncapped repeatable measurements, the Visual Profiler, `--gpu-profile`, viewport render statistics, MultiMesh basics, and shader warm-up. The techniques below add missing detail or correct existing advice.

## Corrections to existing skills

1. `skills/godot-optimization/SKILL.md:98`, `:132`, and `:164`, plus `skills/godot-optimization/references/draw-calls.md:11-20`, incorrectly prescribe `CanvasGroup` as the general 2D batcher. Godot 4.7 automatically batches compatible 2D work in every renderer. `CanvasGroup` is a backbuffer-compositing node for treating children as one visual object; it uses a screen-reading shader and can add buffer and composite cost. Keep compatible sprites/text consecutive and measure draw calls; use `CanvasGroup` only for its compositing semantics. Sources: [internal rendering architecture](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#d-rendering-techniques), [CanvasGroup](https://docs.godotengine.org/en/4.7/classes/class_canvasgroup.html).
2. `skills/godot-optimization/references/draw-calls.md:33-40` and `:54-61` label duplicated `ShaderMaterial`s as shared. Each example calls `duplicate()`. Use `instance uniform` plus `set_instance_shader_parameter()` to preserve material reuse; duplicate only when an instance uniform cannot represent the difference. Sources: [per-instance uniforms](https://docs.godotengine.org/en/4.7/tutorials/shaders/shader_reference/shading_language.html#per-instance-uniforms), [ShaderMaterial](https://docs.godotengine.org/en/4.7/classes/class_shadermaterial.html).
3. `skills/godot-optimization/references/draw-calls.md:65` says `GeometryInstance3D.gi_mode = GI_MODE_STATIC` lets the engine merge static geometry. That property opts geometry into baked/dynamic GI; it does not merge meshes. Merge compatible static geometry in the asset pipeline or author an HLOD mesh, and retain smaller pieces where culling matters. Sources: [GeometryInstance3D](https://docs.godotengine.org/en/4.7/classes/class_geometryinstance3d.html), [visibility ranges](https://docs.godotengine.org/en/4.7/tutorials/3d/visibility_ranges.html).
4. `skills/godot-optimization/references/draw-calls.md:168-198` uses `VisibleOnScreenNotifier3D` and a per-frame script as a distance-LOD mechanism. The built-in mechanisms are imported mesh LOD and `GeometryInstance3D` visibility ranges; the latter also supports HLOD dependencies and hysteresis without script work. Source: [mesh LOD](https://docs.godotengine.org/en/4.7/tutorials/3d/mesh_lod.html), [visibility ranges](https://docs.godotengine.org/en/4.7/tutorials/3d/visibility_ranges.html).

## Ranked techniques

### 1. Choose the cheapest renderer that supplies the required look

**What:** Benchmark Forward+, Mobile, and Compatibility with the actual scene and ship the least expensive renderer that still provides every required feature.

**Why/thread:** Renderer choice changes lighting, depth-prepass, post-processing, and shader architecture, affecting render CPU and GPU time. Mobile avoids clustered compute and the depth prepass; Compatibility targets older hardware and omits many high-end effects, but shadowed lights scale poorly because they need extra passes.

**Measure:** Run the same fixed camera path, resolution, and quality settings under each supported rendering method. Compare Visual Profiler render CPU/GPU categories, total viewport GPU time, draw calls, and p50/p95/p99 frame time. Log `RenderingServer.get_current_rendering_method()` and `get_current_rendering_driver_name()`.

**Version:** Godot 4.7; feature availability differs by renderer.

**Source:** [renderer overview](https://docs.godotengine.org/en/4.7/tutorials/rendering/renderers.html), [internal rendering architecture](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#rendering-methods).

**Evidence:** **Verified** from official 4.7 architecture and renderer documentation.

**Trap:** A cheaper renderer can require material rework or remove required GI/post effects. Compatibility is not automatically fastest on every modern GPU, and web requires it.

### 2. Lower 3D render scale; select bilinear, FSR 1, or FSR 2 by measured net cost

**What:** Reduce `Viewport.scaling_3d_scale` below `1.0` and choose the upscaler whose quality and own GPU cost fit the target hardware.

**Why/thread:** Fewer shaded pixels reduce GPU fill, lighting, GI, and most post-effect cost. FSR 2.2 has higher quality and temporal antialiasing but costs more than FSR 1 or bilinear; a very slow GPU may gain more from the simpler mode.

**Measure:** Sweep scale values such as `1.0`, `0.85`, `0.75`, and `0.67` at a fixed output resolution. Compare Visual Profiler GPU categories and summed viewport GPU time. A large gain identifies a pixel-bound scene.

**Version:** Godot 4.7; FSR 2.2 is Forward+ only. FSR 1 is Forward+ only; bilinear is available everywhere.

**Source:** [resolution scaling](https://docs.godotengine.org/en/4.7/tutorials/3d/resolution_scaling.html), [Viewport scaling modes](https://docs.godotengine.org/en/4.7/classes/class_viewport.html#enum-viewport-scaling3dmode).

**Evidence:** **Verified** from official documentation.

**Trap:** Upscaler overhead can erase the saving at low resolutions. FSR 2 owns temporal AA, so enabling TAA separately is ignored; motion-vector errors cause ghosting. Godot 4.7 has no built-in dynamic resolution controller.

### 3. Prefer LightmapGI for mostly static scenes

**What:** Bake static direct and indirect lighting with `LightmapGI` instead of paying for SDFGI or VoxelGI every frame.

**Why/thread:** Lightmaps move GI work out of runtime and provide Godot's lowest runtime GI cost, chiefly saving GPU time; baking direct light can also remove real-time light and shadow work.

**Measure:** Capture the GI and shadow categories in the Visual Profiler before and after, plus viewport GPU time and shadow draw calls. Check visual parity with the same camera path.

**Version:** Godot 4.7. Lightmap rendering works in all renderers; baking requires Forward+ or Mobile and the editor.

**Source:** [LightmapGI class](https://docs.godotengine.org/en/4.7/classes/class_lightmapgi.html), [using LightmapGI](https://docs.godotengine.org/en/4.7/tutorials/3d/global_illumination/using_lightmap_gi.html).

**Evidence:** **Verified** from official documentation.

**Trap:** Lighting and level geometry become static, baking needs UV2 and storage, and runtime-generated levels cannot be baked in an export.

### 4. Remove or constrain real-time shadow casters first

**What:** Disable shadows on lights and geometry that do not visibly need them, use light distance fade, and shorten directional shadow distance.

**Why/thread:** Every shadow-casting light renders affected geometry into shadow maps and later samples them, costing render CPU, draw calls, vertex work, bandwidth, and GPU filtering. Compatibility can add passes for shadowed lights.

**Measure:** Compare `Viewport.RENDER_INFO_TYPE_SHADOW` draw calls, Visual Profiler shadow CPU/GPU categories, and viewport GPU time. Sweep `DirectionalLight3D.directional_shadow_max_distance` and visible shadowed-light count.

**Version:** Godot 4.7; exact light limits and shadow implementation vary by renderer.

**Source:** [3D lights and shadows](https://docs.godotengine.org/en/4.7/tutorials/3d/lights_and_shadows.html), [GPU optimization](https://docs.godotengine.org/en/4.7/tutorials/performance/gpu_optimization.html#shadows).

**Evidence:** **Verified** from official documentation.

**Trap:** Removing a caster can produce light leaks or detach characters from the scene. A long fade range can leave nearly the same cost.

### 5. Disable expensive post effects before tuning their quality

**What:** Remove unneeded SSAO, SSIL, SSR, glow, depth of field, volumetric fog, and TAA; then lower resolution, quality, samples, and blur passes for effects that remain.

**Why/thread:** These are extra full- or half-screen GPU passes whose cost grows with pixel count. SSAO, SSIL, and SSR default to half resolution in Forward+, while TAA requires motion-vector generation that runs previous-frame vertex transforms.

**Measure:** Toggle one effect per A/B run and use its Visual Profiler GPU category, total viewport GPU time, and resolution-scale sensitivity. Measure combinations because GPU tasks marked `(Parallel)` may overlap.

**Version:** Godot 4.7. Forward+ has the full set; Mobile and Compatibility support subsets. Compatibility gained simplified SSAO in 4.6.

**Source:** [environment and post-processing](https://docs.godotengine.org/en/4.7/tutorials/3d/environment_and_post_processing.html), [screen-space effects and TAA internals](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#screen-space-effects-ssao-ssil-ssr-sss).

**Evidence:** **Verified** from official documentation and engine-source links on the architecture page.

**Trap:** GPU passes can overlap, so individual category times are not always additive. Removing TAA changes aliasing, and SSR only reflects on-screen opaque content.

### 6. Use imported mesh LOD and tune its screen-space threshold

**What:** Generate mesh LODs during scene import and raise `Viewport.mesh_lod_threshold` or lower per-object `lod_bias` where quality permits.

**Why/thread:** Lower-detail surfaces reduce GPU vertex throughput, shadow geometry, and sometimes memory bandwidth while retaining per-instance culling.

**Measure:** In a fixed camera pullback, graph `Viewport.RENDER_INFO_PRIMITIVES_IN_FRAME`, shadow primitives, Visual Profiler geometry/shadow GPU time, and total viewport GPU time. Use wireframe to verify transitions.

**Version:** Godot 4.7; imported scene formats support generated LODs, while OBJ must be imported as a scene.

**Source:** [mesh LOD](https://docs.godotengine.org/en/4.7/tutorials/3d/mesh_lod.html), [rendering architecture](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#automatic-mesh-lod).

**Evidence:** **Verified** from official documentation, which points to `scene/resources/3d/importer_mesh.cpp` and `servers/rendering/renderer_scene_cull.cpp`.

**Trap:** This does not reduce draw-call count. Aggressive thresholds can cause silhouette loss, and CPU-bound scenes may show no frame-time gain.

### 7. Build HLOD with visibility ranges

**What:** Replace many distant objects with one merged low-detail mesh or impostor using `GeometryInstance3D` visibility begin/end ranges and dependencies.

**Why/thread:** HLOD reduces render-thread object processing, draw calls, vertex count, and GPU material work at distance while preserving fine-grained culling nearby.

**Measure:** Drive across transition distances and compare visible object, draw-call, and primitive counts plus render CPU/GPU time. Use disabled fade mode with margins to measure hysteresis without transparency cost.

**Version:** Godot 4.7.

**Source:** [visibility ranges](https://docs.godotengine.org/en/4.7/tutorials/3d/visibility_ranges.html).

**Evidence:** **Verified** from official documentation.

**Trap:** Alpha fade modes temporarily render transparent objects and can cost more than an instant hysteresis switch. A single giant merged mesh harms nearby frustum and occlusion culling.

### 8. Add occlusion culling only where occluders reject substantial work

**What:** Bake or author `OccluderInstance3D` geometry for indoor or blocked-view scenes and enable occlusion culling on the viewport.

**Why/thread:** The CPU rasterizes a low-resolution occlusion buffer, then avoids render CPU work, draw calls, vertices, and GPU shading for hidden instances.

**Measure:** Compare visible objects, draw calls, primitives, frame setup/render CPU, and GPU time with occlusion enabled and disabled along the same camera path. Use the editor occlusion debug view to inspect rejected instances.

**Version:** Godot 4.7; the implementation uses Embree and does not support truly dynamic occluder geometry.

**Source:** [occlusion culling](https://docs.godotengine.org/en/4.7/tutorials/3d/occlusion_culling.html), [architecture](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#occlusion-culling).

**Evidence:** **Verified** from official documentation and its source pointers to `servers/rendering/renderer_scene_occlusion_cull.*`.

**Trap:** Open scenes or weak occluders can lose CPU time. Forward+ already uses a depth prepass, so its shading gain is smaller than Mobile's.

### 9. Use MultiMesh for many copies, partitioned into spatial chunks

**What:** Submit repeated meshes through `MultiMesh`, update moving instances with a whole buffer, and split broad populations into cullable spatial cells.

**Why/thread:** One MultiMesh sharply reduces main/render-thread submissions and draw calls; bulk buffer updates reduce API overhead and permit parallel preparation.

**Measure:** Compare visible draw calls, objects, frame setup CPU, viewport render CPU, and GPU time against individual nodes. Sweep chunk size and camera position.

**Version:** Godot 4.7.

**Source:** [using MultiMesh](https://docs.godotengine.org/en/4.7/tutorials/performance/using_multimesh.html), [MultiMesh class](https://docs.godotengine.org/en/4.7/classes/class_multimesh.html).

**Evidence:** **Verified** from official documentation. Existing skills cover the basic API; spatial partitioning and whole-buffer measurement remain essential.

**Trap:** A MultiMesh is culled as one object, so a world-sized batch draws every instance. Instances lose per-node scripts/collision and require data-oriented gameplay state.

### 10. Merge compatible static meshes and material surfaces selectively

**What:** Combine nearby, always-co-visible static meshes that share a material, and collapse avoidable material surfaces in the DCC/import pipeline.

**Why/thread:** Static merging can outperform automatic Vulkan instancing by reducing render-thread objects, state changes, and draw calls; shared materials also reduce pipeline/material churn.

**Measure:** Compare draw calls, visible objects, primitives, frame setup CPU, and viewport CPU/GPU time before and after each merge region. Test from viewpoints where only part of the region is visible.

**Version:** Godot 4.7; Forward+ automatically instances identical opaque/alpha-tested objects, but not alpha-blended objects.

**Source:** [batching and instancing architecture](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#batching-and-instancing), [GPU optimization](https://docs.godotengine.org/en/4.7/tutorials/performance/gpu_optimization.html#draw-calls).

**Evidence:** **Verified** from official documentation.

**Trap:** Over-merging increases overdraw and destroys frustum, occlusion, and HLOD granularity. Different materials still require separate surfaces/draws.

### 11. Keep one material and vary instances with instance uniforms

**What:** Declare `instance uniform` values and set them on `GeometryInstance3D` or `CanvasItem` instead of duplicating `ShaderMaterial` resources.

**Why/thread:** Material reuse reduces render-thread material changes and unique GPU state while retaining per-object color or scalar variation.

**Measure:** Compare material resource count, pipeline-compilation monitor deltas, draw calls, frame setup/render CPU, and GPU time with duplicated versus instance-uniform materials.

**Version:** Godot 4.7; maximum 16 instance uniforms per shader, with multi-material index rules. Compatibility cannot dynamically index sampler arrays with instance uniforms.

**Source:** [per-instance uniforms](https://docs.godotengine.org/en/4.7/tutorials/shaders/shader_reference/shading_language.html#per-instance-uniforms), [ShaderMaterial](https://docs.godotengine.org/en/4.7/classes/class_shadermaterial.html).

**Evidence:** **Verified** from official documentation.

**Trap:** Instance uniforms do not directly provide arbitrary per-instance textures. Multi-surface meshes need consistent uniform names, types, and indices.

### 12. Preserve automatic 2D batches and reduce transparent fill

**What:** Keep compatible sprites, glyphs, and canvas primitives consecutive with shared textures/materials; atlas textures and convert large mostly-transparent sprites to tight `MeshInstance2D` geometry.

**Why/thread:** Godot 4.7 batches compatible 2D work in all renderers, reducing render-thread submissions and draw calls. Tight geometry avoids shading transparent pixels, saving GPU fill.

**Measure:** Use the Visual Profiler's 2D categories and viewport visible draw calls. Compare at fixed output resolutions; if cost scales strongly with resolution but not draw calls, fill is the likely limit.

**Version:** Automatic RD-backend batching arrived in Godot 4.4 and was redesigned in 4.6; this report targets 4.7.

**Source:** [2D architecture](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#d-rendering-techniques), [2D meshes](https://docs.godotengine.org/en/4.7/tutorials/2d/2d_meshes.html).

**Evidence:** **Verified** from official documentation and Godot's [4.6 renderer optimization announcement](https://godotengine.org/article/dev-snapshot-godot-4-6-dev-5/).

**Trap:** Texture, material, primitive, clip, and ordering changes can split batches. `CanvasGroup` is not the automatic batcher and can add backbuffer cost. Extra mesh triangles can lose when transparent fill was not the bottleneck.

### 13. Tune shadow map size, atlas allocation, cascades, and filters

**What:** Lower directional and positional shadow resolution, choose atlas quadrant subdivision for the visible light-size distribution, reduce directional cascades, and lower soft-shadow filtering.

**Why/thread:** Smaller maps reduce GPU raster bandwidth, sampling bandwidth, and VRAM. Fewer cascades reduce shadow draws. Cheaper filters reduce fragment samples; 16-bit maps may improve performance on some devices.

**Measure:** Inspect shadow draw calls and Visual Profiler shadow GPU time while sweeping one setting. Check atlas occupancy and quality across near/far cameras; compare orthogonal, 2-split, and 4-split directional modes.

**Version:** Godot 4.7. PCSS from nonzero directional angular distance is Forward+ only.

**Source:** [3D lights and shadows](https://docs.godotengine.org/en/4.7/tutorials/3d/lights_and_shadows.html#shadow-atlas), [DirectionalLight3D modes](https://docs.godotengine.org/en/4.7/classes/class_directionallight3d.html), [Light3D angular size](https://docs.godotengine.org/en/4.7/classes/class_light3d.html#class-light3d-property-light-angular-distance).

**Evidence:** **Verified** from official documentation.

**Trap:** An undersized or over-subdivided atlas blurs/pops shadows. Four cascades and split blending improve quality but cost more. Soft PCSS can be disproportionately expensive at 3.33 ms budgets.

### 14. If real-time GI is mandatory, reduce its resolution and update pressure

**What:** Enable half-resolution GI and tune SDFGI cascades/cell size/rays/update cadence or VoxelGI subdivision/quality to the minimum acceptable level.

**Why/thread:** SDFGI and VoxelGI are among Godot's heaviest GPU features. Half resolution quarters the GI buffer pixel count; lower spatial/sample/update quality reduces compute and bandwidth.

**Measure:** Use Visual Profiler SDFGI/VoxelGI GPU categories and viewport GPU time in static and moving-light scenes. Compare half/full resolution and each quality step independently.

**Version:** Godot 4.7; both techniques are Forward+ only.

**Source:** [SDFGI](https://docs.godotengine.org/en/4.7/tutorials/3d/global_illumination/using_sdfgi.html), [VoxelGI](https://docs.godotengine.org/en/4.7/classes/class_voxelgi.html), [GI architecture](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#global-illumination).

**Evidence:** **Verified** from official documentation.

**Trap:** Half resolution aliases thin details. SDFGI is poor for low-end GPUs and lacks dynamic occluders; VoxelGI needs baked static geometry coverage and can stall at high subdivision.

### 15. Simplify shaders and reduce transparent overdraw

**What:** Remove unused material features, texture samples, dynamic branches, and layered alpha; prefer opaque or alpha-tested geometry when the look permits.

**Why/thread:** Fragment shaders run per covered sample, so cost multiplies with resolution and overdraw. Dynamic branches can increase register use even when pixels take the same branch; alpha-blended surfaces cannot use ordinary opaque instancing and must shade overlapping layers.

**Measure:** Isolate the material in the Visual Profiler, compare viewport GPU time at two resolutions, and inspect a RenderDoc frame for draw count, shader duration, texture fetches, and overdraw. Use `--gpu-profile` for pass-level confirmation.

**Version:** Godot 4.7; shader and renderer feature support differs.

**Source:** [shader architecture](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#shaders-and-materials), [GPU transparency guidance](https://docs.godotengine.org/en/4.7/tutorials/performance/gpu_optimization.html#transparency-and-blending).

**Evidence:** **Verified** from official documentation and linked core shader source paths.

**Trap:** Replacing runtime branches with compile-time variants can multiply pipelines and compilation work. Alpha scissor can alias; alpha hash/dither can add noise.

### 16. Make Godot 4.7 pipeline precompilation see every runtime feature early

**What:** Instantiate dynamic 3D effects once during loading, expose rendering features before bulk asset loading, watch pipeline compilation monitors, and enable the export shader baker where supported.

**Why/thread:** Ubershaders provide an immediately usable pipeline while optimized specializations compile in the background; early instancing moves unavoidable surface/pipeline work away from gameplay hitches. Shader baking skips source-to-intermediate compilation.

**Measure:** Watch Canvas, Mesh, Surface, Draw, and Specialization pipeline-compilation monitors while replaying a cold first-use path. Count p99/worst frames and frames over the hitch threshold; `Draw` increments during play are defects worth minimizing.

**Version:** Ubershaders and pipeline precompilation are Godot 4.4+ Forward+/Mobile. Shader baker is 4.5+ Forward+/Mobile. Compatibility needs actual one-frame drawing because it lacks ubershaders.

**Source:** [pipeline compilations](https://docs.godotengine.org/en/4.7/tutorials/performance/pipeline_compilations.html), [jitter and stutter](https://docs.godotengine.org/en/4.7/tutorials/rendering/jitter_stutter.html#stutter).

**Evidence:** **Verified** from official documentation.

**Trap:** `preload()` alone does not expose every node/material override pipeline. The shader baker cannot bake the final hardware/driver pipeline and generally improves startup, not all gameplay hitches. Hidden warm-up nodes must not run gameplay scripts.

### 17. Treat the separate render-thread model as an experimental A/B option

**What:** Test `rendering/driver/threads/thread_model = 2` (`Separate`) only when render setup blocks the main thread and the target platforms survive a full stress matrix.

**Why/thread:** A separate rendering driver thread can overlap some main-thread and render work, but synchronization may add jitter and does not reduce GPU cost.

**Measure:** Compare process/main-thread time, `RenderingServer.get_frame_setup_time_cpu()`, summed viewport render CPU, frame p95/p99, and crash/stability results. Stress particles, window resize, scene churn, and graphics-setting changes.

**Version:** Godot 4.7; the project setting remains experimental and documented with known bugs.

**Source:** [ProjectSettings thread model](https://docs.godotengine.org/en/4.7/classes/class_projectsettings.html#class-projectsettings-property-rendering-driver-threads-thread-model), [thread-safe APIs](https://docs.godotengine.org/en/4.7/tutorials/performance/thread_safe_apis.html#rendering).

**Evidence:** **Verified** from official documentation.

**Trap:** The setting can crash, especially around particles or resize, and synchronization can worsen frame pacing. Never call GPU-interacting APIs from arbitrary worker threads; they can force RenderingServer synchronization.

### 18. Use a layered GPU timing workflow, including all viewports

**What:** Start with the Visual Profiler, automate CPU/GPU viewport timings, then use RenderDoc only to inspect the pass or draw already identified as expensive.

**Why/thread:** Measurement does not itself save time, but it prevents optimizing main/physics work when the renderer or GPU owns the budget. `get_frame_setup_time_cpu()` captures global render setup; viewport CPU/GPU methods capture each viewport.

**Measure:** Enable each viewport with `RenderingServer.viewport_set_measure_render_time(rid, true)`. Compute render CPU as `RenderingServer.get_frame_setup_time_cpu()` plus `viewport_get_measured_render_time_cpu()` for every drawn viewport; sum every viewport's measured GPU time separately. Correlate with Visual Profiler categories and an uncapped release-export wall-time sample. Launch with `--gpu-profile`; for source-level RenderDoc shader debugging on a debug build, add `--generate-spirv-debug-info`.

**Version:** Godot 4.7. OpenGL/headless lacks `RenderingDevice`; GPU timers can be unavailable on a backend/platform. Visual Profiler is unavailable for Compatibility on macOS.

**Source:** [Visual Profiler](https://docs.godotengine.org/en/4.7/tutorials/scripting/debug/debugger_panel.html#visual-profiler), [RenderingServer timing methods](https://docs.godotengine.org/en/4.7/classes/class_renderingserver.html#class-renderingserver-method-get-frame-setup-time-cpu), [command-line GPU options](https://docs.godotengine.org/en/4.7/tutorials/editor/command_line_tutorial.html), [RenderDoc](https://renderdoc.org/docs/).

**Evidence:** **Verified** from official Godot and RenderDoc documentation.

**Trap:** Visual Profiler and GPU capture add overhead. GPU pass times can overlap and must not be blindly summed. A capped GPU may downclock, making reported GPU time misleading. A root-only measurement misses SubViewport work.

## Detection

Use this sequence in a real project to find where the top techniques apply:

1. **Establish ownership of the frame:** run an uncapped, fixed-path release benchmark and a short debug Visual Profiler capture. If summed viewport GPU time approaches the 3.33–5.00 ms budget while process/physics remain low, stay in this report's GPU path. If `get_frame_setup_time_cpu()` plus viewport render CPU is high, prioritize submissions, culling, and materials.
2. **Pixel-cost signature:** halve `scaling_3d_scale`. A large GPU-time drop points to techniques 2, 3–5, 13–15. Toggle GI and post effects one at a time; Visual Profiler category names identify the owner.
3. **Shadow signature:** log `get_render_info(RENDER_INFO_TYPE_SHADOW, RENDER_INFO_DRAW_CALLS_IN_FRAME)`. High or camera-dependent shadow draws point to techniques 4 and 13. Grep scenes for `shadow_enabled = true`, `directional_shadow_max_distance`, and `light_angular_distance`.
4. **Geometry/submission signature:** high visible draw calls, objects, or primitives point to techniques 6–11. Grep `.tscn`/`.scn` import metadata for repeated mesh paths, duplicated material subresources, `visibility_range_`, `lod_bias`, and `MultiMeshInstance`. Inspect import settings for generated LODs.
5. **Occlusion signature:** if wireframe shows rooms/blocks behind walls and visible-object counts stay high, test technique 8. Grep for `OccluderInstance3D`; absence in a blocked-view level is a candidate, not proof.
6. **2D signature:** high canvas draw calls with repeated sprites/text point to technique 12. Grep for `CanvasGroup`, unique `CanvasItemMaterial`/`ShaderMaterial` resources, frequent texture alternation, clips, and large transparent sprites. Remove `CanvasGroup` only in a scratch A/B because it may be required for visual compositing.
7. **Shader/material signature:** GPU time scales with resolution while primitives remain stable, or RenderDoc identifies long fragment draws. Grep `.gdshader` for `discard`, `hint_screen_texture`, loops, texture samples, dynamic `if`, and alpha blend modes; grep scripts/scenes for `.duplicate()` on materials and convert eligible values to `instance uniform`.
8. **Pipeline-stutter signature:** replay first-use events while watching the five pipeline compilation monitors. Gameplay-time `Surface` or `Draw` increments and matching frame spikes point to technique 16. Grep for runtime `instantiate()`, `load()`, material overrides, particles, and graphics-feature toggles.
9. **Thread-model signature:** only test technique 17 when global/render CPU setup is material and GPU time has headroom. Reject it if p99 worsens or any target-platform stress scenario fails.
10. **Deep capture:** after a Visual Profiler category identifies the expensive pass, capture one representative frame in RenderDoc. Inspect event count, render-target dimensions, repeated state/material switches, overdraw, and the longest draw/dispatch. Do not infer a gain from a capture; confirm the selected change with interleaved A/B frame timings.
