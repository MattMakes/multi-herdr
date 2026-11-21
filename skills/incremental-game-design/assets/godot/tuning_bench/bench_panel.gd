extends CanvasLayer
## The panel of the tuning bench. The script builds each control; the panel has no scene file.
##
## Tabs: Meters, Controls, Timeline, Actions, Tools. The panel is a view of the autoload `TuningBench`.
## It keeps no value of its own: each change goes to the bench, and the bench sends a signal back.

const Knobs := preload("res://addons/tuning_bench/bench_knobs.gd")
const Timeline := preload("res://addons/tuning_bench/bench_timeline.gd")

const WIDTH := 580.0
const FONT_SIZE := 13
const SMALL := 12
const NAME_WIDTH := 240.0
const VALUE_WIDTH := 74.0
const GROUP_ORDER: Array[String] = ["Whole game", "Player"]
const STATUS_COLOR: Dictionary = {"pass": Color(0.45, 0.9, 0.5), "pass (in range)": Color(0.95, 0.85, 0.35),
	"fail": Color(1.0, 0.45, 0.4), "n/a": Color(0.65, 0.68, 0.74)}
const IMPACT_COLOR: Dictionary = {"low": Color(0.45, 0.65, 0.95), "medium": Color(0.45, 0.85, 0.5), "high": Color(1.0, 0.62, 0.2)}
const DIM := Color(0.74, 0.77, 0.83)
## The background is opaque: the game below must not show through the text of the panel.
const BACK := Color(0.11, 0.12, 0.15, 1.0)
const SUFFIXES: Array[String] = ["", "k", "M", "B", "T", "Qa", "Qi", "Sx"]
const METER_UNITS: Dictionary = {"M1": " h", "M2": " min", "M3": " min", "M4": "%", "M5": "%", "M6": " x", "M7": "", "M8": "%"}

var _bench: Node = null
var _status: Label = null
var _tabs: TabContainer = null
var _meters_box: VBoxContainer = null
var _targets_grid: GridContainer = null
var _controls_box: VBoxContainer = null
var _search: LineEdit = null
var _rows: Dictionary = {}
var _groups: Dictionary = {}
var _timeline: Control = null
var _actions: Tree = null
var _output: RichTextLabel = null
var _setting_name: LineEdit = null
var _setting_list: OptionButton = null
var _fit_list: OptionButton = null
var _confirm: ConfirmationDialog = null
var _task_buttons: Array[Button] = []
# True while a row of the Controls tab calls `set_k`.
var _from_row: bool = false


## The bench calls this before it adds the panel to the tree.
func setup(bench: Node) -> void:
	_bench = bench
	layer = 100


func _ready() -> void:
	var theme: Theme = Theme.new()
	theme.default_font_size = FONT_SIZE
	var root: PanelContainer = PanelContainer.new()
	root.theme = theme
	root.anchor_left = 1.0
	root.anchor_right = 1.0
	root.anchor_bottom = 1.0
	root.offset_left = -WIDTH
	var style: StyleBoxFlat = StyleBoxFlat.new()
	style.bg_color = BACK
	style.set_content_margin_all(8.0)
	root.add_theme_stylebox_override("panel", style)
	add_child(root)

	var page: VBoxContainer = VBoxContainer.new()
	root.add_child(page)
	var head: HBoxContainer = HBoxContainer.new()
	page.add_child(head)
	var title: Label = Label.new()
	title.text = "Tuning bench: %s" % str((_bench.get("ctx")["bench"] as Dictionary).get("game", ""))
	title.add_theme_font_size_override("font_size", 16)
	title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	title.clip_text = true
	head.add_child(title)
	head.add_child(_button("Run again", func() -> void: _bench.call("request_run")))
	head.add_child(_button("Reset to base", func() -> void: _bench.call("reset_to_base")))
	head.add_child(_button("Close", func() -> void: visible = false))
	var review: String = str(_bench.call("review_line"))
	var texts: String = str(_bench.call("texts_line"))
	if review != "" or texts != "":
		# The ledger has numbers that nobody looked at, or texts of the view that hold a copy of a number.
		# A balance number can hide in them.
		var parts: PackedStringArray = PackedStringArray()
		if review != "":
			parts.append(review)
		if texts != "":
			parts.append(texts)
		var warn: Label = _small("Ledger: %s. Look at them with pw_bench.py." % "; ".join(parts), STATUS_COLOR["pass (in range)"])
		warn.name = "ReviewLine"
		page.add_child(warn)
	_status = Label.new()
	_status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_status.custom_minimum_size = Vector2(0.0, 36.0)
	_status.add_theme_color_override("font_color", DIM)
	page.add_child(_status)

	_tabs = TabContainer.new()
	_tabs.size_flags_vertical = Control.SIZE_EXPAND_FILL
	page.add_child(_tabs)
	_build_meters()
	_build_controls()
	_build_timeline()
	_build_actions()
	_build_tools()

	_bench.connect("result_changed", _show_result)
	_bench.connect("status_changed", _show_status)
	_bench.connect("k_changed", _fill_controls)
	_bench.connect("k_set", _on_k_set)
	_bench.connect("task_done", _on_task)
	_show_status()
	_show_result()


