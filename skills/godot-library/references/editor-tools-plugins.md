# Editor tools and plugins

Target engine: Godot 4.7. How to run code in the editor, extend it with plugins, and drive it from the command line, with the rules that keep scenes and undo history intact.

## Use this shelf when

- Designers need a level tool: a gizmo, a custom inspector, a dock, a viewport click tool.
- You want a preview in the editor (`@tool`) for a node that draws or builds itself.
- A batch edit must touch many scenes or resources, by hand once or headless in CI.
- A custom file format should import like a built-in resource.
- You ship an addon that must not collide with the host project.

## How the engine works here

- The edited scene lives inside the editor's own scene tree, so a `@tool` script runs in the editor process. Nothing protects the editor: freeing a node it still uses crashes it.
- A script without `@tool` is not instantiated in the editor: its instance methods and members fail. Static methods, constants and enums work. Static variables read as `null` with no warning.
- `@tool` is not inherited. A script that extends a tool script needs its own `@tool`.
- Nodes added by code are invisible in the Scene dock and are not saved unless `owner` is the edited scene root (`get_tree().edited_scene_root`, or `get_scene()` in an `EditorScript`). Code changes do not mark the scene unsaved by themselves.
- Undo is per scene. `EditorUndoRedoManager` keeps one history per open scene plus a global one, chosen from the first object in the action: a `Node` uses the edited scene, a built-in resource its scene, anything else the global history.
- `EditorInterface` is a singleton with static access. It exists only in editor builds.
- A plugin is `addons/<name>/plugin.cfg` plus a `@tool` script that extends `EditorPlugin`. `_enter_tree` and `_exit_tree` run on every editor start; `_enable_plugin` and `_disable_plugin` only when the user toggles it.
- `project.godot` stores only values that differ from the default. Many settings are read once at startup, so a later `set_setting` changes nothing at runtime.

## Advanced techniques

### `@tool` with guarded sides

Split editor and game code with `Engine.is_editor_hint()`. Inspector edits call the property setter, also for in-place `Array` and `Dictionary` edits (at runtime only assignment does). A sub-resource edit does not call the setter: make the resource a tool script that calls `emit_changed()`, connect `changed` in the setter, and disconnect the old resource first. Call `notify_property_list_changed()` after code changes an exported value, and `update_configuration_warnings()` when the result of `_get_configuration_warnings()` changes.

### Tool buttons in the inspector (since 4.4)

```gdscript
@tool
extends Node3D

@export var radius: float = 2.0
@export_tool_button("Rebuild", "Callable") var rebuild_action = rebuild

func rebuild() -> void:
	print("Rebuild with radius ", radius)
	update_gizmos()
```

The exported variable is left untyped on purpose: that is the form the class reference shows. The icon must be a built-in editor icon name (`"Callable"` is the default). This replaces the old "bool export as a button" trick. The call has no undo by itself; wrap real edits in an undo action as shown next.

### Always go through `EditorUndoRedoManager`

```gdscript
@tool
extends EditorPlugin

func add_marker(parent: Node3D, at: Vector3) -> void:
	var marker := Marker3D.new()
	marker.position = at
	var undo_redo: EditorUndoRedoManager = get_undo_redo()
	undo_redo.create_action("Add marker")
	undo_redo.add_do_method(parent, "add_child", marker, true)
	undo_redo.add_do_property(marker, "owner", EditorInterface.get_edited_scene_root())
	undo_redo.add_do_reference(marker)
	undo_redo.add_undo_method(parent, "remove_child", marker)
	undo_redo.commit_action()
```

The action marks the scene unsaved. `add_do_reference` keeps the new node alive while it sits in history (nodes only). Pass `custom_context` to `create_action` when the first object would pick the wrong history. Since 4.5, `mark_unsaved = false` suits view-only actions. `UndoRedo.MERGE_ENDS` folds a drag into one step. Outside a plugin use `EditorInterface.get_editor_undo_redo()`.

### One-off batch edits with `EditorScript`

A `@tool` script that extends `EditorScript` and implements `_run()`. Run it with **File > Run** (Ctrl+Shift+X), from the FileSystem dock, or, with a `class_name`, from the command palette (since 4.5), which turns a folder of such scripts into a project command list. No undo: save first, and call `EditorInterface.mark_scene_as_unsaved()` after edits. Skip children of instanced scenes (`node.owner != get_scene()`): edits to them are lost on reload.

### Headless tools

```sh
godot --headless --path . --import
godot --headless --path . --script res://tools/check_levels.gd -- --strict
```

A `--script` file must extend `SceneTree` or `MainLoop` and call `quit()`. Arguments after `--` come from `OS.get_cmdline_user_args()`. `--import` implies `--editor --quit`. Unknown flags give no warning. `--recovery-mode` opens a project with tool scripts, plugins and GDExtensions off, for when tool code crashes the editor at load. `godot-headless-runs` has the full run procedure. The core team lists reliable headless export as an open priority, so in CI check that the output file exists; do not trust the exit code alone.

