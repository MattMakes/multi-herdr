# Deprecated in Godot 4.7: the old name and the 4.7 way

Read this as the last scan of a migration, and when you review code that
must stay current. Each old name below still runs on 4.7.2 but is marked
deprecated in the 4.7 class reference (`godot-docs` branch `4.7` at
`9adca4c1c72917bfe1b7be3108abed5ce26696a6`, `classes/class_*.rst`). Every
replacement exists in the 4.7.2 `--doctool` dump.

The list covers game code. It leaves out the editor, GDExtension, the
language server, `RenderingDevice` and OpenXR extension internals. The
`DisplayServer.accessibility_*` methods and constants (about 170) all move to
`AccessibilityServer`; `DisplayServer.global_menu_*` moves to `NativeMenu`
or `PopupMenu`.

## Scan

```bash
rg -n -g '*.gd' -e 'Color8\(' -e '\bconvert\(' -e 'inst_to_dict|dict_to_inst' \
   -e 'type_exists\(' -e 'ParallaxBackground|ParallaxLayer' -e '\bTileMap\b' \
   -e 'AnimationPlayer\.ANIMATION_(PROCESS|METHOD)' -e 'set_process_callback|set_method_call_mode' \
   -e '\.sync\b' -e 'auto_translate\b' -e 'Image\.create\(' -e 'is_valid_identifier' \
   -e 'set_animation_loop|get_animation_loop' -e 'add_submenu_item' \
   -e 'NOTIFICATION_MOVED_IN_PARENT' -e 'push_unhandled_input' -e 'move_to_foreground' \
   -e 'map_force_update' -e 'make_polygons_from_outlines' -e 'region_bake_navigation_mesh' .
```

## GDScript globals

| deprecated | the 4.7 way |
|---|---|
| `Color8(r, g, b, a)` | `Color.from_rgba8(r, g, b, a)` |
| `convert(value, type)` | `type_convert(value, type)` |
| `inst_to_dict()` / `dict_to_inst()` | `JSON.from_native()` / `JSON.to_native()`, or your own dictionary from `get_property_list()` |
| `type_exists("Name")` | `ClassDB.class_exists("Name")` |

## Nodes and resources

| deprecated | the 4.7 way |
|---|---|
| `TileMap` | one `TileMapLayer` node per layer (converter: `headless-tools.md`) |
| `TileMap.force_update()` | `notify_runtime_tile_data_update()` and/or `update_internals()` |
| `TileData.get_occluder()` / `set_occluder()` | `get_occluder_polygon()` / `set_occluder_polygon()` |
| `ParallaxBackground`, `ParallaxLayer` | `Parallax2D` |
| `AnimatedTexture` | no replacement; it does not work right in 4.7. Use `AnimatedSprite2D` / `SpriteFrames` or an `AnimationPlayer` |
| `AStarGrid2D.size` | `region` |
| `AudioEffectLimiter` | `AudioEffectHardLimiter` |
| `CollisionShape3D`, `GridMap`, `ShapeCast3D` `resource_changed()` | connect to the resource's `changed` signal |
| `CSGShape3D.snap` | nothing; Manifold CSG does not snap |
| `GeometryInstance3D.gi_lightmap_scale` and `LIGHTMAP_SCALE_*` | `gi_lightmap_texel_scale` |
| `HingeJoint3D` `angular_limit/softness`, `PARAM_LIMIT_SOFTNESS` | nothing; the engine never used them |
| `Image.create()` | `Image.create_empty()` |
| `InputEventJoypadButton.pressure` | nothing; always 0 |
| `LightmapGIData.light_texture` | `lightmap_textures` |
| `MultiMesh` `transform_array`, `transform_2d_array`, `color_array`, `custom_data_array` | `set_instance_transform()` / `set_instance_transform_2d()` / `set_instance_color()` / `set_instance_custom_data()` (and the `get_` forms) |
| `Node.NOTIFICATION_MOVED_IN_PARENT` | `NOTIFICATION_CHILD_ORDER_CHANGED` (the old one is never sent) |
| `PackedDataContainer`, `PackedDataContainerRef` | `var_to_bytes()` or `FileAccess.store_var()`; compress with `PackedByteArray.compress()` or `FileAccess.open_compressed()` |
| `PopupMenu.add_submenu_item()` / `get_item_submenu()` / `set_item_submenu()` | `add_submenu_node_item()` / `get_item_submenu_node()` / `set_item_submenu_node()` |
| `Resource.setup_local_to_scene()`, signal `setup_local_to_scene_requested` | override `_setup_local_to_scene()` |
| `RichTextLabel.is_ready()` | `is_finished()` |
| `SpriteFrames.get_animation_loop()` / `set_animation_loop()` | `get_animation_loop_mode()` / `set_animation_loop_mode()` |
| `String` / `StringName` `is_valid_identifier()` | `is_valid_ascii_identifier()` |
| `SplitContainer.split_offset`, `get_drag_area_control()` | `split_offsets[0]`, `get_drag_area_controls()[0]` |
| `SurfaceTool.generate_lod()` | `ImporterMesh.generate_lods()` |
| `TabContainer.all_tabs_in_front` | nothing; tabs are always in front |
| `TextEdit.get_selection_line()` / `get_selection_column()` | `get_selection_origin_line()` / `get_selection_origin_column()` |
| `TextEdit.adjust_carets_after_edit()`, `get_caret_index_edit_order()` | not needed; use `get_sorted_carets()` if you need an order |
| `TreeItem.set_custom_draw()` | `set_custom_draw_callback()` |
| `Viewport.push_unhandled_input()` | `push_input()` |
| `Window.move_to_foreground()` | `grab_focus()` |
| `Control` / `Window` `LAYOUT_DIRECTION_LOCALE` | `LAYOUT_DIRECTION_APPLICATION_LOCALE` |
| `Control.auto_translate`, `Window.auto_translate` | `Node.auto_translate_mode` and `Node.can_auto_translate()` |
| `ColorPicker.MODE_RAW` | `MODE_LINEAR` |
| `TranslationServer` / `TranslationDomain` `get_translation_object()` | `find_translations()` |
| `ResourceImporterOggVorbis.load_from_buffer()` / `load_from_file()` | `AudioStreamOggVorbis.load_from_buffer()` / `load_from_file()` |
| `Script.instance_has(obj)` | `obj.get_script() == script` |