## Show a tab: 0 Meters, 1 Controls, 2 Timeline, 3 Actions, 4 Tools.
func show_tab(index: int) -> void:
	_tabs.current_tab = index


func _button(text: String, action: Callable) -> Button:
	var button: Button = Button.new()
	button.text = text
	button.pressed.connect(action)
	return button


func _small(text: String, color: Color = DIM) -> Label:
	var label: Label = Label.new()
	label.text = text
	label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	label.add_theme_font_size_override("font_size", SMALL)
	label.add_theme_color_override("font_color", color)
	return label


func _scroll(tab_name: String) -> VBoxContainer:
	var scroll: ScrollContainer = ScrollContainer.new()
	scroll.name = tab_name
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	_tabs.add_child(scroll)
	var box: VBoxContainer = VBoxContainer.new()
	box.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	scroll.add_child(box)
	return box


func _show_status() -> void:
	var text: String = str(_bench.get("status"))
	var note: String = str(_bench.get("live_note"))
	if note != "":
		text += "\nLive game: %s." % note
	_status.text = text


# ---------------------------------------------------------------------------------------------- Meters

func _build_meters() -> void:
	var box: VBoxContainer = _scroll("Meters")
	_meters_box = VBoxContainer.new()
	_meters_box.add_theme_constant_override("separation", 2)
	box.add_child(_meters_box)
	box.add_child(HSeparator.new())
	var head: Label = Label.new()
	head.text = "Targets (most targets are made-up rules: change them here)"
	box.add_child(head)
	_targets_grid = GridContainer.new()
	_targets_grid.columns = 4
	box.add_child(_targets_grid)
	_fill_targets()


func _fill_targets() -> void:
	for child: Node in _targets_grid.get_children():
		child.queue_free()
	var targets: Dictionary = _bench.call("targets")
	for key: Variant in targets:
		if not targets[key] is Dictionary:
			continue
		for sub: Variant in targets[key]:
			var value: Variant = targets[key][sub]
			if not (value is int or value is float):
				continue
			var label: Label = _small("%s: %s" % [str(key).replace("_", " "), str(sub).replace("_", " ")])
			label.autowrap_mode = TextServer.AUTOWRAP_OFF
			label.clip_text = true
			label.custom_minimum_size = Vector2(170.0, 0.0)
			_targets_grid.add_child(label)
			var spin: SpinBox = SpinBox.new()
			spin.min_value = 0.0
			spin.max_value = 100000.0
			spin.step = 0.01
			spin.allow_greater = true
			spin.custom_minimum_size = Vector2(92.0, 0.0)
			spin.set_value_no_signal(float(value))
			spin.value_changed.connect(func(new_value: float) -> void: _bench.call("set_target", str(key), str(sub), new_value))
			_targets_grid.add_child(spin)


