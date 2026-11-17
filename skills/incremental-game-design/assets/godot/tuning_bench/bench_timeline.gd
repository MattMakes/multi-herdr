extends Control
## The timeline of the tuning bench: 1 game of the active player on 1 picture.
##
## The x axis is the time. The phases are bands. The income is a line on a log scale.
## Each purchase is a mark with the colour of its impact. A purchase that the game made by itself (auto-buy)
## is a mark at the top of the row "buys". Each new thing is a tick. The mouse over a band shows its phase.

const Meters := preload("res://addons/tuning_bench/bench_meters.gd")

const BANDS: Array[Color] = [Color(0.30, 0.42, 0.62, 0.30), Color(0.42, 0.32, 0.60, 0.30), Color(0.62, 0.44, 0.26, 0.30),
	Color(0.30, 0.55, 0.45, 0.30), Color(0.55, 0.30, 0.40, 0.30)]
const IMPACT: Dictionary = {"none": Color(0.55, 0.55, 0.6), "low": Color(0.45, 0.65, 0.95), "medium": Color(0.45, 0.85, 0.5),
	"high": Color(1.0, 0.62, 0.2)}
const LINE := Color(1.0, 0.92, 0.55)
const CONTENT := Color(1.0, 1.0, 1.0)
const ADVANCE := Color(1.0, 0.85, 0.3)
const RESET := Color(0.95, 0.4, 0.4)
const AUTO := Color(0.8, 0.55, 1.0)
const AUTO_TALL := 5.0
const ELLIPSIS := "…"
const LABEL_PAD := 4.0
const DIM := Color(0.75, 0.78, 0.84)
const GRID := Color(1.0, 1.0, 1.0, 0.10)
const LEFT := 52.0
const RIGHT := 12.0
const TOP := 26.0
const STRIP := 22.0
const FONT_SIZE := 12
const MAX_POINTS := 1500

var _result: Dictionary = {}
var _impact_target: Dictionary = {}
## The bands of the last draw: [x0, x1, text of the tooltip].
var _bands: Array = []


## Show a result of a run. `targets` gives the limits of the impact classes.
func show_result(result: Dictionary, targets: Dictionary) -> void:
	_result = result
	_impact_target = targets.get("impact", {}) if targets.get("impact", null) is Dictionary else {}
	queue_redraw()