## Animation

| deprecated | the 4.7 way |
|---|---|
| `AnimationPlayer` / `AnimationTree` `ANIMATION_PROCESS_PHYSICS/IDLE/MANUAL` | `AnimationMixer.ANIMATION_CALLBACK_MODE_PROCESS_PHYSICS/IDLE/MANUAL` |
| `AnimationPlayer.ANIMATION_METHOD_CALL_DEFERRED/IMMEDIATE` | `AnimationMixer.ANIMATION_CALLBACK_MODE_METHOD_DEFERRED/IMMEDIATE` |
| `set_process_callback()` / `get_process_callback()` | the `callback_mode_process` property |
| `AnimationPlayer.set_method_call_mode()` / `get_method_call_mode()` | the `callback_mode_method` property |
| `AnimationPlayer.set_root()` / `get_root()` | the `root_node` property |
| `AnimationNodeBlendSpace1D/2D.sync` | `sync_mode` |
| `AnimationNode._process()` | still the only GDScript hook; marked for a future replacement |
| `SkeletonIK3D` | an `IKModifier3D` node (`TwoBoneIK3D`, `FABRIK3D`, `CCDIK3D`, `JacobianIK3D`, `SplineIK3D`); `interpolation` → `SkeletonModifier3D.influence` |
| `SkeletonModifier3D._process_modification()` | `_process_modification_with_delta()` |
| `Skeleton3D` bone global pose override methods, `physical_bones_*`, `animate_physical_bones` | marked "may change"; for ragdolls read `godot-physics-system` |

## Navigation

| deprecated | the 4.7 way |
|---|---|
| `NavigationMeshGenerator.bake()`, `NavigationServer3D.region_bake_navigation_mesh()` | `NavigationServer3D.parse_source_geometry_data()` into a `NavigationMeshSourceGeometryData3D`, then `bake_from_source_geometry_data()` (or `NavigationRegion3D.bake_navigation_mesh()`) |
| `NavigationPolygon.make_polygons_from_outlines()` | `NavigationServer2D.parse_source_geometry_data()` and `bake_from_source_geometry_data()` (or `NavigationRegion2D.bake_navigation_polygon()`) |
| `NavigationRegion2D/3D.get_region_rid()` | `get_rid()` |
| `NavigationServer2D/3D.map_force_update()` | do not use; it breaks with async region updates. Wait one physics frame for the map to sync |

## Multiplayer and XR

| deprecated | the 4.7 way |
|---|---|
| `SceneReplicationConfig.property_set_sync()` / `property_set_watch()` | `property_set_replication_mode()` with `REPLICATION_MODE_ALWAYS` / `REPLICATION_MODE_ON_CHANGE` |
| `SceneReplicationConfig.property_get_sync()` / `property_get_watch()` | `property_get_replication_mode()` |
| `XRInterface` `start_passthrough()` / `stop_passthrough()` / `is_passthrough_*()` | `environment_blend_mode` = `XR_ENV_BLEND_MODE_ALPHA_BLEND` / `XR_ENV_BLEND_MODE_OPAQUE`; `get_supported_environment_blend_modes()` |
| `XRPositionalTracker.get_input()` / `set_input()` | the same calls on `XRControllerTracker` |