func _show_result() -> void:
	var result: Dictionary = _bench.get("result")
	for child: Node in _meters_box.get_children():
		child.queue_free()
	if result.is_empty():
		_meters_box.add_child(_small("No result yet."))
		return
	var names: Dictionary = result.get("phase_names", {})
	for meter: Dictionary in result["meters"]:
		var head: HBoxContainer = HBoxContainer.new()
		_meters_box.add_child(head)
		var title: Label = Label.new()
		title.text = "%s  %s" % [meter["id"], meter["name"]]
		title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		head.add_child(title)
		var status: Label = Label.new()
		status.text = str(meter["status"]).to_upper()
		status.add_theme_color_override("font_color", STATUS_COLOR.get(meter["status"], DIM))
		head.add_child(status)
		var value: Label = Label.new()
		value.text = str(meter["text"])
		value.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		value.add_theme_color_override("font_color", STATUS_COLOR.get(meter["status"], DIM))
		_meters_box.add_child(value)
		for extra: Variant in meter.get("lines", []):
			_meters_box.add_child(_small(str(extra), Color.WHITE))
		_meters_box.add_child(_small("Target: %s" % meter["target"]))
		var parts: PackedStringArray = PackedStringArray()
		for phase_id: Variant in phase_order(_bench.get("ctx")["bench"], meter["phases"]):
			parts.append("%s %s" % [names.get(phase_id, phase_id), _phase_value(str(meter["id"]), float(meter["phases"][phase_id]))])
		var line: String = ""
		if not parts.is_empty():
			line = "Phases: " + " | ".join(parts)
		var moves: PackedStringArray = PackedStringArray()
		for id: Variant in meter["hint"]:
			var control: Dictionary = _bench.call("control", str(id))
			if not control.is_empty():
				moves.append(str(control["name"]))
		if not moves.is_empty():
			line += ("\n" if line != "" else "") + "Move: " + ", ".join(moves)
		if line != "":
			_meters_box.add_child(_small(line))
		_meters_box.add_child(HSeparator.new())
	_timeline.call("show_result", result, _bench.call("targets"))
	_fill_actions(result)


## Return the phase ids of a meter in the order of `phases` of the bench file. A result that came from a file
## has its keys in the order of the alphabet, which is not the order of the game.
static func phase_order(bench: Dictionary, values: Dictionary) -> Array:
	var out: Array = []
	for phase: Variant in bench.get("phases", []):
		if phase is Dictionary and values.has((phase as Dictionary).get("id", "")):
			out.append((phase as Dictionary)["id"])
	for id: Variant in values:
		if not out.has(id):
			out.append(id)
	return out


static func _phase_value(meter_id: String, value: float) -> String:
	if ["M4", "M5", "M8"].has(meter_id):
		return "%d%%" % roundi(value * 100.0)
	if meter_id == "M7":
		return "%d" % int(value)
	return ("%.2f" % value).rstrip("0").rstrip(".") + str(METER_UNITS.get(meter_id, ""))


# -------------------------------------------------------------------------------------------- Controls

func _build_controls() -> void:
	var page: VBoxContainer = VBoxContainer.new()
	page.name = "Controls"
	_tabs.add_child(page)
	_search = LineEdit.new()
	_search.placeholder_text = "Search a control or a number"
	_search.clear_button_enabled = true
	_search.text_changed.connect(_filter)
	page.add_child(_search)
	var scroll: ScrollContainer = ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	page.add_child(scroll)
	_controls_box = VBoxContainer.new()
	_controls_box.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	scroll.add_child(_controls_box)
	_fill_controls()


func _group_rank(group: String) -> int:
	if group == "Whole game":
		return 0
	if group.begins_with("Phase:"):
		return 1
	if group == "Player":
		return 2
	if group == Knobs.ALL_NUMBERS:
		return 4
	return 3