func _draw() -> void:
	var font: Font = get_theme_default_font()
	if _result.is_empty() or not _result.has("timeline"):
		draw_string(font, Vector2(LEFT, TOP + 20.0), "No result yet.", HORIZONTAL_ALIGNMENT_LEFT, -1.0, FONT_SIZE, DIM)
		return
	var timeline: Dictionary = _result["timeline"]
	var length: float = maxf(float(timeline.get("length_s", 1.0)), 1.0)
	var names: Dictionary = _result.get("phase_names", {})
	var legend_h: float = 64.0
	var strips_h: float = STRIP * 3.0 + 18.0
	var plot: Rect2 = Rect2(LEFT, TOP, size.x - LEFT - RIGHT, maxf(size.y - TOP - strips_h - legend_h - 22.0, 60.0))
	var strips_y: float = plot.end.y + 22.0
	var full_bottom: float = strips_y + STRIP * 3.0

	# The phases as bands, over the plot and the 3 strips.
	_bands = []
	var band_at: int = 0
	for phase: Array in timeline.get("phases", []):
		var x0: float = plot.position.x + plot.size.x * float(phase[1]) / length
		var x1: float = plot.position.x + plot.size.x * float(phase[2]) / length
		draw_rect(Rect2(x0, plot.position.y, x1 - x0, full_bottom - plot.position.y), BANDS[band_at % BANDS.size()])
		var label: String = str(names.get(phase[0], phase[0]))
		_bands.append([x0, x1, "%s: %s to %s" % [label, _clock(float(phase[1])), _clock(float(phase[2]))]])
		# A name that is wider than its band is cut at a letter and ends with "…". The tooltip has the full name.
		var fitted: String = fit_label(font, label, x1 - x0 - LABEL_PAD * 2.0)
		if fitted != "":
			draw_string(font, Vector2(x0 + LABEL_PAD, plot.position.y - 8.0), fitted, HORIZONTAL_ALIGNMENT_LEFT, -1.0, FONT_SIZE, Color.WHITE)
		band_at += 1
	draw_rect(plot, Color(1.0, 1.0, 1.0, 0.25), false, 1.0)

	# The time axis.
	var hours: float = length / 3600.0
	var hour_step: float = 1.0
	if hours > 12.0:
		hour_step = 2.0
	elif hours < 2.0:
		hour_step = 0.25
	var mark: float = 0.0
	while mark <= hours + 1.0e-6:
		var x: float = plot.position.x + plot.size.x * mark / hours
		draw_line(Vector2(x, plot.position.y), Vector2(x, plot.end.y), GRID, 1.0)
		var text: String = ("%d h" % int(mark)) if hour_step >= 1.0 else ("%d min" % roundi(mark * 60.0))
		draw_string(font, Vector2(x - 14.0, plot.end.y + 15.0), text, HORIZONTAL_ALIGNMENT_LEFT, -1.0, FONT_SIZE, DIM)
		mark += hour_step

	# The income line on a log scale.
	var series: Array = timeline.get("rate", [])
	var low: float = INF
	var high: float = 0.0
	for point: Array in series:
		var value: float = point[1]
		if value > 0.0:
			low = minf(low, value)
			high = maxf(high, value)
	if high > 0.0:
		var log_low: float = floor(log(low) / log(10.0))
		var log_high: float = ceil(log(high) / log(10.0))
		if log_high <= log_low:
			log_high = log_low + 1.0
		var decade_step: int = maxi(1, int(ceil((log_high - log_low) / 6.0)))
		var decade: int = int(log_low)
		while decade <= int(log_high):
			var y: float = plot.end.y - plot.size.y * (float(decade) - log_low) / (log_high - log_low)
			draw_line(Vector2(plot.position.x, y), Vector2(plot.end.x, y), GRID, 1.0)
			draw_string(font, Vector2(4.0, y + 4.0), _power(decade), HORIZONTAL_ALIGNMENT_LEFT, LEFT - 8.0, FONT_SIZE, DIM)
			decade += decade_step
		var points: PackedVector2Array = PackedVector2Array()
		var stride: int = maxi(1, int(ceil(float(series.size()) / float(MAX_POINTS))))
		var last_y: float = plot.end.y
		for index: int in range(series.size()):
			var point: Array = series[index]
			var value: float = maxf(float(point[1]), low)
			var x: float = plot.position.x + plot.size.x * float(point[0]) / length
			var y: float = plot.end.y - plot.size.y * (log(value) / log(10.0) - log_low) / (log_high - log_low)
			# The income is constant between 2 points: draw a step.
			if index % stride == 0 or index == series.size() - 1:
				if not points.is_empty():
					points.append(Vector2(x, last_y))
				points.append(Vector2(x, y))
				last_y = y
		points.append(Vector2(plot.end.x, last_y))
		if points.size() >= 2:
			draw_polyline(points, LINE, 1.5, true)
	draw_string(font, Vector2(plot.position.x + 6.0, plot.position.y + 15.0), "income for each second (log scale)", HORIZONTAL_ALIGNMENT_LEFT, -1.0, FONT_SIZE, LINE)

	# Strip 1: the purchases. Strip 2: the new things. Strip 3: the walls and the resets.
	var rows: Array[String] = ["buys", "new", "walls"]
	for row: int in range(3):
		var y: float = strips_y + STRIP * float(row)
		draw_line(Vector2(plot.position.x, y), Vector2(plot.end.x, y), GRID, 1.0)
		draw_string(font, Vector2(4.0, y + 15.0), rows[row], HORIZONTAL_ALIGNMENT_LEFT, LEFT - 8.0, FONT_SIZE, DIM)
	for purchase: Array in timeline.get("purchases", []):
		var x: float = plot.position.x + plot.size.x * float(purchase[0]) / length
		var kind: String = Meters.impact_class(float(purchase[2]), _impact_target)
		var tall: float = 6.0
		if kind == "medium":
			tall = 11.0
		elif kind == "high":
			tall = 17.0
		draw_line(Vector2(x, strips_y + STRIP - 2.0), Vector2(x, strips_y + STRIP - 2.0 - tall), IMPACT[kind], 1.0)
	for purchase: Array in timeline.get("auto", []):
		var x: float = plot.position.x + plot.size.x * float(purchase[0]) / length
		draw_line(Vector2(x, strips_y + 1.0), Vector2(x, strips_y + 1.0 + AUTO_TALL), AUTO, 1.0)
	for item: Array in timeline.get("content", []):
		var x: float = plot.position.x + plot.size.x * float(item[0]) / length
		var y: float = strips_y + STRIP
		draw_colored_polygon(PackedVector2Array([Vector2(x - 4.0, y + 4.0), Vector2(x + 4.0, y + 4.0), Vector2(x, y + 16.0)]), CONTENT)
	for item: Array in timeline.get("advances", []):
		var x: float = plot.position.x + plot.size.x * float(item[0]) / length
		var y: float = strips_y + STRIP * 2.0 + 10.0
		draw_colored_polygon(PackedVector2Array([Vector2(x, y - 6.0), Vector2(x + 5.0, y), Vector2(x, y + 6.0), Vector2(x - 5.0, y)]), ADVANCE)
	for time: Variant in timeline.get("resets", []):
		var x: float = plot.position.x + plot.size.x * float(time) / length
		draw_dashed_line(Vector2(x, plot.position.y), Vector2(x, full_bottom), RESET, 1.0, 4.0)

	# The legend.
	var y0: float = full_bottom + 22.0
	var x0: float = 8.0
	x0 = _legend(font, x0, y0, IMPACT["low"], "low impact")
	x0 = _legend(font, x0, y0, IMPACT["medium"], "medium")
	x0 = _legend(font, x0, y0, IMPACT["high"], "high")
	x0 = _legend(font, x0, y0, IMPACT["none"], "no rate change")
	x0 = _legend(font, x0, y0, AUTO, "auto purchase")
	x0 = 8.0
	x0 = _legend(font, x0, y0 + 20.0, CONTENT, "new thing")
	x0 = _legend(font, x0, y0 + 20.0, ADVANCE, "wall passed")
	x0 = _legend(font, x0, y0 + 20.0, RESET, "reset")
	x0 = _legend(font, x0, y0 + 20.0, LINE, "income")
	var by_game: int = 0
	for purchase: Array in timeline.get("auto", []):
		by_game += int(purchase[2])
	draw_string(font, Vector2(8.0, y0 + 40.0), "%d purchases by hand, %d auto, %d new things, %d walls, %d resets in %.2f h" % [
		(timeline.get("purchases", []) as Array).size(), by_game, (timeline.get("content", []) as Array).size(),
		(timeline.get("advances", []) as Array).size(), (timeline.get("resets", []) as Array).size(), hours],
		HORIZONTAL_ALIGNMENT_LEFT, -1.0, FONT_SIZE, DIM)


