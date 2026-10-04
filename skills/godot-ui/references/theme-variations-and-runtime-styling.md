Adds theme type variations, safe runtime StyleBox changes, theme-aware custom drawing and whole-UI theme swaps; read it when one widget kind needs several looks or the look changes at runtime.

# Theme Variations and Runtime Styling

> ← Back to [SKILL.md](../SKILL.md). Base theme setup is in [theme-system.md](theme-system.md).

All code targets Godot 4.7.

---

## 1. Type Variations Instead of Copied Scenes

A **type variation** is a named theme type that extends a built-in type. A
`DangerButton` variation based on `Button` keeps every `Button` item and
changes only the items you set on it. You do not need a second button scene or
a second `Theme`.

- In the Theme editor: add a type, name it `DangerButton`, and set its base
  type to `Button`.
- In code: `Theme.set_type_variation(&"DangerButton", &"Button")`.
- On the node: set `theme_type_variation = &"DangerButton"`.

```gdscript
extends Node

## Builds a small theme with a "DangerButton" variation and applies it.
func build_theme() -> Theme:
	var theme := Theme.new()
	theme.set_type_variation(&"DangerButton", &"Button")
	theme.set_color(&"font_color", &"DangerButton", Color(1.0, 0.85, 0.85))

	var normal := StyleBoxFlat.new()
	normal.bg_color = Color(0.55, 0.08, 0.08)
	normal.set_corner_radius_all(6)
	theme.set_stylebox(&"normal", &"DangerButton", normal)
	return theme


func make_delete_button(root: Control) -> Button:
	root.theme = build_theme()
	var button := Button.new()
	button.text = "Delete save"
	button.theme_type_variation = &"DangerButton"
	root.add_child(button)
	return button
```

A variation can also hold your palette. Add a custom type such as `Palette`
with color items (`primary`, `danger`), and read them from code with
`theme.get_color(&"primary", &"Palette")`. The colors then have one source.

## 2. Change a StyleBox at Runtime: Duplicate First

`get_theme_stylebox()` returns the **shared** resource from the theme. If you
change its `bg_color`, every control that uses the theme changes too.
Duplicate it, change the copy, and set the copy as an override on one node.

```gdscript
extends PanelContainer

## Tints only this panel. The theme's StyleBox stays unchanged.
func set_tint(color: Color) -> void:
	var shared := get_theme_stylebox(&"panel")
	var own := shared.duplicate() as StyleBox
	if own is StyleBoxFlat:
		(own as StyleBoxFlat).bg_color = color
	add_theme_stylebox_override(&"panel", own)


func clear_tint() -> void:
	remove_theme_stylebox_override(&"panel")
```

When you set many overrides at once, wrap them in
`begin_bulk_theme_override()` and `end_bulk_theme_override()`. The control
then sends one theme-changed update, not one per override.

To animate a style (for example a pulsing border), tween a property of the
duplicated StyleBox, never of the shared one. For a visual-only scale or
shake that must not move siblings, use the 4.7 `offset_transform_*`
properties described in [ui-patterns.md](ui-patterns.md#godot-47-additions).

## 3. Theme-Aware Custom Drawing

A control that draws in `_draw()` must read its colors and fonts from the
theme, or a theme swap leaves it behind. Read the items with
`get_theme_color()`, `get_theme_font()` and `get_theme_font_size()`, and
redraw on `NOTIFICATION_THEME_CHANGED`.

Keep resources that you pass to draw calls as members. A StyleBox created as
a local variable inside `_draw()` can be freed before the renderer uses it.

```gdscript
extends Control

## A bar chart that follows the active theme.
var values: PackedFloat32Array = PackedFloat32Array([0.2, 0.7, 0.4, 0.9])
var _bar_box: StyleBoxFlat = StyleBoxFlat.new()


func _notification(what: int) -> void:
	if what == NOTIFICATION_THEME_CHANGED:
		_bar_box.bg_color = get_theme_color(&"font_color", &"Label")
		queue_redraw()


func _draw() -> void:
	if values.is_empty():
		return
	var bar_width := size.x / values.size()
	for i in values.size():
		var h := size.y * clampf(values[i], 0.0, 1.0)
		var rect := Rect2(i * bar_width + 2.0, size.y - h, bar_width - 4.0, h)
		draw_style_box(_bar_box, rect)
	var font := get_theme_font(&"font", &"Label")
	var font_size := get_theme_font_size(&"font_size", &"Label")
	draw_string(font, Vector2(4.0, font_size), "Score", HORIZONTAL_ALIGNMENT_LEFT, -1.0, font_size)
```

## 4. Swap the Whole Look at Runtime

Assign the new `Theme` to the **root** control of the screen. Every
descendant without its own `theme` picks it up, and each one receives
`NOTIFICATION_THEME_CHANGED`. Do not walk the tree to set themes node by node.

Two things block the cascade:

- A descendant with its own `theme` keeps that theme.
- A local `add_theme_*_override()` beats any theme. Remove stale overrides
  with `remove_theme_*_override()` when you swap.

```gdscript
extends Control

## Switches between named themes, for example "dark" and "high_contrast".
@export var themes: Dictionary[StringName, Theme] = {}


func apply_theme(id: StringName) -> void:
	if not themes.has(id):
		push_error("ThemeSwitcher.apply_theme: unknown theme '%s'" % id)
		return
	theme = themes[id]
```

The project-wide default comes from **Project Settings → GUI → Theme →
Custom**. Code can read it with `ThemeDB.get_project_theme()` (it is `null`
when none is set) and the engine fallback with
`ThemeDB.get_default_theme()`.

A theme can hold icons as well as colors. A seasonal or high-contrast theme
can swap icon textures for every button that reads them from the theme.

## 5. Rules That Prevent Common Bugs

| Rule | Why |
|---|---|
| Use `add_theme_color_override()` and its siblings, not `set("font_color", ...)` | Theme items are not plain properties; `set()` on the bare name does nothing. The inspector path is `theme_override_colors/font_color`. |
| Never give a focusable control an empty `focus` StyleBox without another cue | Keyboard and gamepad users lose track of focus. Keep a visible focus style, or show focus another way. |
| `expand_margin_*` grows only the drawn area | It does not grow the clickable rect. To make a hit area bigger, change the control's size or `custom_minimum_size`. `content_margin_*` adds inner padding. |
| Do not change theme resources in `_process()` or `_draw()` | Each change triggers theme propagation and relayout. |
| High-contrast themes need strong focus outlines | Use a thicker border on the `focus` StyleBox and pure foreground/background colors. |