## Build each row of the Controls tab again from the bench.
func _fill_controls() -> void:
	for child: Node in _controls_box.get_children():
		child.queue_free()
	_rows = {}
	_groups = {}
	var ctx: Dictionary = _bench.get("ctx")
	var k: Dictionary = _bench.get("k")
	var order: Array = []
	var by_group: Dictionary = {}
	for control: Dictionary in ctx["controls"]:
		var group: String = control["group"]
		if not by_group.has(group):
			by_group[group] = []
			order.append(group)
		(by_group[group] as Array).append(control)
	var indexed: Dictionary = {}
	for index: int in range(order.size()):
		indexed[order[index]] = index
	order.sort_custom(func(a: String, b: String) -> bool:
		if _group_rank(a) != _group_rank(b):
			return _group_rank(a) < _group_rank(b)
		return int(indexed[a]) < int(indexed[b]))
	for group: String in order:
		var list: Array = by_group[group]
		var content: VBoxContainer = VBoxContainer.new()
		var head: Button = Button.new()
		head.alignment = HORIZONTAL_ALIGNMENT_LEFT
		head.toggle_mode = true
		var open: bool = group != Knobs.ALL_NUMBERS
		head.button_pressed = open
		head.text = "%s  %s (%d)" % ["[-]" if open else "[+]", group, list.size()]
		head.toggled.connect(func(pressed: bool) -> void:
			content.visible = pressed
			head.text = "%s  %s (%d)" % ["[-]" if pressed else "[+]", group, list.size()])
		content.visible = open
		_controls_box.add_child(head)
		_controls_box.add_child(content)
		_groups[group] = {"head": head, "content": content}
		if group == Knobs.ALL_NUMBERS:
			list = list.duplicate()
			list.sort_custom(func(a: Dictionary, b: Dictionary) -> bool: return str(a["element"]) < str(b["element"]))
			_add_not_tunable(content, ctx)
		var element: String = ""
		for control: Dictionary in list:
			if group == Knobs.ALL_NUMBERS and str(control["element"]) != element:
				element = str(control["element"])
				var sub: Label = _small(element, Color.WHITE)
				sub.set_meta("element", element)
				content.add_child(sub)
			_add_row(content, control, float(k.get(control["id"], control["base"])), group)
	_filter(_search.text)
	if _fit_list != null:
		_fill_fit_list()
	if _targets_grid != null:
		_fill_targets()


func _add_not_tunable(content: VBoxContainer, ctx: Dictionary) -> void:
	var lines: PackedStringArray = PackedStringArray()
	for id: Variant in ctx["numbers"]:
		var number: Dictionary = ctx["numbers"][id]
		if str(number.get("class", "")) == "balance" and not Knobs.is_tunable(number):
			lines.append("%s = %s (%s)" % [number.get("label", id), number.get("value", "?"), number.get("reason", "no reason")])
	if lines.is_empty():
		return
	content.add_child(_small("%d balance numbers are not tunable:\n%s" % [lines.size(), "\n".join(lines)], STATUS_COLOR["pass (in range)"]))


func _add_row(content: VBoxContainer, control: Dictionary, value: float, group: String) -> void:
	var row: HBoxContainer = HBoxContainer.new()
	content.add_child(row)
	var id: String = control["id"]
	var name_label: Label = Label.new()
	name_label.text = str(control["name"])
	name_label.tooltip_text = "%s\n%s, %d numbers" % [id, control["op"], (control["targets"] as Array).size()]
	name_label.mouse_filter = Control.MOUSE_FILTER_STOP
	name_label.clip_text = true
	name_label.custom_minimum_size = Vector2(NAME_WIDTH, 0.0)
	row.add_child(name_label)
	var slider: HSlider = HSlider.new()
	slider.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	slider.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	slider.min_value = float(control["min"])
	slider.max_value = float(control["max"])
	slider.step = 0.0
	if float(control["min"]) > 0.0 and float(control["max"]) / float(control["min"]) >= 4.0:
		slider.exp_edit = true
	slider.set_value_no_signal(value)
	row.add_child(slider)
	var shown: Label = Label.new()
	shown.custom_minimum_size = Vector2(VALUE_WIDTH, 0.0)
	shown.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	shown.text = _k_text(control, value)
	row.add_child(shown)
	var reset: Button = Button.new()
	reset.text = "Reset"
	reset.tooltip_text = "Set this control back to its base value"
	reset.disabled = value == float(control["base"])
	row.add_child(reset)
	slider.value_changed.connect(func(new_value: float) -> void:
		var rounded: float = _round_k(control, new_value)
		shown.text = _k_text(control, rounded)
		reset.disabled = rounded == float(control["base"])
		_from_row = true
		_bench.call("set_k", id, rounded)
		_from_row = false)
	reset.pressed.connect(func() -> void:
		slider.set_value_no_signal(float(control["base"]))
		shown.text = _k_text(control, float(control["base"]))
		reset.disabled = true
		_from_row = true
		_bench.call("set_k", id, float(control["base"]))
		_from_row = false)
	_rows[id] = {"row": row, "group": group, "element": str(control["element"]), "slider": slider, "shown": shown,
		"reset": reset, "control": control,
		"search": ("%s %s %s" % [control["name"], id, control["element"]]).to_lower()}


