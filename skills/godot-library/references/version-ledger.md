# Version ledger: Godot 4.4 to 4.7

Target engine: Godot 4.7. What changed in 4.4, 4.5, 4.6 and 4.7, by area, so that knowledge
learned on 3.x or 4.0-4.3 does not produce wrong code or wrong defaults.

## Use this shelf when

- You are about to write code from memory and your memory may predate 4.4.
- A tutorial, blog or old project disagrees with what the 4.7 editor shows.
- You upgrade a project and need the breaking changes and the changed defaults in one place.

## How to read it

Each line is `version: change`. No mark = stated in the 4.7 docs (migration pages or a
"since 4.x" note). `*` = version recalled from the release notes, not re-read (web blocked);
the name is checked in the 4.7.2 class XML. "New projects" = the default applies only to
projects created in that version; an upgraded project keeps its old value.

Support (release policy): 4.7 (June 2026) and 4.6 (January 2026) get bug fixes, 4.5 gets
security and platform fixes only, 4.4 and older are end of life. Only the latest patch of a
minor series is supported.

## Core and GDScript

- 4.4*: typed dictionaries, `Dictionary[String, int]`, `Dictionary.is_typed`.
- 4.4*: scripts and shaders get UIDs (`.uid` side files, commit them); see `ResourceUID`.
- 4.4: `@export_file` stores `uid://` when set in the Inspector. 4.5: `@export_file_path`
  keeps a raw `res://` path.
- 4.4*: `@export_tool_button` puts a button in the Inspector for a `@tool` script.
- 4.4: `Curve` enforces `min_value`/`max_value` (default 0..1); widen them for other ranges.
- 4.4: `FileAccess.store_*` methods return `bool` (they returned nothing before).
- 4.5: variadic functions (`...args`), `@abstract` classes and methods.
- 4.5: errors log a script backtrace, also in exported builds when
  `debug/settings/gdscript/always_track_call_stacks` is on; `Engine.capture_script_backtraces`.
- 4.5: custom loggers: subclass `Logger`, register with `OS.add_logger`.
- 4.5: `Resource.duplicate(true)` copies only resources internal to the file; use
  `Resource.duplicate_deep` with `DEEP_DUPLICATE_ALL` for the old behavior.
- 4.5: `Node.get_rpc_config` renamed `get_node_rpc_config`.
- 4.6: `.tscn` files save unique node IDs and drop `load_steps`; the first save of an old
  scene gives a large diff. Run Project > Tools > Upgrade Project Files once.
- 4.7: an override of a method with a typed return inherits the return type and needs an
  explicit `return`.
- 4.7: writing one element of a packed array property no longer calls the property setter.

## Editor and debugging

- 4.4*: the game runs embedded in the editor (Game screen) with pause, frame advance, speed
  and node picking. See `design-iteration-toolkit`.
- 4.4*: `EditorContextMenuPlugin` adds entries to editor context menus.
- 4.6: ObjectDB Profiler tab in the Debugger (object snapshots and diffs).
- 4.6: `Performance.add_custom_monitor` takes a `type` (`Performance.MonitorType`).
- 4.6: `EditorFileDialog` members moved to `FileDialog`.
- 4.6*: Tracy profiler support in engine builds. 4.7: Perfetto export templates are shipped.

## 2D

- 4.5: `TileMapLayer` merges tile colliders into chunks (`physics_quadrant_size`, default
  on). `get_coords_for_body_rid` is then approximate; set the size to 1 for exact cells.
- 4.7: `CanvasItem` line drawing no longer adds an antialiasing feather; lines look thinner.
- 4.7*: `CollisionShape2D.one_way_collision_direction` (one-way in any direction).
- 4.7*: `DrawableTexture2D` (a texture you can draw into).

## 3D and rendering

- 4.4: ubershaders and pipeline precompilation remove most first-use shader stutter on
  Forward+ and Mobile. 4.5: the shader baker export option cuts first-load time further.
- 4.4: Forward+/Mobile try the other of Vulkan/Direct3D 12 before falling back to
  Compatibility.
- 4.4*: Metal driver on Apple platforms (`rendering/rendering_device/driver.macos`). 4.7:
  Metal 4 when supported.
