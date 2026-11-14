# UI, advanced

Target engine: Godot 4.7. How Control layout, theme lookup, text rendering, focus and windows
work inside the engine, and the techniques that keep large interfaces correct and cheap.

## Use this shelf when

- A layout fights you: sizes jump, a container ignores a size, or a resize is slow.
- You build a custom Control or container, or a theme used by many screens.
- A menu must work with a controller or keyboard alone.
- Text is blurry, stutters on first display, or a long log freezes the frame.
- A list has thousands of rows, or UI sits in a `SubViewport` or in 3D.
- A click does not reach the game.

## How the engine works here

- A Control's rectangle comes from four anchors (0.0 to 1.0 of the parent rectangle) plus four
  pixel offsets. A `Container` parent takes this over: it writes position and size for each child
  on every sort, so manual values are lost. Children only steer it through size flags,
  `size_flags_stretch_ratio` and their minimum size.
- Minimum size flows upward. Each control reports `get_combined_minimum_size()`: the larger of
  `custom_minimum_size` and `_get_minimum_size()`. A change calls `update_minimum_size()`, which
  drops the cached value in that node and in every parent up to the top level. Containers then
  queue one sort. One changed label deep in a tree therefore touches the whole chain above it.
- A sort is the notification `NOTIFICATION_SORT_CHILDREN`, wrapped by the signals
  `pre_sort_children` and `sort_children`. `queue_sort()` requests one.
- Drawing is retained. `_draw()` runs only after `queue_redraw()`, at most once per frame, and
  the commands are kept until the next redraw. Static UI costs draw calls but no script time.
- Theme item lookup order: local override on the control, then the `theme` of the control and
  each Control ancestor (nearest first), then the project theme (`gui/theme/custom`), then the
  engine default theme. At each step the names tried are the type variation, the class, then
  each parent class. A non-Control node in the chain stops the walk up.
- Mouse events go to the control under the cursor and bubble up by `mouse_filter`. Key and
  joypad events go to the viewport's focus owner only.

## Advanced techniques

### Custom container

Extend `Container`, place children in the sort notification and report a minimum size. Call
`queue_sort()` and `update_minimum_size()` when one of your own settings changes.

```gdscript
extends Container

const GAP: float = 8.0

func _get_minimum_size() -> Vector2:
	var total := Vector2.ZERO
	for child: Node in get_children():
		var c := child as Control
		if c != null and c.visible:
			var m: Vector2 = c.get_combined_minimum_size()
			total = Vector2(maxf(total.x, m.x), total.y + m.y + GAP)
	return total

func _notification(what: int) -> void:
	if what == NOTIFICATION_SORT_CHILDREN:
		var y: float = 0.0
		for child: Node in get_children():
			var c := child as Control
			if c != null and c.visible:
				var h: float = c.get_combined_minimum_size().y
				fit_child_in_rect(c, Rect2(0.0, y, size.x, h))
				y += h + GAP
```

### Keep layout work small

- Give text that changes every frame (timers, counters) a fixed `custom_minimum_size`, or clip
  it, so its minimum size stays constant and no parent re-sorts.
- Wrap many theme override calls in `begin_bulk_theme_override()` and `end_bulk_theme_override()`
  so `NOTIFICATION_THEME_CHANGED` fires once.
- Animate with `offset_transform_*` (4.7). It is a visual transform that containers do not
  overwrite and that does not trigger layout. With `offset_transform_visual_only` true, hit
  testing stays at the laid out position.

### Maximum size (4.7)

`custom_maximum_size` caps a control. It wins over the minimum size. `propagate_maximum_size`
on a parent passes the cap down to children (containers have it on by default). A
`RichTextLabel` with `fit_content` and autowrap needs such a cap to wrap correctly.

### Theme architecture for many screens

- One project theme. Variants are type variations: a type such as `DangerButton` marked as a
  variation of `Button` overrides some items and inherits the rest. Set
  `theme_type_variation` on the node. Keep all variations in the project theme so that the
  inspector lists them.