## A script (or a tool of the panel) called `set_k` of the bench. Move the slider of that control.
## A change that comes from the slider itself needs no move.
func _on_k_set(id: String, value: float) -> void:
	if _from_row or not _rows.has(id):
		return
	var entry: Dictionary = _rows[id]
	var control: Dictionary = entry["control"]
	(entry["slider"] as HSlider).set_value_no_signal(value)
	(entry["shown"] as Label).text = _k_text(control, value)
	(entry["reset"] as Button).disabled = value == float(control["base"])


static func _round_k(control: Dictionary, value: float) -> float:
	if control["op"] == "set":
		if absf(value) >= 1000.0:
			var digits: float = pow(10.0, floor(log(absf(value)) / log(10.0)) - 2.0)
			return roundf(value / digits) * digits
		return snappedf(value, 0.001)
	return snappedf(value, 0.01)


static func _k_text(control: Dictionary, value: float) -> String:
	match str(control["op"]):
		"set":
			return short(value)
		"add":
			return "%+.2f" % value
	return "x %.2f" % value


## Return a short text for a number.
static func short(value: float) -> String:
	var at: int = 0
	var shown: float = value
	while absf(shown) >= 1000.0 and at < SUFFIXES.size() - 1:
		shown /= 1000.0
		at += 1
	var text: String = "%.3f" % shown if at == 0 else "%.2f" % shown
	if text.contains("."):
		text = text.rstrip("0").rstrip(".")
	return text + SUFFIXES[at]


func _filter(text: String) -> void:
	var needle: String = text.strip_edges().to_lower()
	var shown_groups: Dictionary = {}
	var shown_elements: Dictionary = {}
	for id: Variant in _rows:
		var entry: Dictionary = _rows[id]
		var show: bool = needle == "" or str(entry["search"]).contains(needle)
		(entry["row"] as Control).visible = show
		if show:
			shown_groups[entry["group"]] = true
			shown_elements[entry["element"]] = true
	for group: Variant in _groups:
		var parts: Dictionary = _groups[group]
		var head: Button = parts["head"]
		var content: VBoxContainer = parts["content"]
		if needle == "":
			head.visible = true
			content.visible = head.button_pressed
		else:
			head.visible = shown_groups.has(group)
			content.visible = shown_groups.has(group)
		for child: Node in content.get_children():
			if child.has_meta("element"):
				(child as Control).visible = needle == "" or shown_elements.has(child.get_meta("element"))


# -------------------------------------------------------------------------------- Timeline and actions

func _build_timeline() -> void:
	_timeline = Timeline.new()
	_timeline.name = "Timeline"
	_tabs.add_child(_timeline)


func _build_actions() -> void:
	_actions = Tree.new()
	_actions.name = "Actions"
	_actions.columns = 6
	_actions.column_titles_visible = true
	_actions.hide_root = true
	var titles: Array[String] = ["Action", "Kind", "First", "Count", "Wait", "Impact"]
	var widths: Array[int] = [120, 62, 62, 106, 68, 110]
	for column: int in range(6):
		_actions.set_column_title(column, titles[column])
		_actions.set_column_title_alignment(column, HORIZONTAL_ALIGNMENT_LEFT)
		_actions.set_column_custom_minimum_width(column, widths[column])
		_actions.set_column_expand(column, column == 0)
	_tabs.add_child(_actions)


