# Version ledger: Godot 4.4 to 4.7

Target engine: Godot 4.7. What changed in 4.4, 4.5, 4.6 and 4.7, by area, so that knowledge learned on 3.x or 4.0-4.3 does not produce wrong code or wrong defaults.

## Use this shelf when

- You are about to write code from memory and your memory may predate 4.4.
- A tutorial, blog or old project disagrees with what the 4.7 editor shows.
- You upgrade a project and need the breaking changes and the changed defaults in one place.
- You want to know which newer feature replaces a workaround you were about to build.

## How to read it

Each line is `version: change`. Every version tag is backed by the official release page entry, the migration page, or a diff of the class reference XML of 4.3, 4.4, 4.5, 4.6 and 4.7.2 (a name present in 4.5 and absent in 4.4 arrived in 4.5). "New projects" means the default applies only to projects created in that version; an upgraded project keeps its old value. "(break)" marks a change that can break existing code or looks.

Support (release policy): 4.7 (June 2026) and 4.6 (January 2026) get bug fixes, 4.5 gets security and platform fixes only, 4.4 and older are end of life. Only the latest patch of a minor series is supported.

## Core and GDScript

- 4.4: typed dictionaries, `Dictionary[String, int]`, `Dictionary.is_typed`; the Inspector edits typed keys and values.
- 4.4: scripts and shaders get UIDs through `.uid` side files. Commit them and move them with the file. `preload("uid://...")` works in scripts. 4.5: `ResourceUID.path_to_uid` and `ensure_path`; Ctrl-drag a resource into a script to preload it by UID.
- 4.4 (break): `@export_file` stores `uid://` when set in the Inspector. 4.5: `@export_file_path` keeps a raw `res://` path.
- 4.4: `@export_tool_button` puts a button in the Inspector for a `@tool` script.
- 4.4: `Curve` gains an X domain (`min_domain`, `max_domain`) beyond 0..1. (break) It also enforces `min_value`/`max_value`; old curves with points outside the range need them set.
- 4.4: temporary files and folders that delete themselves: `FileAccess.create_temp`, `DirAccess.create_temp`.
- 4.4: `FileAccess.store_*` methods return `bool` (they returned nothing before).
- 4.5: variadic functions (`...args: Array`), `@abstract` classes and methods.
- 4.5: errors log a script backtrace, also in release builds when `debug/settings/gdscript/always_track_call_stacks` is on; `Engine.capture_script_backtraces` returns `ScriptBacktrace` objects.
- 4.5: custom loggers: subclass `Logger`, register with `OS.add_logger` (in-game bug reporters, telemetry).
- 4.5 (break): `Resource.duplicate(true)` copies only resources internal to the file. New `duplicate_deep` on `Resource`, `Array` and `Dictionary` gives explicit control (`Resource.DEEP_DUPLICATE_ALL` restores the old result).
- 4.5: `@export var x: Variant` is editable as any type in the Inspector.
- 4.5 (break): `Node.get_rpc_config` renamed `get_node_rpc_config`.
- 4.6: every node saves a unique ID in `.tscn`, so renames and moves in inherited or instanced scenes keep their overrides; `load_steps` is gone. The first save of an old scene gives a large diff: run Project > Tools > Upgrade Project Files once.
- 4.6: signals whose name starts with `_` are hidden from completion and docs.
- 4.6: `SceneTree.change_scene_to_node` switches to a node you built ahead.
- 4.7 (break): an override of a method with a typed return inherits the return type and needs an explicit `return`.
- 4.7 (break): writing one element of a packed array property no longer calls the setter.
- 4.7: `Tween.tween_await` (returns `AwaitTweener`) pauses a tween until a signal fires.

## Editor and debugging