## Return the label of a band that fits into `width`: the full text, a text that ends with "…", or "" when
## not even 1 letter fits.
static func fit_label(font: Font, text: String, width: float) -> String:
	if font.get_string_size(text, HORIZONTAL_ALIGNMENT_LEFT, -1.0, FONT_SIZE).x <= width:
		return text
	for count: int in range(text.length() - 1, 0, -1):
		var cut: String = text.substr(0, count).strip_edges() + ELLIPSIS
		if font.get_string_size(cut, HORIZONTAL_ALIGNMENT_LEFT, -1.0, FONT_SIZE).x <= width:
			return cut
	return ""


static func _clock(seconds: float) -> String:
	if seconds >= 3600.0:
		return "%.2f h" % (seconds / 3600.0)
	return "%.1f min" % (seconds / 60.0)


## The mouse is over a band: show the full name of the phase and its time.
func _get_tooltip(at_position: Vector2) -> String:
	for band: Array in _bands:
		if at_position.x >= float(band[0]) and at_position.x < float(band[1]):
			return str(band[2])
	return ""


func _legend(font: Font, x: float, y: float, color: Color, text: String) -> float:
	draw_rect(Rect2(x, y - 10.0, 10.0, 10.0), color)
	draw_string(font, Vector2(x + 14.0, y), text, HORIZONTAL_ALIGNMENT_LEFT, -1.0, FONT_SIZE, DIM)
	return x + 14.0 + font.get_string_size(text, HORIZONTAL_ALIGNMENT_LEFT, -1.0, FONT_SIZE).x + 14.0


static func _power(decade: int) -> String:
	var suffixes: Array[String] = ["", "k", "M", "B", "T", "Qa", "Qi", "Sx"]
	if decade < 0:
		return "%.*f" % [-decade, pow(10.0, float(decade))]
	var group: int = decade / 3
	var lead: int = int(pow(10.0, float(decade % 3)))
	if group < suffixes.size():
		return "%d%s" % [lead, suffixes[group]]
	return "1e%d" % decade