static func _clock(seconds: Variant) -> String:
	if seconds == null:
		return "-"
	var s: float = float(seconds)
	if s >= 6000.0:
		return "%.2f h" % (s / 3600.0)
	if s >= 60.0:
		return "%.1f min" % (s / 60.0)
	return "%d s" % int(s)


## Return the text of the column "Count": all purchases, and the part that the game made by itself.
static func count_text(action: Dictionary) -> String:
	var by_game: int = int(action.get("auto_count", 0))
	if by_game <= 0:
		return str(int(action["count"]))
	return "%d (%d auto)" % [int(action["count"]), by_game]


func _fill_actions(result: Dictionary) -> void:
	_actions.clear()
	var root: TreeItem = _actions.create_item()
	for action: Dictionary in result.get("actions", []):
		var item: TreeItem = _actions.create_item(root)
		item.set_text(0, str(action.get("label", action["id"])))
		item.set_tooltip_text(0, str(action["id"]))
		item.set_text(1, str(action["kind"]))
		item.set_text(2, _clock(action["first_s"]))
		item.set_text(3, count_text(action))
		if int(action.get("auto_count", 0)) > 0:
			item.set_tooltip_text(3, "%d purchases; the game made %d of them by itself (auto-buy)" % [int(action["count"]), int(action["auto_count"])])
		item.set_text(4, _clock(action["median_wait_s"]))
		item.set_tooltip_text(4, "median time between 2 purchases of this action")
		var impact: String = str(action["impact"])
		if action["median_gain"] != null and IMPACT_COLOR.has(impact):
			item.set_text(5, "%s (+%d%%)" % [impact, roundi(float(action["median_gain"]) * 100.0)])
			item.set_custom_color(5, IMPACT_COLOR[impact])
		else:
			item.set_text(5, impact)
			if impact == "not used":
				item.set_custom_color(5, STATUS_COLOR["fail"])
			elif impact == "choice":
				item.set_tooltip_text(5, "a free choice of the player (\"bot\": false): the bot does not use it")


# ----------------------------------------------------------------------------------------------- Tools

func _build_tools() -> void:
	var page: VBoxContainer = VBoxContainer.new()
	page.name = "Tools"
	_tabs.add_child(page)

	var save_row: HBoxContainer = HBoxContainer.new()
	page.add_child(save_row)
	_setting_name = LineEdit.new()
	_setting_name.placeholder_text = "Name of the setting"
	_setting_name.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	save_row.add_child(_setting_name)
	save_row.add_child(_button("Save setting", func() -> void:
		_bench.call("save_setting", _setting_name.text)
		_fill_setting_list()))
	_setting_list = OptionButton.new()
	_setting_list.custom_minimum_size = Vector2(130.0, 0.0)
	save_row.add_child(_setting_list)
	save_row.add_child(_button("Load setting", func() -> void:
		if _setting_list.selected >= 0:
			_bench.call("load_setting", _setting_list.get_item_text(_setting_list.selected))))
	_fill_setting_list()

	var compare_row: HBoxContainer = HBoxContainer.new()
	page.add_child(compare_row)
	compare_row.add_child(_button("Store as A", func() -> void: _store("A")))
	compare_row.add_child(_button("Store as B", func() -> void: _store("B")))
	compare_row.add_child(_button("Compare A against B", _show_compare))
	compare_row.add_child(_button("Use A", func() -> void: _use_slot("A")))
	compare_row.add_child(_button("Use B", func() -> void: _use_slot("B")))

	var task_row: HBoxContainer = HBoxContainer.new()
	page.add_child(task_row)
	_task_button(task_row, "Rank controls", func() -> void: _bench.call("start_task", "rank"))
	_fit_list = OptionButton.new()
	_fit_list.custom_minimum_size = Vector2(150.0, 0.0)
	_fit_list.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_fit_list.clip_text = true
	task_row.add_child(_fit_list)
	_fill_fit_list()
	_task_button(task_row, "Fit length", func() -> void:
		if _fit_list.selected >= 0:
			_bench.call("start_task", "fit", [str(_fit_list.get_item_metadata(_fit_list.selected))]))

	var check_row: HBoxContainer = HBoxContainer.new()
	page.add_child(check_row)
	_task_button(check_row, "Self-check", func() -> void: _bench.call("start_task", "selfcheck"))
	var bench: Dictionary = _bench.get("ctx")["bench"]
	if str(bench.get("core", "")) != "" and str(bench.get("adapter", "")) == "":
		_task_button(check_row, "Replay check", func() -> void: _bench.call("start_task", "replay"))
	check_row.add_child(_button("Write to data files", func() -> void: _confirm.popup_centered()))

	_confirm = ConfirmationDialog.new()
	_confirm.title = "Write to data files"
	_confirm.dialog_text = "Write each changed value into the data files of the game?\nEach file gets a backup copy (.bak) first."
	_confirm.confirmed.connect(func() -> void:
		var files: PackedStringArray = _bench.call("write_to_data_files")
		if not files.is_empty():
			_print("[b]Wrote the new values[/b]\n" + "\n".join(files) + "\nEach old file is beside it with the ending .bak."))
	add_child(_confirm)

	_output = RichTextLabel.new()
	_output.bbcode_enabled = true
	_output.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_output.add_theme_font_size_override("normal_font_size", SMALL)
	_output.add_theme_font_size_override("bold_font_size", SMALL)
	page.add_child(_output)
	_print("Save a setting, compare 2 settings, or start a tool. The result shows here.\n"
		+ "Rank controls: move each control by 25 percent and list the change of each meter.\n"
		+ "Fit length: find the value of 1 control that gives the target length.\n"
		+ "Self-check: prove that each number changes the simulation.")