- 4.4: the game runs embedded in the editor (Game screen) on Linux, Windows and Android; macOS since 4.5. Nodes in the running game can be clicked and the camera moved.
- 4.4: expression evaluator tab in the debugger (evaluate with local state at a breakpoint); "autostart" checkbox for all profilers; `EditorContextMenuPlugin`; Camera3D preview in the Inspector; pin favorite properties; 3D placement snaps to surfaces.
- 4.5: multi-select in the Remote tree; "Mute Game" in the Game screen; named `EditorScript` files run from the command palette; live translation preview in the editor; inspector sections with an enable checkbox (`PROPERTY_HINT_GROUP_ENABLE`); batch import editing is back; the build profile tool detects more of what a project needs.
- 4.6: game speed controls in the Game screen; Step Out in the debugger; ObjectDB snapshots with diff (Debugger > ObjectDB Profiler); click a file in an Output error to open it; docks move anywhere, bottom panels included (`EditorDock`); "Modern" theme by default; Select-only mode in 3D; plugins register shortcuts with `EditorSettings.add_shortcut`; drag a resource into a script to create an `@export`.
- 4.6: script profiling with Tracy, Perfetto and Instruments in custom builds. 4.7: Perfetto is the default tracer for Android editor and template builds.
- 4.6: `Performance.add_custom_monitor` takes a `type` (`MONITOR_TYPE_MEMORY`, `_TIME`, `_PERCENTAGE`, `_QUANTITY`).
- 4.7: Asset Store replaces the Asset Library; 2D Scene Paint mode (scatter scenes without a tile map); `MeshLibrary` editor for `GridMap`; 3D vertex snapping; copy and paste Inspector sections; the Remote inspector folds groups and shows enum names; export templates download per platform; GDExtensions are listed in Project Settings.

## 2D

- 4.4: 2D batching in Forward+ and Mobile (it was Compatibility-only); instance uniforms in `CanvasItem` shaders, which do not break batching.
- 4.5 (break): `TileMapLayer` merges tile colliders into chunks (`physics_quadrant_size`, default 16). One body now covers many cells, so `get_coords_for_body_rid` is approximate; set the size to 1 for exact cells.
- 4.6: scene tiles in a `TileMapLayer` can be rotated in 90 degree steps.
- 4.7 (break): `CanvasItem` line drawing no longer adds an antialiasing feather; lines look thinner.
- 4.7: `CollisionShape2D.one_way_collision_direction` (one-way in any local direction).
- 4.7: `DrawableTexture2D`, a texture you draw into without a `SubViewport`.
- 4.7: `GradientTexture2D.FILL_CONIC`.

## 3D and rendering

- 4.4: ubershaders and pipeline precompilation remove most first-use shader stutter on Forward+ and Mobile, with no content change. 4.5: more pipeline settings are detected at load. 4.5: the shader baker export option precompiles shaders for the target driver; the release page reports about 20x shorter load on Metal and D3D12 for the TPS demo.
- 4.4: Forward+/Mobile fall back to the Compatibility renderer when no modern driver works (`rendering/rendering_device/fallback_to_opengl3`).
- 4.4: Metal backend on macOS and iOS (replaces Vulkan through MoltenVK). 4.7: Metal 4 when supported.
- 4.4: `Environment.TONE_MAPPER_AGX`. 4.6: `tonemap_agx_white`, `tonemap_agx_contrast`.
- 4.4: per-vertex shading works in all renderers (`BaseMaterial3D.SHADING_MODE_PER_VERTEX`, `rendering/shading/overrides/force_vertex_shading`).
- 4.4: `Light3D.shadow_caster_mask` chooses which layers cast shadows per light.
- 4.4: LightmapGI shadowmasks (`LightmapGI.shadowmask_mode`: baked far shadows, dynamic near), bicubic lightmap sampling, baked transparency.
- 4.4 (break): CSG uses the Manifold library; non-manifold meshes are rejected. CSG bakes to a mesh (`CSGShape3D.bake_static_mesh`). 4.7: `CSGShape3D.autosmooth`.
- 4.4: particles: `use_fixed_seed` and `seed`; `restart(keep_seed)`.
- 4.4 (break): `RenderingDevice.draw_list_begin` lost many parameters.
- 4.5: stencil buffer in materials and shaders (`BaseMaterial3D.stencil_mode`).
- 4.5: SMAA (`Viewport.SCREEN_SPACE_AA_SMAA`); bent normal maps (`BaseMaterial3D.bent_normal_enabled`); specular occlusion from ambient light; the Mobile renderer uses half-precision floats explicitly.
- 4.6: new projects on Windows use Direct3D 12 (`rendering/rendering_device/driver.windows`).
- 4.6: screen-space reflections rewritten, with a half-resolution mode.
- 4.6 (break): glow blends before tonemapping; `Environment.glow_blend_mode` defaults to Screen, `glow_intensity` to 0.3, with new `glow_levels`. Old scenes look brighter.
- 4.6 (break): volumetric fog blends brighter; lower its density after upgrading.
- 4.6: reflection and radiance probes use octahedral maps (cheaper, less memory); material debanding for 3D in the Mobile renderer; SSAO in Compatibility; LOD generation keeps shapes better; compressed 3D texture import up to 2x faster.
- 4.6 (break): `MeshInstance3D.skeleton` defaults to an empty path (was `..`); old behavior: `animation/compatibility/default_parent_skeleton_in_mesh_instance_3d`.
- 4.7: `AreaLight3D` (rectangular real-time light).
- 4.7: HDR display output on Windows, macOS, iOS, visionOS and Linux Wayland (`display/window/hdr/request_hdr_output`, `Window.hdr_output_requested`).
- 4.7: `Viewport.SCALING_3D_MODE_NEAREST` for crisp low-resolution 3D; clearcoat follows the Disney model; 3D particle scale and rotation controls.