- 4.4*: `Environment.TONE_MAPPER_AGX`; per-vertex shading
  (`BaseMaterial3D.SHADING_MODE_PER_VERTEX`, `rendering/shading/overrides/force_vertex_shading`).
- 4.4: CSG uses the Manifold library; non-manifold meshes are rejected. CSG can be baked to
  a `MeshInstance3D` (`CSGShape3D.bake_static_mesh`).
- 4.4: particles: `restart(keep_seed)`; `use_fixed_seed` and `seed` give repeatable effects.
- 4.4: `RenderingDevice.draw_list_begin` lost many parameters.
- 4.5: stencil buffer in materials and shaders (`BaseMaterial3D.stencil_mode`).
- 4.5*: SMAA (`Viewport.SCREEN_SPACE_AA_SMAA`); bent normal maps
  (`BaseMaterial3D.bent_normal_enabled`).
- 4.6: new projects on Windows use Direct3D 12 (`rendering/rendering_device/driver.windows`).
- 4.6: glow defaults changed: `Environment.glow_blend_mode` is Screen, `glow_intensity` 0.3,
  new `glow_levels`; Mobile glow was rewritten. Old scenes look brighter; retune.
- 4.6: volumetric fog blends brighter; lower its density after upgrading.
- 4.6: simplified SSAO in the Compatibility renderer.
- 4.6: `MeshInstance3D.skeleton` defaults to an empty path (was `..`); old behavior:
  `animation/compatibility/default_parent_skeleton_in_mesh_instance_3d`.
- 4.7*: `AreaLight3D` (rectangular light). 4.7*: HDR display output
  (`display/window/hdr/request_hdr_output`, `Window.hdr_output_requested`).

## Physics

- 4.4*: Jolt is built in: `physics/3d/physics_engine` = `Jolt Physics`. 4.6: it is the
  default for new projects. 2D still uses Godot Physics.
- 4.4*: 3D physics interpolation (`physics/common/physics_interpolation`,
  `Node.physics_interpolation_mode`); 2D had it since 4.3.
- 4.5: Jolt always reports `Area3D` overlaps with static bodies; filter with layers.
- 4.7 (Jolt): `WorldBoundaryShape3D.plane` distance sign flipped to the Godot convention;
  `SoftBody3D` mass defaults to 1 kg total and `linear_stiffness` behaves differently
  (retune); `Area3D` now reports `SoftBody3D` overlaps.

## Animation

- 4.4*: `LookAtModifier3D`, `SpringBoneSimulator3D`, `RetargetModifier3D`; animation markers
  (`Animation.add_marker`, `AnimationPlayer.play_section_with_markers`).
- 4.5*: `BoneConstraint3D` family: `CopyTransformModifier3D`, `ConvertTransformModifier3D`,
  `AimModifier3D`.
- 4.6*: IK nodes on `IKModifier3D`: `TwoBoneIK3D`, `FABRIK3D`, `CCDIK3D`, `JacobianIK3D`,
  `SplineIK3D`; also `LimitAngularVelocityModifier3D`, `BoneTwistDisperser3D`.
