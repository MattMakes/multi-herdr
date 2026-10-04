# Upgrading a Godot 4.0-4.6 project to 4.7

Read this after `project.godot` shows a 4.x version, and after a 3.x port
(start at 4.0 → 4.1). Read every hop from the project's version up to 4.7;
the changes add up. Each line says what breaks and what the 4.7 way is.

Sources: the official pages "Upgrading from Godot 4.N to 4.N+1"
(`tutorials/migrating/upgrading_to_godot_4.1.rst` to `..._4.7.rst`, godot-docs
branch `4.7` at `9adca4c1c72917bfe1b7be3108abed5ce26696a6`). `GH-n` is
`https://github.com/godotengine/godot/pull/n`. Every 4.7 name was looked up
in the 4.7.2 `--doctool` dump.

Not listed: GDExtension, `RenderingDevice`, `TextServerExtension`,
OpenXR-extension and editor-plugin internals. If the project has a
GDExtension or an editor plugin that uses them, read the official page of
each hop in full.

## 4.0 → 4.1

GDScript breaks:

- `PathFollow2D.lookahead` is gone (GH-72842).
- `Area2D.priority` and `Area3D.priority` are `int` (GH-72749).
- `PhysicsDirectSpaceState2D/3D.collide_shape()` returns `Array[Vector2]` /
  `Array[Vector3]` (one flat array of points) (GH-75260).
- Navigation avoidance (GH-69988): `NavigationAgent2D/3D.time_horizon` is now
  `time_horizon_agents` and `time_horizon_obstacles`;
  `NavigationAgent3D.agent_height_offset` is `path_height_offset`;
  `ignore_y` and `NavigationObstacle2D/3D.estimate_radius` are gone;
  `NavigationServer2D/3D.agent_set_callback()` is
  `agent_set_avoidance_callback()`. The 4.7 way to feed avoidance: set
  `NavigationAgent2D/3D.velocity` each physics frame and read the safe
  velocity from the `velocity_computed` signal. (The 4.1 page renames
  `NavigationObstacle*.get_rid()` to `get_agent_rid()`; the 4.7.2 dump has
  `get_rid()` and no `get_agent_rid()`. Use `get_rid()`.)
- A custom `AnimationNode._process()` takes a `test_only` argument (GH-75759).
  The method is deprecated in 4.7.

Behaviour:

- A `SubViewportContainer` passes input to its `SubViewport` only when its
  `mouse_filter` is `MOUSE_FILTER_STOP` or `MOUSE_FILTER_PASS` (GH-57894).
  Stacked containers that must all get mouse input need `Area2D` picking
  instead.
- A `Viewport` with `physics_object_picking` on marks each picked
  `InputEvent` as handled (GH-77595). Code in `_unhandled_input()` no longer
  sees it.

## 4.1 → 4.2

- **Mesh format.** 4.2 can read 4.0-4.1 meshes, but 4.1 cannot read 4.2
  meshes. The upgrade dialog and **Project > Tools > Upgrade Mesh Surfaces**
  are GUI only. Send a `QUESTION:` for an operator step (GH-81138).
- `AnimationPlayer` and `AnimationTree` share the base class
  `AnimationMixer`. The 4.7 names: `active` (was `playback_active`),
  `callback_mode_process` (was `playback_process_mode` /
  `process_callback`), `callback_mode_method` (was `method_call_mode`),
  `root_node`. The old constants `ANIMATION_PROCESS_*` and
  `ANIMATION_METHOD_CALL_*` are deprecated; use
  `AnimationMixer.ANIMATION_CALLBACK_MODE_*`.
- `Node.NOTIFICATION_NODE_RECACHE_REQUESTED` is gone (GH-84419).
- `TileMap.cell_quadrant_size` is `rendering_quadrant_size` (GH-81070).
- `GraphEdit` / `GraphNode`: `arrange_nodes_button_hidden` →
  `show_arrange_button`, `use_snap` → `snapping_enabled`, `snap_distance` →
  `snapping_distance`, `get_zoom_hbox()` → `get_menu_hbox()`;
  `close_request` → `delete_request` on `GraphElement`; the
  `get_connection_input_*` / `get_connection_output_*` methods, `comment`,
  `overlay`, `show_close`, `language`, `text_direction` are gone.

## 4.2 → 4.3

- **TileMap layers are nodes.** `TileMap` is deprecated; each layer is a
  `TileMapLayer` node. The editor's "Extract TileMap layers" action is GUI.
  Use the headless converter in `headless-tools.md` (GH-87379, GH-89179).
