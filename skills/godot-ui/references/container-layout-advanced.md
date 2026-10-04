Adds container layout beyond the basic boxes: weighted size flags, wrapping and aspect-locked layouts, width-driven grids, custom containers, scroll timing, 3D previews in UI and long virtual lists; read it when a layout must adapt, scale to thousands of rows, or does something no built-in container does.

# Container Layout: Advanced Patterns

> ← Back to [SKILL.md](../SKILL.md). Container basics are in SKILL.md section 2. Screen-size breakpoints and stretch modes are in **godot-responsive-ui**.

All code targets Godot 4.7.

---

## 1. The Container Owns Its Children's Rects

A `Container` sets the position and size of each child every time it sorts.
Code that sets a child's `position` or `size` is undone at the next sort.
Control a child through:

- `custom_minimum_size` — the smallest size the child accepts.
- `size_flags_horizontal` / `size_flags_vertical` — fill, expand, shrink.
- `size_flags_stretch_ratio` — the share of the extra space among the
  children that expand.

The default flag is `SIZE_FILL` without `SIZE_EXPAND`, so a child does not
take extra space until you add `SIZE_EXPAND`.

```gdscript
extends HBoxContainer

## A 1:3 sidebar/content split that keeps the ratio as the window resizes.
func _ready() -> void:
	var sidebar := PanelContainer.new()
	var content := PanelContainer.new()
	for c: Control in [sidebar, content]:
		c.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		add_child(c)
	sidebar.size_flags_stretch_ratio = 1.0
	content.size_flags_stretch_ratio = 3.0
	sidebar.custom_minimum_size.x = 180.0
	add_theme_constant_override(&"separation", 12)
```