- 4.6: `AnimationPlayer.current_animation`, `assigned_animation`, `autoplay` are
  `StringName` (C# source break).
- 4.7: blend spaces use `sync_mode` (enum) in place of the `sync` bool; check each
  `AnimationNodeBlendSpace1D`/`2D` if transitions look wrong.
- 4.7: `LookAtModifier3D.relative` defaults to false. `Tween.tween_await`* waits for a signal.

## GUI and text

- 4.5*: screen reader support (`accessibility/general/accessibility_support`); 4.7:
  `AccessibilityServer` owns the enums (`Control.accessibility_live` type changed).
- 4.5*: `FoldableContainer`; `Control.mouse_behavior_recursive`,
  `focus_behavior_recursive`; `DPITexture` (resolution-independent SVG);
  `LabelSettings.stacked_outline_count`.
- 4.5: `RichTextLabel.add_image` split `size_in_percent` into width and height flags.
  4.7: those became `width_unit` and `height_unit` and sizes are `float`.
- 4.6: `Control.grab_focus` can hide the focus ring (`hide_focus`).
- 4.7: new projects use stretch mode `canvas_items` and aspect `expand`
  (`display/window/stretch/mode`, `display/window/stretch/aspect`).

## Audio

- 4.7: `AudioStreamPlayer2D.area_mask` and `AudioStreamPlayer3D.area_mask` default to 0.
  `Area2D`/`Area3D` bus overrides stop working until you set the mask to layer 1 again.

## Input

- 4.4: Android sensors are off by default (`input_devices/sensors/enable_accelerometer`, ...).
- 4.5: gamepads go through SDL 3 on desktop.
- 4.7: mouse and keyboard events use `InputEvent.DEVICE_ID_MOUSE` and
  `InputEvent.DEVICE_ID_KEYBOARD`, not device 0. Code that maps device 0 to "player 1
  keyboard" breaks.
- 4.7*: `VirtualJoystick` control for touch screens.

## Navigation and networking

- 4.5: region updates run on threads (`navigation/world/region_use_async_iterations`), so a
  map change can land a frame later. Navmesh edge merging order changed; fix overlaps or
  lower `navigation/3d/merge_rasterizer_cell_scale`.
- 4.6: `AStar2D.get_point_path`, `AStar3D.get_point_path` and `AStarGrid2D` paths are empty
  when the start point is disabled or solid.
- 4.6: `StreamPeerTCP` and `TCPServer` share bases `StreamPeerSocket` and `SocketServer`;
  `StreamPeerUDS`* and `UDSServer`* add Unix domain sockets.

## Import

- 4.5: glTF/FBX/Blend files import non-joint nodes inside skeletons correctly; old files
  keep the old "Naming Version" until you change it in the Import dock.
- 4.6: CSV translations can carry context and plural columns.

## Export and platforms

- 4.5: Android C# export needs .NET 9 (other platforms: .NET 8 minimum). 4.5*: visionOS
  export (`EditorExportPlatformVisionOS`).
- 4.6: the Android build template follows the Android Studio layout
  (`android/build/src/main/java/...`); custom Java files must move.
- 4.7: minimum macOS is 11. The `wasm64` feature tag exists but the docs say such builds
  are not yet possible. C# still cannot export to the web.

## XR

- 4.5: the OpenXR editor classes are editor-only (also in 4.4.1).
- 4.6: `OpenXRExtensionWrapper._get_requested_extensions` gets an `xr_version` parameter.
- 4.7: `OpenXRExtensionWrapper._on_register_metadata` changed signature.

## C# and GDExtension

- 4.5: C# `StringExtensions.PathJoin` and `GetExtension` changed on edge cases.
- 4.6: LibGodot (engine as a library) is experimental.
- 4.7: `Animation.length` is `double` in C#.

## Outdated beliefs

| Belief | True in 4.7 |
| --- | --- |
| Godot Physics is the 3D default | New projects use Jolt since 4.6 |
| Vulkan is the Windows default | New projects use Direct3D 12 since 4.6 |
| Shader stutter needs a hand-made warm-up scene | Mostly automatic since 4.4 |
| 3D has no physics interpolation | Built in since 4.4 |
| Dictionaries cannot be typed | They can since 4.4 |
| GDScript has no abstract classes or varargs | Both since 4.5 |
| Only a path identifies a script | `uid://` since 4.4 |
| No stencil buffer access | Since 4.5 |
| `SkeletonIK3D` is the IK node | Deprecated; use the 4.6 IK nodes |
| `TileMap`, `ParallaxBackground` are current | Deprecated: `TileMapLayer`, `Parallax2D` |
| You must call `randomize()` | Automatic since 4.0 |
| Keyboard is device 0 | Dedicated IDs since 4.7 |
| New projects do not stretch the UI | `canvas_items` + `expand` since 4.7 |
| `yield`, `instance()`, `KinematicBody`, `Tween` node | `await`, `instantiate()`, `CharacterBody3D`, `create_tween()` (4.0) |

## Traps

- A default marked "new projects" does not change on upgrade. Read the value in
  `project.godot` before you assume which physics engine or driver a project uses.
- Glow and fog looks shift on the 4.5 -> 4.6 upgrade without any error.
- The 4.7 `area_mask` change fails silently: audio just stops switching bus.

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
- https://docs.godotengine.org/en/stable/about/list_of_features.html
- https://docs.godotengine.org/en/stable/tutorials/performance/pipeline_compilations.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/logging.html
- The release posts on godotengine.org for 4.4 to 4.7 could not be fetched in this session.