### Custom inspectors

`EditorInspectorPlugin`: `_can_handle(object)` selects objects (resources included). `_parse_begin`, `_parse_category`, `_parse_group`, `_parse_end` add controls with `add_custom_control`. `_parse_property` returns `true` to remove the default editor after `add_property_editor(name, editor)`. The editor is an `EditorProperty`: build children in `_init`, call `add_focusable`, refresh in `_update_property`, and report edits with `emit_changed(get_edited_property(), value)`, which gives undo for free. `set_bottom_editor` places a wide control under the label. For small needs use `_validate_property` and `_get_property_list` in the tool script: no plugin needed.

### 3D gizmos with handles

Register an `EditorNode3DGizmoPlugin` with `add_node_3d_gizmo_plugin`. Implement `_get_gizmo_name`, `_has_gizmo(node)`, and `_redraw(gizmo)`: `gizmo.clear()`, then `add_lines`, `add_mesh`, `add_handles`. Create materials once in `_init` (`create_material`, `create_handle_material`) and fetch them with `get_material(name, gizmo)` so selection tint works. Handles need `_get_handle_name`, `_get_handle_value`, `_set_handle` (drag) and `_commit_handle` (create the undo action; `restore` is the old value). Return an `EditorNode3DGizmo` subclass from `_create_gizmo` for per-gizmo state. The node calls `update_gizmos()` when data changes.

### Viewport tools

Return `true` from `_handles(object)` for one type, keep the object in `_edit` (it gets `null` on deselect). Then `_forward_3d_gui_input(camera, event)` and `_forward_canvas_gui_input(event)` receive viewport input: return `AFTER_GUI_INPUT_STOP` to consume, `AFTER_GUI_INPUT_PASS` to forward. Draw overlays in `_forward_3d_draw_over_viewport` and request them with `update_overlays()`. `set_input_event_forwarding_always_enabled()` gives input without a selection.

### Docks (new API since 4.6)

4.6 unified the dock system: bottom panels are docks, docks move between the sides and the bottom, and most can float. `EditorDock` (a `MarginContainer`, still marked experimental) is the root of a dock and owns its title, slots and saved layout. Old plugin calls keep working through compatibility code. Create an `EditorDock`, add your scene as a child, set `title`, `default_slot` (`EditorDock.DOCK_SLOT_*`, bottom slots included) and `available_layouts`, then `add_dock(dock)`. In `_exit_tree`: `remove_dock(dock)` and `queue_free()`. `add_control_to_dock` and `add_control_to_bottom_panel` are deprecated. A dock is an accessibility landmark by default since 4.7 (`accessibility_region`). `transient` suits docks that open only for a selected node type.

### Custom settings

In `_enter_tree`: `ProjectSettings.set_setting` if `has_setting` is false, then `set_initial_value`, then `add_property_info` with `name`, `type`, `hint`, `hint_string`. Do this on every start. Do not pass a `usage` key (warns since 4.5); use `set_as_basic`, `set_restart_if_changed`, `set_as_internal`. Per-user values go in `EditorSettings` (`set_setting`, `add_property_info`, signal `settings_changed`), per-project editor state in `EditorSettings.set_project_metadata`, per-scene view state in `_get_state` and `_set_state` (needs `_get_plugin_name`).

### Import plugins

`EditorImportPlugin` needs `_get_importer_name` (unique, stored in `.import`), `_get_recognized_extensions`, `_get_save_extension`, `_get_resource_type`, the preset and option methods (`_get_import_options` must always return an array) and `_import`. `save_path` has no extension. Push feature tags into `r_platform_variants` for per-platform variants and extra files into `r_gen_files`.

### Export, debugger and context menu plugins

- `EditorExportPlugin`: `_export_file` can `skip()` or `add_file`; `_customize_scene` and `_customize_resource` rewrite content per preset (strip debug nodes) and are cached by `_get_customization_configuration_hash`.
- `EditorDebuggerPlugin`: `_has_capture(prefix)` and `_capture` receive messages the game sends with `EngineDebugger.send_message("prefix:name", data)`. Reply with `EditorDebuggerSession.send_message`; `add_session_tab` shows a tab. Use it for a live game-state inspector.
- `EditorContextMenuPlugin` (since 4.4): `add_context_menu_plugin(slot, plugin)` with slots for the scene tree, FileSystem, script editor, scene tabs, 2D editor and inspector properties; add items in `_popup_menu(paths)`.

## Niche but useful