- **Reverse Z.** The depth buffer is reversed. Custom shaders that read or
  write depth may break (article "Introducing Reverse Z").
- `Decal.modulate` is converted from sRGB to linear (GH-89849). Decals look
  different; a human checks them.
- `Skeleton3D` signal `bone_pose_changed` → `skeleton_updated` (GH-90575).
- `PhysicsShapeQueryParameters3D.motion` is a `Vector3` (GH-85393).
- `NavigationRegion2D` lost `avoidance_layers`, `constrain_avoidance` and
  their methods, with no replacement (GH-90747).
- `AnimationMixer` capture mode and `AnimationNode` time handling changed
  (GH-86715, GH-87171). Blends can look different. Article: "Migrating
  Animations from Godot 4.0 to 4.3".
- Default font outline colour is black, was white (GH-54641).
- `auto_translate` is deprecated. The 4.7 way: `Node.auto_translate_mode`
  (default `AUTO_TRANSLATE_INHERIT`) and `Node.can_auto_translate()`. A child
  of a node that does not translate stops translating (GH-87530).
- High-level multiplayer: the `SceneMultiplayer` protocol changed (GH-90027).
  Server and clients must run the same Godot version.
- Binary serialization of scripted objects and typed arrays changed
  (GH-78219). Saves made with `var_to_bytes()` / `store_var()` by an older
  version need a test load.
- Android permissions are not requested on their own. Call
  `OS.request_permission()` and wait for `MainLoop.on_request_permissions_result`
  (GH-87080).

## 4.3 → 4.4

- `@export_file` set from the inspector stores a `uid://` path, not
  `res://` (GH-97912). Code that parses the path must accept both. On 4.5+
  `@export_file_path` keeps raw `res://` paths.
- `FileAccess.store_*()` methods return `bool` (GH-78289). The 4.7 way: check
  the result of each write that matters.
- `OS.read_string_from_stdin()` takes a `buffer_size` argument; the old
  default was 1024 (GH-91201).
- `Curve` keeps its points inside `min_value` .. `max_value`. Set the range
  first if points are outside `[0, 1]`.
- CSG uses the Manifold library (GH-94321). A non-manifold CSG mesh no
  longer works: use `MeshInstance3D` for planes and open meshes.
  `CSGShape3D.snap` does nothing.
- `VisualShaderNodeVec4Constant` takes a `Vector4`; enter the values again.
- Android sensor events are off by default (GH-94799): turn them on under
  **Input Devices > Sensors** in the project settings.

## 4.4 → 4.5

- `Resource.duplicate(true)` copies only sub-resources inside the same file.
  The 4.7 way to copy a resource and every resource it points to:
  `resource.duplicate_deep(Resource.DEEP_DUPLICATE_ALL)`.
- `Node.get_rpc_config()` → `get_node_rpc_config()` (GH-106848).
- `JSONRPC.set_scope()` → `set_method()` (GH-104890).
- `RenderingServer.instance_reset_physics_interpolation()` and
  `instance_set_interpolated()` are gone (GH-104269).
- `TileMapLayer` physics chunking is on: `get_coords_for_body_rid()` is less
  exact. Set `physics_quadrant_size = 1` to get exact cells.
- Navigation regions update on threads (`navigation/world/region_use_async_iterations`,
  default `true`). A region change shows up a little later. Navmesh merging
  changed order; old merge errors can show up more.
- Jolt: the setting `physics/jolt_physics_3d/simulation/areas_detect_static_bodies`
  is gone. An `Area3D` always reports static bodies; filter with layers and
  masks (GH-105746).
- glTF / `.blend` / FBX: new files import non-joint nodes in a skeleton the
  new way; old files keep the old way until you change "Naming Version"
  in the import settings (GH-104184, GH-107352).
- `ProjectSettings.add_property_info()` warns on bad keys, including
  `usage`. Use `ProjectSettings.set_as_basic()`, `set_restart_if_changed()`
  or `set_as_internal()`.
- `RichTextLabel.add_image()` / `update_image()`: `size_in_percent` was split
  (GH-107347); 4.7 changed it again (see 4.6 → 4.7).
- C# Android export needs .NET 9.

## 4.5 → 4.6

- Scene files store a unique ID per node and no `load_steps` (GH-106837,
  GH-103352). The first editor save of each scene in 4.6+ gives a large
  diff. 4.5 and 4.6 can read each other's scenes. Commit such a diff alone.