- Keep local overrides for layout values (separation, margins), not for look.
- A second `Theme` on the root of a branch (a popup, a faction skin) restyles that branch.
  `Theme.merge_with()` builds a combined theme at runtime (base plus high contrast).
- Custom controls read items with `get_theme_color("name", "MyType")` and friends, and refresh
  on `NOTIFICATION_THEME_CHANGED`.
- The `focus` StyleBox is drawn over the normal one. Design it as an outline.
- `StyleBoxFlat` rounded corners cost vertices: `corner_detail` 4 to 5 is enough below radius
  10, 8 to 12 below radius 30. Value 1 gives chamfered corners; turn `anti_aliasing` off then.

### Scale: three different knobs

- `Window.content_scale_factor` on the root (project setting `display/window/stretch/scale`):
  scales all 2D, overridden themes included. Can change at runtime.
- `gui/theme/default_theme_scale`: base scale of the default theme, read at start only.
- `ThemeDB.fallback_base_scale` and `Theme.default_base_scale`: per theme, runtime.

Offer a UI scale option through `content_scale_factor`. The OS scale factor is only reported on
macOS, Android and Linux with Wayland; on Windows and X11 the player must be able to set it.
`display/window/stretch/scale_mode` set to `integer` floors the factor for pixel art.

### Fonts

- MSDF (`multichannel_signed_distance_field` in the import settings): one atlas serves every
  size, with no new rasterization when the size animates, and no grain when scaled down or in
  `Label3D`. Costs: higher baseline render cost on low-end mobile, no hinting so small text is
  softer, no LCD subpixel mode. Outline size is limited to half of `msdf_pixel_range`
  (default 16, so 8 pixels).
- Rasterized fonts create glyphs per size on first use. Pre-render the needed sizes and
  character sets in the advanced import settings to avoid a hitch on first display.
- With stretch mode `canvas_items`, font oversampling re-rasterizes at the real resolution
  (`gui/fonts/dynamic_fonts/use_oversampling`). Per viewport: `Viewport.oversampling` and
  `oversampling_override`. Scaled-down rasterized text needs mipmaps to avoid grain.
- `FontVariation` wraps one font file with variable axes, OpenType features (slashed zero,
  ligatures off), spacing, faux bold and its own `fallbacks`.
- Missing glyphs fall through `Font.fallbacks`, then system fonts (`allow_system_fallback`).
  `SystemFont` loads by name with zero pack size, but metrics differ per platform, so leave
  spare room in layouts. It works on desktop and iOS; Android support is unreliable.
- Pixel fonts: nearest filter, subpixel positioning off, whole multiples of the design size.

### RichTextLabel at scale

- `append_text()` parses only the added part. Assigning `text` parses everything again and
  wipes content added by `append_text()` and the `push_*()` calls.
- The `push_*()` and `pop()` calls skip BBCode parsing completely.
- `threaded` moves shaping off the main thread. It does not make it faster; use it for
  logs of thousands of lines only.
- Escape player text: replace `[` with `[lb]`, in names too.
- A custom effect is a `RichTextEffect` script with a `bbcode` variable and
  `_process_custom_fx()`. One installed effect makes the label process every frame, so keep
  effects off idle screens.

### Controller-first focus

- Nothing has focus at start. Call `grab_focus()` on open (deferred inside `_ready()`), and
  again when a submenu closes. A control that becomes hidden loses focus.
- Unset neighbors are guessed by geometry. Set `focus_neighbor_*` where the layout is irregular.
  `find_valid_focus_neighbor()` shows what the engine would choose.
- `Viewport.gui_get_focus_owner()` and the `gui_focus_changed` signal give one global place for
  focus sounds, a custom focus cursor, and scrolling. `ScrollContainer.follow_focus` scrolls to
  the focused child.
- `focus_behavior_recursive` (4.5) set to disabled removes a whole branch from focus, and
  `mouse_behavior_recursive` does the same for the mouse. One property blocks a screen
  behind a dialog.