func _task_button(row: HBoxContainer, text: String, action: Callable) -> void:
	var button: Button = _button(text, func() -> void:
		action.call()
		_lock_tasks())
	row.add_child(button)
	_task_buttons.append(button)


func _lock_tasks() -> void:
	var busy: bool = _bench.call("task_running")
	for button: Button in _task_buttons:
		button.disabled = busy


func _print(text: String) -> void:
	_output.text = text


func _fill_setting_list() -> void:
	_setting_list.clear()
	for setting_name: String in _bench.call("setting_names"):
		_setting_list.add_item(setting_name)


func _fill_fit_list() -> void:
	_fit_list.clear()
	for control: Dictionary in _bench.get("ctx")["controls"]:
		if control["generated"] or control["kind"] != "numbers" or (control["targets"] as Array).is_empty():
			continue
		_fit_list.add_item(str(control["name"]))
		_fit_list.set_item_metadata(_fit_list.item_count - 1, control["id"])


func _store(slot: String) -> void:
	_bench.call("store_slot", slot)
	_print("Stored the values and the result of now as %s." % slot)
	if (_bench.get("slots") as Dictionary).has("A") and (_bench.get("slots") as Dictionary).has("B"):
		_show_compare()


func _use_slot(slot: String) -> void:
	var slots: Dictionary = _bench.get("slots")
	if slots.has(slot):
		_bench.call("use_k", slots[slot]["k"], slots[slot]["targets"])


func _color_tag(status: String) -> String:
	return (STATUS_COLOR.get(status, DIM) as Color).to_html(false)


func _show_compare() -> void:
	var slots: Dictionary = _bench.get("slots")
	if not slots.has("A") or not slots.has("B"):
		_print("Store a setting as A and a setting as B first.")
		return
	var a: Dictionary = slots["A"]["result"]
	var b: Dictionary = slots["B"]["result"]
	if a.is_empty() or b.is_empty():
		_print("A or B has no result. Wait for the run, then store again.")
		return
	var text: String = "[b]A against B[/b]\n[table=3][cell][b]Meter[/b][/cell][cell][b]A[/b][/cell][cell][b]B[/b][/cell]"
	for index: int in range((a["meters"] as Array).size()):
		var meter_a: Dictionary = a["meters"][index]
		var meter_b: Dictionary = b["meters"][index]
		text += "[cell]%s %s   [/cell]" % [meter_a["id"], meter_a["name"]]
		text += "[cell][color=#%s]%s[/color]\n%s   [/cell]" % [_color_tag(meter_a["status"]), meter_a["status"], meter_a["text"]]
		text += "[cell][color=#%s]%s[/color]\n%s[/cell]" % [_color_tag(meter_b["status"]), meter_b["status"], meter_b["text"]]
	text += "[/table]\n\n[b]Controls that differ[/b]\n"
	var ctx: Dictionary = _bench.get("ctx")
	var changed_a: Dictionary = Knobs.changed_k(ctx, slots["A"]["k"])
	var changed_b: Dictionary = Knobs.changed_k(ctx, slots["B"]["k"])
	var ids: Dictionary = {}
	for id: Variant in changed_a:
		ids[id] = true
	for id: Variant in changed_b:
		ids[id] = true
	var lines: int = 0
	for id: Variant in ids:
		var control: Dictionary = _bench.call("control", str(id))
		var value_a: float = float(slots["A"]["k"].get(id, control["base"]))
		var value_b: float = float(slots["B"]["k"].get(id, control["base"]))
		if value_a != value_b:
			text += "%s: A %s, B %s\n" % [control["name"], _k_text(control, value_a), _k_text(control, value_b)]
			lines += 1
	if lines == 0:
		text += "none\n"
	_print(text)