## Physics

- 4.4: Jolt is built in but experimental: `physics/3d/physics_engine` = `Jolt Physics`. 4.6: not experimental, and the default for new 3D projects. 2D still uses Godot Physics.
- 4.4: 3D physics interpolation, MultiMesh included (`physics/common/physics_interpolation`; 2D had it in 4.3). 4.5: it moved from the RenderingServer to the SceneTree, so `Node3D.get_global_transform_interpolated` and node logic see interpolated transforms; the API did not change.
- 4.5 (break): Jolt always reports `Area3D` overlaps with static bodies; filter by layers.
- 4.6: Mesh menu generates a matching `CollisionShape3D` from a primitive mesh.
- 4.7 (break, Jolt): `WorldBoundaryShape3D.plane` distance sign flipped to the Godot convention; `SoftBody3D` mass defaults to 1 kg total and `linear_stiffness` behaves differently (retune); `Area3D` now reports `SoftBody3D` overlaps.

## Animation

- 4.4: `LookAtModifier3D`, `SpringBoneSimulator3D`, `RetargetModifier3D`; animation markers for sub-regions and loops (`Animation.add_marker`, `AnimationPlayer.play_section_with_markers`).
- 4.5: `BoneConstraint3D` family: `CopyTransformModifier3D`, `ConvertTransformModifier3D`, `AimModifier3D`.
- 4.6: IK returns on `IKModifier3D`: `TwoBoneIK3D`, `SplineIK3D`, and the iterative `FABRIK3D`, `CCDIK3D`, `JacobianIK3D`; plus `LimitAngularVelocityModifier3D` and `BoneTwistDisperser3D`.
- 4.6 (break, C#): `AnimationPlayer.current_animation`, `assigned_animation`, `autoplay` are `StringName`.
- 4.7 (break): blend spaces use `sync_mode` (enum) in place of the `sync` bool; check each `AnimationNodeBlendSpace1D`/`2D` if transitions look wrong.
- 4.7 (break): `LookAtModifier3D.relative` defaults to false.

## GUI and text

- 4.5: screen reader support through AccessKit (`accessibility/general/accessibility_support`). 4.7: landmark regions; the enums moved to `AccessibilityServer`.
- 4.5: `FoldableContainer`; recursive mouse and focus behavior (`Control.mouse_behavior_recursive`, `focus_behavior_recursive`) to disable a whole subtree; stacked label effects (`LabelSettings.stacked_outline_count`); `DPITexture` and sharp icons on HiDPI.
- 4.5 (break): `RichTextLabel.add_image` split `size_in_percent` into width and height flags. 4.7 (break): those became `width_unit`/`height_unit`, sizes are `float`, and `[img height=1em]` scales with the font.
- 4.6: mouse clicks no longer show the keyboard focus ring (`Control.grab_focus` has `hide_focus`); `Control.pivot_offset_ratio` keeps a pivot centered on resize.
- 4.7: `Control` offset transforms (`offset_transform_enabled`, `offset_transform_position`, `_rotation`, `_scale`) animate a control inside a container without the container resetting it.
- 4.7: `PopupMenu.search_bar_enabled`.
- 4.7: new projects use stretch mode `canvas_items` and aspect `expand` (`display/window/stretch/mode`, `display/window/stretch/aspect`).

## Audio

- 4.4: WAV files load at runtime (`AudioStreamWAV.load_from_file`).
- 4.7 (break): `AudioStreamPlayer2D.area_mask` and `AudioStreamPlayer3D.area_mask` default to 0. `Area2D`/`Area3D` bus overrides stop working until the mask is set to layer 1.

## Input

- 4.4 (break): Android sensors are off by default (`input_devices/sensors/enable_accelerometer`, ...).
- 4.5: gamepads use the SDL 3 driver on desktop. 4.7: on iOS too.
- 4.6: controller LED color (`Input.set_joy_light`, `Input.has_joy_light`).
- 4.7: controller motion sensors for gyro aiming (`Input.get_joy_gyroscope`, `Input.get_joy_accelerometer`; marked experimental).
- 4.7: `input_devices/joypads/ignore_joypad_on_unfocused_application`.
- 4.7 (break): mouse and keyboard events use `InputEvent.DEVICE_ID_MOUSE` and `InputEvent.DEVICE_ID_KEYBOARD`, not device 0. Code that maps device 0 to "player 1 keyboard" breaks.
- 4.7: `VirtualJoystick` control (fixed, dynamic and following modes).

## Navigation and networking

- 4.4: navigation map sync runs on a background thread (`navigation/world/map_use_async_iterations`); faster path queries.
- 4.5: region updates are async too (`navigation/world/region_use_async_iterations`), so a change can land a frame later. (break) Edge merging order changed; fix overlapping navmeshes or lower `navigation/3d/merge_rasterizer_cell_scale`.
- 4.5: a real 2D navigation server; 2D games no longer need 3D navigation compiled in.
- 4.6 (break): `AStar2D.get_point_path`, `AStar3D.get_point_path` and `AStarGrid2D` paths are empty when the start point is disabled or solid.
- 4.6: `StreamPeerTCP` and `TCPServer` get bases `StreamPeerSocket` and `SocketServer`; `StreamPeerUDS` and `UDSServer` add Unix domain sockets.

## Import

- 4.4: Betsy GPU texture compressor makes "VRAM Compressed" imports much faster; glTF animations can target custom properties (`GLTFObjectModelProperty`).
- 4.5: glTF/FBX/Blend files import non-joint nodes inside skeletons correctly; old files keep the old "Naming Version" until changed in the Import dock.
- 4.6: CSV translations take `?context` and `?plural` columns; C# `Tr`/`TrN` strings are collected for POT/CSV generation.

## Export and platforms

- 4.4: export from the Android editor; Swappy frame pacing on Android (automatic).
- 4.5: visionOS export; Android 16 KB page support and edge-to-edge option; Web builds use WebAssembly SIMD; Windows `.exe` metadata is set without `rcedit`; native Wayland sub-windows.
- 4.5 (break): Android C# export needs .NET 9.
- 4.6: patch PCKs use delta encoding (only changed parts of a resource); LibGodot (engine as a library) on desktop; Android file access through the system picker without broad storage permission; `scrcpy` mirroring option on Android deploy.
- 4.6 (break): the Android build template follows the Android Studio layout (`android/build/src/main/java/...`).
- 4.6: Gradle builds from the Android editor through the GABE companion app; stable in 4.7.
- 4.7: picture-in-picture on Android (`DisplayServer.FEATURE_PIP_MODE`); Android splash screen set in the export options; Wayland touch input.
- 4.7 (break): minimum macOS is 11. The `wasm64` feature tag exists but the docs say such builds are not yet possible. C# still cannot export to the web.

## XR

- 4.4: Android editor runs on Meta Quest 3, 3S and Pro.
- 4.5: OpenXR render models (`OpenXRRenderModel`); Application SpaceWarp; foveated rendering on Vulkan Mobile; Direct3D 12 OpenXR backend.
- 4.5 (break): the OpenXR editor classes are editor-only (also in 4.4.1).
- 4.6: OpenXR 1.1 is used when the runtime supports it (`xr/openxr/target_api_version`); OpenXR Spatial Entities (anchors, plane and marker tracking); XR editor on Android XR.
- 4.7: Vulkan subsampled images speed up foveated rendering; simpler default action map (generic controller profile); composition layers can follow `XRCamera3D`; Android XR and Steam Frame support.

## C# and GDExtension

- 4.4 (break): minimum .NET is 8; projects are upgraded on open. C# on Android supports all ABIs.
- 4.5: GDExtension main loop callbacks (startup, shutdown, frame); the class reference marks virtual methods that must be overridden as `required`.
- 4.6: the GDExtension interface is described in JSON; parameters and return values can be declared required (non-null) for strict bindings.
- 4.7 (break, C#): `Animation.length` is `double`.
- 4.7: GDScript can implement Java interfaces through `JavaClassWrapper` on Android.

## Outdated beliefs

| Belief | True in 4.7 |
| --- | --- |
| Godot Physics is the 3D default | New projects use Jolt since 4.6 |
| Vulkan is the Windows default | New projects use Direct3D 12 since 4.6 |
| Shader stutter needs a hand-made warm-up scene | Mostly automatic since 4.4; baker in 4.5 |
| 3D has no physics interpolation | Built in since 4.4 |
| Dictionaries cannot be typed | They can since 4.4 |
| GDScript has no abstract classes or varargs | Both since 4.5 |
| Only a path identifies a script | `uid://` since 4.4 |
| No stencil buffer access | Since 4.5 |
| Release builds give no script stack | Opt-in since 4.5 |
| Drawing into a texture needs a `SubViewport` | `DrawableTexture2D` since 4.7 |
| Animating a control in a container needs a wrapper | Offset transforms since 4.7 |
| `SkeletonIK3D` is the IK node | Deprecated; use the 4.6 IK nodes |
| `TileMap`, `ParallaxBackground` are current | Deprecated: `TileMapLayer`, `Parallax2D` |
| One collider per tile | Chunked since 4.5 |
| A curve only maps 0..1 | Free X domain since 4.4 |
| You must call `randomize()` | Automatic since 4.0 |
| Keyboard is device 0 | Dedicated IDs since 4.7 |
| New projects do not stretch the UI | `canvas_items` + `expand` since 4.7 |
| The engine streams textures and meshes | Not in 4.7; it is a listed team priority |
| `yield`, `instance()`, `KinematicBody`, `Tween` node | `await`, `instantiate()`, `CharacterBody3D`, `create_tween()` (4.0) |

## Traps

- A default marked "new projects" does not change on upgrade. Read `project.godot` before you assume which physics engine, driver or stretch mode a project uses.
- Glow and fog looks shift on the 4.5 -> 4.6 upgrade without any error.
- The 4.7 `area_mask` change fails silently: audio just stops switching bus.
- 4.8 development posts describe features that are not in 4.7. Check the 4.7.2 class reference before using a name from a recent blog post.

## Related skills

- `godot-version-migration` - the step-by-step upgrade procedure and API rename tables.
- `godot-project-setup` - choosing renderer, physics engine and stretch settings.
- `godot-physics-system` - Jolt and Godot Physics in practice.
- `godot-export-pipeline` - export presets and platform requirements.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.4.html
- https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.5.html
- https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.6.html
- https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.7.html
- https://docs.godotengine.org/en/stable/about/release_policy.html
- https://docs.godotengine.org/en/stable/tutorials/performance/pipeline_compilations.html
- https://godotengine.org/releases/4.4/
- https://godotengine.org/releases/4.5/
- https://godotengine.org/releases/4.6/
- https://godotengine.org/releases/4.7/
- https://godotengine.org/article/uid-changes-coming-to-godot-4-4/
- https://godotengine.org/article/dev-snapshot-godot-4-5-dev-3/
- https://godotengine.org/article/dev-snapshot-godot-4-5-dev-4/
- https://godotengine.org/article/dev-snapshot-godot-4-6-dev-2/
- https://godotengine.org/priorities/