- `grab_focus(true)` (4.6) takes focus without the focus visual, for mouse-driven flows.
- Do not use the `ui_*` actions for gameplay. Focus navigation consumes them.

### Long lists

`Tree` and `ItemList` are single controls that draw their rows themselves, so a row is data, not
a node. Prefer them over one scene per row for long lists. `Tree` cells support buttons,
checkboxes, inline edit and custom drawing through `TreeItem.set_custom_draw_callback()`. When rows must be full scenes, recycle: keep only the
visible count of row nodes plus one, drive them from a `VScrollBar` value, and rebind data on
scroll. This is a pattern, not an engine feature.

### UI in a SubViewport or in 3D

A `SubViewport` gets input only from a `SubViewportContainer` parent. For a screen in 3D, convert
the hit position to viewport pixels and call `Viewport.push_input()`. Set
`render_target_update_mode` to `UPDATE_ONCE` or `UPDATE_WHEN_VISIBLE` so a static panel is not
rendered every frame. `SubViewportContainer.stretch` resizes the viewport to the control.

### Windows and popups

With `display/window/subwindows/embed_subwindows` on (default), dialogs and popups are drawn
inside the main window. Off, they are OS windows. `Window.force_native` picks per window.
`transient` plus `exclusive` gives a modal window. `popup_window` closes on an outside click.
Tooltips: `_make_custom_tooltip()` returns a fresh Control each time; the engine frees it.

## Niche but useful

- `set_drag_forwarding()` handles drag and drop for a child from its owner, with no subclass.
- `force_drag()` starts a drag from code.
- `_has_point()` gives a control a non-rectangular hit area.
- `Viewport.gui_get_hovered_control()` with `is_ancestor_of()` answers "is the mouse over the
  UI" without a flag per panel.
- `FoldableContainer` (4.5) with a shared `foldable_group` gives an accordion.
- `ScrollContainer.scroll_hint_mode` (4.6) shows that more content exists.
- `application/run/low_processor_mode` redraws only on change. The editor menu
  Debug > Debug Canvas Item Redraws flashes each redrawn area, which finds a hidden animation.

## Traps

- `Control` defaults to `MOUSE_FILTER_STOP`. A full-rect root Control eats every click before
  `_unhandled_input()`. Containers and `TextureRect` default to pass, `Label` to ignore.
- Wheel events pass through stop filters (`mouse_force_pass_scroll_events`). Turn it off on
  the UI root if the game zooms on wheel in `_unhandled_input()`.
- `clip_contents` clips input as well as drawing.
- Scaling a `SubViewportContainer` distorts its content. Change its size.

## Version notes

- 4.5: `FoldableContainer`, recursive mouse and focus behavior, screen reader support
  (`accessibility_name`, `FOCUS_ACCESSIBILITY`).
- 4.6: `grab_focus(hide_focus)`, scroll hints.
- 4.7: `custom_maximum_size`, `offset_transform_*`, `RichTextLabel.add_image()` size units.

## Related skills

- `godot-ui`: node choice, anchors, theme setup and common screens, step by step.
- `godot-responsive-ui`: stretch mode and aspect settings per project type.
- `godot-hud-system`: HUD scenes on a `CanvasLayer`.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/ui/size_and_anchors.html
- https://docs.godotengine.org/en/stable/tutorials/ui/gui_containers.html
- https://docs.godotengine.org/en/stable/tutorials/ui/custom_gui_controls.html
- https://docs.godotengine.org/en/stable/tutorials/ui/gui_navigation.html
- https://docs.godotengine.org/en/stable/tutorials/ui/gui_skinning.html
- https://docs.godotengine.org/en/stable/tutorials/ui/gui_theme_type_variations.html
- https://docs.godotengine.org/en/stable/tutorials/ui/gui_using_fonts.html
- https://docs.godotengine.org/en/stable/tutorials/ui/bbcode_in_richtextlabel.html
- https://docs.godotengine.org/en/stable/tutorials/ui/creating_applications.html
- https://docs.godotengine.org/en/stable/tutorials/rendering/multiple_resolutions.html