func _on_task(kind: String, payload: Dictionary) -> void:
	_lock_tasks()
	if str(payload.get("format", "")) == "tuning-bench-error":
		_print("[b]Error[/b]\n%s" % payload.get("error", ""))
		return
	match kind:
		"rank":
			var ids: Array = (payload["base"] as Dictionary).keys()
			ids.sort()
			var text: String = "[b]Rank controls[/b]: the change of each meter when the control moves by 25 percent.\n"
			text += "[table=%d][cell][b]Control[/b]   [/cell]" % (ids.size() + 1)
			for id: Variant in ids:
				text += "[cell][b]%s[/b]  [/cell]" % id
			for row: Dictionary in payload["controls"]:
				text += "[cell]%s   [/cell]" % row["name"]
				for id: Variant in ids:
					var change: float = float(row["change"][id])
					var cell: String = "0" if absf(change) < 0.005 else "%+d%%" % roundi(change * 100.0)
					text += "[cell][color=#%s]%s[/color]  [/cell]" % [_color_tag(str(row["status"][id])), cell]
			_print(text + "[/table]\nThe colour is the status of the meter after the move.")
		"fit":
			var control: Dictionary = _bench.call("control", str(payload["control"]))
			_print("[b]Fit length[/b]\n%s = %s gives %.2f h (target %.2f h).\n%s" % [control.get("name", payload["control"]),
				_k_text(control, float(payload["k"])), float(payload["length_h"]), float(payload["target_h"]),
				"The control has this value now." if payload["reached"] else "This control does not reach the target. It has the nearest value now."])
		"selfcheck":
			var text: String = "[b]Self-check: %s[/b]\n" % ("ok" if payload["ok"] else "FAIL")
			for check: Dictionary in payload["checks"]:
				text += "[color=#%s]%s: %s[/color]\n" % [_color_tag("pass" if check["ok"] else "fail"), check["name"], "ok" if check["ok"] else "FAIL"]
				for line: Variant in check["detail"]:
					text += "    %s\n" % line
			for item: Dictionary in payload["dead"]:
				if item.has("dead_ok"):
					text += "No effect, accepted: %s (%s)\n" % [item["label"], item["dead_ok"]]
			for line: Variant in payload.get("stale", []):
				text += "Old value in the ledger: %s\n" % line
			if payload.get("partial", false):
				text += "Partial: the dead-number check tried %d numbers.\n" % (payload.get("checked", []) as Array).size()
			var numbers: Dictionary = payload["numbers"]
			text += "%d numbers in the ledger; %d balance numbers; %d tunable; %d not tunable." % [int(numbers["total"]),
				int(numbers["balance"]), int(numbers["tunable"]), (numbers["not_tunable"] as Array).size()]
			_print(text)
		"replay":
			_print("[b]Replay check: %s[/b]\nThe bench did the %d actions of the active player again from the record.\n%s" % [
				"ok" if payload["ok"] else "FAIL", int(payload["actions"]),
				"The 2 final states are equal." if payload["ok"] else "The 2 final states differ: the core is not deterministic."])
