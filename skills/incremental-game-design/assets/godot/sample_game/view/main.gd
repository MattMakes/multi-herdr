extends Control

const Core := preload("res://core/game_core.gd")
const SessionScript := preload("res://session/game_session.gd")

const GROUPS: Array[String] = ["Lamps", "Upgrades", "Road"]
const FLASH := Color(1.0, 0.85, 0.35)
const SUFFIXES: Array[String] = ["", "k", "M", "B", "T", "Qa", "Qi"]

var session: Node = null

var _session: Node = null
var _light: Label = null
var _rate: Label = null
var _road: Label = null
var _news: Label = null
var _lists: Dictionary = {}
var _buttons: Dictionary = {}


func _ready() -> void:
	if session == null:
		session = get_node_or_null("/root/Session")
	if session == null:
		session = SessionScript.new()
		add_child(session)
	_session = session
	var back: ColorRect = ColorRect.new()
	back.color = Color(0.09, 0.1, 0.14)
	back.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(back)

	var page: VBoxContainer = VBoxContainer.new()
	page.set_anchors_preset(Control.PRESET_FULL_RECT)
	page.offset_left = 16.0
	page.offset_top = 12.0
	page.offset_right = -16.0
	page.offset_bottom = -12.0
	page.add_theme_constant_override("separation", 8)
	add_child(page)

	var title: Label = Label.new()
	title.text = "Lamp Road"
	title.add_theme_font_size_override("font_size", 26)
	page.add_child(title)
	_light = Label.new()
	_light.add_theme_font_size_override("font_size", 22)
	_light.pivot_offset = Vector2(0.0, 14.0)
	page.add_child(_light)
	_rate = Label.new()
	page.add_child(_rate)
	_road = Label.new()
	page.add_child(_road)

	var click: Button = Button.new()
	click.text = "Strike a match"
	click.custom_minimum_size = Vector2(220.0, 44.0)
	click.size_flags_horizontal = Control.SIZE_SHRINK_BEGIN
	click.pressed.connect(func() -> void: _session.call("click"))
	page.add_child(click)

	_news = Label.new()
	_news.text = "Make light. Light each mile of the road."
	_news.modulate = Color(1.0, 1.0, 1.0, 0.8)
	page.add_child(_news)

	var columns: HBoxContainer = HBoxContainer.new()
	columns.size_flags_vertical = Control.SIZE_EXPAND_FILL
	columns.add_theme_constant_override("separation", 12)
	page.add_child(columns)
	for group: String in GROUPS:
		var column: VBoxContainer = VBoxContainer.new()
		column.custom_minimum_size = Vector2(230.0, 0.0)
		columns.add_child(column)
		var head: Label = Label.new()
		head.text = group
		head.add_theme_font_size_override("font_size", 18)
		column.add_child(head)
		var scroll: ScrollContainer = ScrollContainer.new()
		scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
		scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
		column.add_child(scroll)
		var list: VBoxContainer = VBoxContainer.new()
		list.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		scroll.add_child(list)
		_lists[group] = list

	_session.connect("changed", _refresh)
	_session.connect("event", _on_event)
	_refresh()


func _refresh() -> void:
	var state: Dictionary = _session.get("state")
	var data: Dictionary = _session.get("data")
	_light.text = "%s light" % short(float(state["light"]))
	_rate.text = "%s light for each second   |   each click gives %s" % [short(Core.rate(state, data)), short(Core.click_value(state, data))]
	_road.text = "Mile %d of %d   |   %d embers   |   night %d" % [int(state["stage"]), (data[Core.STAGES_FILE]["stages"] as Array).size(), int(state["embers"]), int(state["resets"]) + 1]
	var live: Dictionary = {}
	for action: Dictionary in _session.call("actions"):
		var id: String = action["id"]
		live[id] = true
		var button: Button = _buttons.get(id, null)
		if button == null:
			button = Button.new()
			button.alignment = HORIZONTAL_ALIGNMENT_LEFT
			button.pressed.connect(func() -> void: _session.call("dispatch", id))
			(_lists[str(action.get("group", "Road"))] as VBoxContainer).add_child(button)
			_buttons[id] = button
		var price: String = "free" if float(action["cost"]) <= 0.0 else short(float(action["cost"]))
		if not action.get("bot", true):
			price = "press to change"
		var label: String = action["label"]
		if action.has("level"):
			label += " (level %d)" % int(action["level"])
		elif action.has("embers"):
			label += " (+%d embers)" % int(action["embers"])
		button.text = "%s\n%s" % [label, price]
		button.disabled = not action["affordable"]
	for id: String in _buttons.keys():
		if not live.has(id):
			(_buttons[id] as Button).queue_free()
			_buttons.erase(id)


func _on_event(e: Dictionary) -> void:
	match str(e["kind"]):
		"bought":
			_say(("The lamplighter bought %s." if e.get("auto", false) else "You bought %s.") % e["label"])
		"unlock":
			_say("New: %s." % e["label"])
		"advance":
			_say("You lit %s." % e["label"])
		"reward":
			_say("%s: %s light." % [e["label"], short(float(e["amount"]))])
		"reset":
			_say("A new night starts. You have %d more embers." % int(e["embers"]))
		"switch":
			_say("%s." % e["label"])
			return
		"refused":
			return
	_light.modulate = FLASH
	_light.scale = Vector2(1.08, 1.08)
	var tween: Tween = create_tween().set_parallel(true)
	tween.tween_property(_light, "modulate", Color.WHITE, 0.35)
	tween.tween_property(_light, "scale", Vector2.ONE, 0.35)


func _say(line: String) -> void:
	_news.text = line


static func short(value: float) -> String:
	var at: int = 0
	var shown: float = value
	while absf(shown) >= 1000.0 and at < SUFFIXES.size() - 1:
		shown /= 1000.0
		at += 1
	if at == 0 and absf(shown) < 100.0:
		return ("%.1f" % shown).trim_suffix(".0")
	if at == 0:
		return "%d" % int(shown)
	return "%.2f%s" % [shown, SUFFIXES[at]]