To animate a child "pushing" its siblings, tween its `custom_minimum_size`,
not its `position`. To animate a child without moving its siblings, use the
4.7 `offset_transform_*` properties (see [ui-patterns.md](ui-patterns.md#godot-47-additions)).

## 2. Pick the Container for the Shape

| Layout | Container | Note |
|---|---|---|
| Chips or tags that wrap to the next line | `HFlowContainer` / `VFlowContainer` | Line breaks follow the width. `alignment` sets how each line is placed. |
| A fixed number of columns | `GridContainer` | `columns` defaults to 1. It never wraps by itself. |
| A card that keeps 2:3 at any size | `AspectRatioContainer` | `ratio` is width / height; `stretch_mode = STRETCH_FIT` keeps the child inside. |
| One child centered | `CenterContainer` | |
| User-resizable panes | `HSplitContainer` / `VSplitContainer` | `split_offsets` holds one offset per divider. |
| A 3D or 2D scene inside UI | `SubViewportContainer` | See section 5. |

```gdscript
extends HFlowContainer

## Fills a wrapping tag cloud. Each tag keeps a 2:3 card next to it.
func add_tag(label_text: String, icon: Texture2D) -> void:
	var row := HBoxContainer.new()
	var frame := AspectRatioContainer.new()
	frame.ratio = 2.0 / 3.0
	frame.stretch_mode = AspectRatioContainer.STRETCH_FIT
	frame.custom_minimum_size = Vector2(24.0, 36.0)
	var image := TextureRect.new()
	image.texture = icon
	image.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	image.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_CENTERED
	frame.add_child(image)
	row.add_child(frame)
	var label := Label.new()
	label.text = label_text
	row.add_child(label)
	add_child(row)
```

## 3. A Grid Whose Column Count Follows Its Width

`GridContainer` needs `columns` set. Recompute it when the container resizes,
from the cell's minimum width plus the theme's `h_separation`.

```gdscript
extends GridContainer

@export var cell_min_width: float = 96.0


func _ready() -> void:
	resized.connect(_update_columns)
	_update_columns()


func _update_columns() -> void:
	var gap := float(get_theme_constant(&"h_separation"))
	var fit := floori((size.x + gap) / (cell_min_width + gap))
	var wanted := maxi(1, fit)
	if wanted != columns:
		columns = wanted
```

## 4. A Custom Container

When no built-in container gives the shape (a radial menu, a fan of cards),
extend `Container`. Lay the children out on `NOTIFICATION_SORT_CHILDREN` with
`fit_child_in_rect()`, and call `queue_sort()` when a layout setting changes.

```gdscript
@tool
extends Container

## Places visible Control children on a circle.
@export var radius: float = 120.0:
	set(value):
		radius = value
		queue_sort()
@export var start_angle_deg: float = -90.0:
	set(value):
		start_angle_deg = value
		queue_sort()


func _notification(what: int) -> void:
	if what == NOTIFICATION_SORT_CHILDREN:
		_layout()


func _layout() -> void:
	var items: Array[Control] = []
	for child in get_children():
		var c := child as Control
		if c and c.visible:
			items.append(c)
	if items.is_empty():
		return
	var center := size * 0.5
	var step := TAU / items.size()
	for i in items.size():
		var c := items[i]
		var child_size := c.get_combined_minimum_size()
		var angle := deg_to_rad(start_angle_deg) + step * i
		var pos := center + Vector2.from_angle(angle) * radius - child_size * 0.5
		fit_child_in_rect(c, Rect2(pos, child_size))


func _get_minimum_size() -> Vector2:
	return Vector2.ONE * radius * 2.0
```

## 5. A 3D Preview Inside UI

Put a `SubViewport` under a `SubViewportContainer` and set
`stretch = true`, so the viewport follows the container's size. To render at
a lower resolution, raise `stretch_shrink` (2 renders at half size). Do not
scale the container to resize it; that distorts the image.

```gdscript
extends SubViewportContainer

## A character preview with a transparent background and its own 3D world.
func setup(model_scene: PackedScene) -> void:
	stretch = true
	stretch_shrink = 1
	var viewport := SubViewport.new()
	viewport.transparent_bg = true
	viewport.own_world_3d = true
	add_child(viewport)
	var camera := Camera3D.new()
	camera.position = Vector3(0.0, 1.2, 3.0)
	viewport.add_child(camera)
	viewport.add_child(DirectionalLight3D.new())
	viewport.add_child(model_scene.instantiate())
```

`own_world_3d = true` keeps the preview model out of the game world, so the
game's lights and physics do not touch it.

## 6. Scrolling: Wait for the Layout

A child added this frame has no final size until the container sorts. Scroll
to it after one frame, or `ensure_control_visible()` scrolls to the old
position.

```gdscript
extends ScrollContainer

@onready var list: VBoxContainer = $List


func append_row(row: Control) -> void:
	list.add_child(row)
	await get_tree().process_frame
	ensure_control_visible(row)
```

`follow_focus = true` scrolls to whichever child gets keyboard or gamepad
focus, which suits menus that the player steps through.

## 7. Thousands of Rows: Recycle a Few Controls

A `ScrollContainer` with 10,000 child rows lays out and draws all of them.
Show only the rows in view instead:

1. Keep a spacer `Control` whose `custom_minimum_size.y` equals
   `row_count * row_height`, so the scroll bar has the full range.
2. Keep a pool of row controls, enough for one screen plus 2.
3. On scroll, compute the first visible index from `scroll_vertical` and
   place each pool row at `index * row_height` with new data.

```gdscript
extends ScrollContainer

## Fixed-height virtual list. Rows are Labels for brevity.
@export var row_height: float = 28.0
var data: PackedStringArray = PackedStringArray()
var _content := Control.new()
var _pool: Array[Label] = []


func _ready() -> void:
	horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	_content.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(_content)
	get_v_scroll_bar().value_changed.connect(_on_scrolled)
	resized.connect(_rebuild_pool)


func set_data(rows: PackedStringArray) -> void:
	data = rows
	_content.custom_minimum_size = Vector2(0.0, data.size() * row_height)
	_rebuild_pool()


func _rebuild_pool() -> void:
	var needed := ceili(size.y / row_height) + 2
	while _pool.size() < needed:
		var label := Label.new()
		_content.add_child(label)
		_pool.append(label)
	_refresh()


func _on_scrolled(_value: float) -> void:
	_refresh()


func _refresh() -> void:
	var first := floori(scroll_vertical / row_height)
	for i in _pool.size():
		var row := _pool[i]
		var index := first + i
		row.visible = index < data.size()
		if row.visible:
			row.text = data[index]
			row.position = Vector2(0.0, index * row_height)
			row.size = Vector2(size.x, row_height)
```

The spacer is a plain `Control`, not a container, so it does not move the
rows. For a list the player edits or sorts, `ItemList` and `Tree` already
draw only the visible part and may be enough.

## 8. Input Through Layers

An overlay container with `mouse_filter = MOUSE_FILTER_STOP` (the default for
many controls) takes every click in its rect, so buttons under it stop
working. Set `MOUSE_FILTER_IGNORE` on decorative overlays and
`MOUSE_FILTER_PASS` where a parent must also see the event.

## 9. Depth Costs Layout Time

Every nested container sorts when a child changes size. Ten levels of boxes
inside margins inside panels make a resize slow. For static padding, prefer
one `MarginContainer` or anchors and offsets on the child, and keep
containers where the layout really adapts.