- New projects use Jolt for 3D physics (GH-105737) and D3D12 on Windows
  (GH-113213). Existing projects keep their settings
  (`physics/3d/physics_engine`, `rendering/rendering_device/driver.windows`).
- Glow: the default blend mode is Screen and other glow defaults changed
  (`glow_intensity` 0.8 → 0.3, `glow_levels`), so glow looks brighter
  (GH-110671). The Mobile renderer glow was rewritten (GH-110077).
  Volumetric fog is brighter (GH-112494). A human checks the look.
- `MeshInstance3D.skeleton` defaults to an empty `NodePath` (was `".."`)
  (GH-112267). Set it, or turn on
  `animation/compatibility/default_parent_skeleton_in_mesh_instance_3d`.
- `AStar2D/3D.get_point_path()` and `AStarGrid2D.get_id_path()` /
  `get_point_path()` return an empty path when the start point is disabled
  or solid (GH-113988).
- `AnimationPlayer.assigned_animation`, `autoplay`, `current_animation` are
  `StringName`; `get_queue()` returns `Array[StringName]` (C# break,
  GH-110767).
- `FileAccess.get_as_text()` has no `skip_cr` argument (GH-110867).
- `StreamPeerTCP` `poll()`, `get_status()`, `disconnect_from_host()` moved to
  the base class `StreamPeerSocket`; GDScript calls still work.
- `PopupMenu.submenu_popup_delay` default 0.3 → 0.2.
- Android export templates use the Android Studio layout
  (`android/build/src/main/java/...`) (GH-110829). Move custom Java files.

## 4.6 → 4.7

- **Typed return of an override.** An override of a method with a typed
  return gets that return type, so every path needs a `return`. Add
  `return null` (or a real value) at the end (GH-115763).
- **Packed array elements.** `obj.packed_prop[i] = x` no longer calls the
  setter of `packed_prop` (GH-113228). Assign the whole array to run the
  setter.
- **Mouse and keyboard device IDs** are `InputEvent.DEVICE_ID_MOUSE` and
  `InputEvent.DEVICE_ID_KEYBOARD`, not `0` (GH-116274). Check the event type,
  or compare `event.device` with those constants.
- `AudioStreamPlayer2D/3D.area_mask` defaults to `0` (GH-107679). If the
  project uses an `Area2D/3D` audio bus override and kept the old default
  (layer 1), set `area_mask = 1` again.
- Jolt: `WorldBoundaryShape3D.plane.d` has the opposite sign (GH-118948);
  `SoftBody3D` total mass defaults to 1 kg and `linear_stiffness` acts
  differently (GH-116041); `Area3D` reports `SoftBody3D` overlaps (GH-114198).
- Blend spaces: `AnimationNodeBlendSpace1D/2D.sync` is deprecated. Set
  `sync_mode` on each blend space if transitions look wrong.
- `CanvasItem` lines lose the antialiasing feather, so they look thinner
  (GH-105122). The `LinearToSRGB` visual shader node no longer clamps on
  Forward+ / Mobile (GH-113956).
- `RichTextLabel.add_image()` / `update_image()`: `width` and `height` are
  `float`; `width_in_percent` / `height_in_percent` became `width_unit` /
  `height_unit` of type `RichTextLabel.ImageUnit`; the mask constant
  `UPDATE_WIDTH_IN_PERCENT` is `UPDATE_WIDTH_UNIT` (GH-112617).
- `AudioEffectSpectrumAnalyzer.tap_back_pos` is gone (GH-114355).
- `Control.accessibility_live` uses `AccessibilityServer.AccessibilityLiveMode`
  (GH-116839). The `DisplayServer` accessibility API is deprecated: use
  `AccessibilityServer`.
- `Object.is_class()` takes a `StringName`; `ImageTexture.get_format()` and
  `PortableCompressedTexture2D.get_format()` live on `Texture2D`.
- `EditorSceneFormatImporter.IMPORT_*` constants are in the `ImportFlags`
  enum (C# source break).
- `type_exists()` is deprecated: use `ClassDB.class_exists()`.
- Defaults: `LookAtModifier3D.relative` is `false`;
  `rendering/reflections/sky_reflections/roughness_layers` is 8 again;
  font import `hinting` is 3; new projects get stretch mode
  `canvas_items` and aspect `expand` (existing projects keep theirs).
- The 4.7 editor needs macOS 11 or later.

## After the last hop

Run the deprecated-API scan in `deprecated-in-4.7.md`, the parse check, the
tests and the resave script. List in `DONE:` every visual item above that a
human must look at.