- `add_tool_menu_item`, `EditorInterface.get_command_palette().add_command`: cheap entry points for team tools.
- `EditorSettings.add_shortcut` (since 4.6) registers a plugin shortcut that users can rebind in Editor Settings like a native one.
- `PROPERTY_HINT_GROUP_ENABLE` (since 4.5) in `_get_property_list`: an inspector group with a checkbox in its header, for optional feature blocks.
- Since 4.5 an `@export var x: Variant` gets a type selector in the inspector.
- Since 4.5 the editor language changes without a restart: quick test of plugin translations.
- Since 4.6 scene files store a unique ID per node, so inherited and instanced scenes survive renames and moves of base nodes. Tools that write `.tscn` text must keep them.
- `EditorInterface.get_editor_toaster().push_toast` (since 4.4): non-blocking feedback.
- `EditorInterface.popup_quick_open` (since 4.4): the stock resource picker for tools.
- `_build()` runs before every play; return `false` to block the run.
- `_get_unsaved_status` and `_save_external_data`: join the editor's save prompt.
- `add_autoload_singleton` in `_enable_plugin`: an addon installs its own autoload.
- Sub-plugins: nested `plugin.cfg` folders, toggled by the main plugin with `EditorInterface.set_plugin_enabled("main/sub", true)`.
- `EditorFileSystem.get_filesystem()` walks project files; `filesystem_changed` signals.
- `EditorResourcePreviewGenerator`: thumbnails for custom resources.
- Plugin translations: `TranslationServer.get_or_add_domain`, then `set_translation_domain` on the dock; `remove_domain` on exit.
- External editors: LSP (port 6005) and DAP (6006) need a running editor.
- Debug a tool script: **Debug > Customize Run Instances**, add `--editor` to run args.

## Traps

- `queue_free()` in tool code on nodes the editor tracks can crash it.
- No `owner`: the node is not saved.
- Every script a plugin uses must be `@tool`, or it acts like an empty file.
- `add_custom_type` makes no real class; prefer `class_name` with `@icon`.
- Global `class_name` in an addon collides with host projects: prefix names, or preload by path. Keep everything under `addons/<name>/` and free all you added in `_exit_tree`.
- C# plugins must be built before they can be enabled.
- A bug where `remove_control_from_docks` freed the passed control was fixed in 4.7 beta 5; new code should use the `EditorDock` API.
- Scripts and shaders have `.uid` files since 4.4: a tool that moves or deletes them on disk must move or delete the `.uid` file too.

## Version notes

- 4.4: `@export_tool_button`; `EditorContextMenuPlugin`; `EditorToaster`; `popup_quick_open`; `.uid` files; filesystem scans up to 3x faster on large projects (release page); `EditorTranslationParserPlugin._parse_file` returns an `Array`.
- 4.5: `create_action` gained `mark_unsaved`; named `EditorScript`s in the command palette; `PROPERTY_HINT_GROUP_ENABLE`.
- 4.6: unified docks and `EditorDock`; `EditorSettings` shortcut API; unique node IDs in scene files, `load_steps` dropped; `EditorFileDialog.add_side_menu` removed.
- 4.7: `EditorInterface.get_unsaved_scenes` and `ScriptEditor.get_unsaved_files` (absent from the 4.6 class reference); `EditorVCSInterface._commit` gained `amend`.

## Related skills

- `godot-addon-development`: step-by-step plugin setup and checklist.
- `godot-headless-runs`: running tool scripts and imports without a window.
- Sibling shelves: `assets-import`, `design-iteration-toolkit`, `debugging-and-profiling-tools`.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/plugins/running_code_in_the_editor.html
- https://docs.godotengine.org/en/stable/tutorials/plugins/editor/making_plugins.html
- https://docs.godotengine.org/en/stable/tutorials/plugins/editor/import_plugins.html
- https://docs.godotengine.org/en/stable/tutorials/plugins/editor/3d_gizmos.html
- https://docs.godotengine.org/en/stable/tutorials/plugins/editor/inspector_plugins.html (and `making_main_screen_plugins`, `visual_shader_plugins`, `installing_plugins`, `internationalizing_plugins` in that folder)
- https://docs.godotengine.org/en/stable/tutorials/editor/command_line_tutorial.html (and `project_settings`, `external_editor`, `customizing_editor`, `managing_editor_features`, `script_editor` in that folder)
- https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_exports.html
- https://godotengine.org/releases/4.4/ , https://godotengine.org/releases/4.5/ , https://godotengine.org/releases/4.6/ , https://godotengine.org/releases/4.7/
- https://godotengine.org/article/dev-snapshot-godot-4-6-beta-1/ (dock rework)
- https://godotengine.org/article/dev-snapshot-godot-4-5-beta-1/
- https://godotengine.org/article/dev-snapshot-godot-4-7-beta-5/
- https://godotengine.org/article/uid-changes-coming-to-godot-4-4/
- https://godotengine.org/priorities/
