extends Node
## Player settings: storage in user://settings.cfg, window sizing, audio, and motion.
## Every default value and every sizing number comes from res://data/settings_defaults.json.

signal changed(key: String)

const DEFAULTS_PATH: String = "res://data/settings_defaults.json"
const DEFAULT_STORAGE_PATH: String = "user://settings.cfg"
const SECTION: String = "settings"
const MASTER_BUS: int = 0
const KEYS: Array[String] = [
	"window_mode", "window_scale", "vsync", "master_volume",
	"sfx_enabled", "reduced_motion", "show_debug_controls",
]

## Tests set this to user://test_settings.cfg.
var storage_path: String = DEFAULT_STORAGE_PATH
## The "sizing" object of the defaults file. The static sizing functions take it.
var rules: Dictionary = {}

var _file_data: Dictionary = {}
var _defaults: Dictionary = {}
var _values: Dictionary = {}


func _init() -> void:
	_file_data = _read_defaults_file()
	rules = _file_data.get("sizing", {}) as Dictionary
	_defaults = _platform_defaults(_file_data, _current_environment())
	_values = _defaults.duplicate()


func _ready() -> void:
	load_settings()
	_apply_audio()
	_apply_motion()
	if _has_window():
		_apply_vsync()
		_apply_window()


# ---------------------------------------------------------------- sizing rule

## Scale for the canvas from the usable screen size. See data/settings_defaults.json "sizing".
static func auto_scale(usable_size: Vector2i, sizing: Dictionary) -> float:
	var base: Vector2 = Vector2(float(sizing["base_width"]), float(sizing["base_height"]))
	var fraction: float = float(sizing["screen_fraction"])
	var step: float = float(sizing["scale_step"])
	var min_scale: float = float(sizing["min_scale"])
	var fit: float = minf(float(usable_size.x) / base.x, float(usable_size.y) / base.y)
	if fit < min_scale:
		return fit
	var wanted: float = minf(float(usable_size.x) * fraction / base.x, float(usable_size.y) * fraction / base.y)
	var snapped_scale: float = floorf(wanted / step + 0.000001) * step
	return maxf(snapped_scale, min_scale)


static func window_size_for_scale(scale: float, sizing: Dictionary) -> Vector2i:
	return Vector2i(
		roundi(float(sizing["base_width"]) * scale),
		roundi(float(sizing["base_height"]) * scale)
	)


## The presets whose window fits inside the usable size.
static func scales_that_fit(usable_size: Vector2i, sizing: Dictionary) -> Array[float]:
	var result: Array[float] = []
	for preset: Variant in sizing["scale_presets"]:
		var scale: float = float(preset)
		var size: Vector2i = window_size_for_scale(scale, sizing)
		if size.x <= usable_size.x and size.y <= usable_size.y:
			result.append(scale)
	return result


static func is_steam_deck(env: Dictionary) -> bool:
	return str(env.get("SteamDeck", "")) == "1"


# ---------------------------------------------------------------- getters and setters

func get_window_mode() -> String:
	return _values["window_mode"] as String


func set_window_mode(value: String) -> void:
	_store("window_mode", value)


func get_window_scale() -> float:
	return _values["window_scale"] as float


## 0.0 means automatic. Any other value must be one of the scale presets.
func set_window_scale(value: float) -> void:
	_store("window_scale", value)


func get_vsync() -> bool:
	return _values["vsync"] as bool


func set_vsync(value: bool) -> void:
	_store("vsync", value)


func get_master_volume() -> float:
	return _values["master_volume"] as float


func set_master_volume(value: float) -> void:
	_store("master_volume", value)


func get_sfx_enabled() -> bool:
	return _values["sfx_enabled"] as bool


func set_sfx_enabled(value: bool) -> void:
	_store("sfx_enabled", value)


func get_reduced_motion() -> bool:
	return _values["reduced_motion"] as bool


func set_reduced_motion(value: bool) -> void:
	_store("reduced_motion", value)


func get_show_debug_controls() -> bool:
	return _values["show_debug_controls"] as bool


func set_show_debug_controls(value: bool) -> void:
	_store("show_debug_controls", value)


func settings_path() -> String:
	return storage_path


func scale_presets() -> Array[float]:
	var result: Array[float] = []
	for preset: Variant in rules["scale_presets"]:
		result.append(float(preset))
	return result


## The presets that fit the current screen. Without a window, all presets.
func available_scales() -> Array[float]:
	if not _has_window():
		return scale_presets()
	return scales_that_fit(_usable_rect().size, rules)


## The scale that the windowed mode uses now: the chosen preset, or the automatic scale.
func effective_scale() -> float:
	var automatic: float = auto_scale(_usable_rect().size, rules) if _has_window() else float(rules["min_scale"])
	var chosen: float = get_window_scale()
	if chosen <= 0.0:
		return automatic
	if _has_window() and not available_scales().has(chosen):
		return automatic
	return chosen


func reset_to_defaults() -> void:
	for key: String in KEYS:
		_store(key, _defaults[key])


## Reads storage_path. A missing or damaged file gives the defaults. A bad value gives its default.
func load_settings() -> void:
	_values = _defaults.duplicate()
	if not FileAccess.file_exists(storage_path):
		return
	var stored: Dictionary = _read_section(FileAccess.get_file_as_string(storage_path), SECTION)
	for key: String in KEYS:
		if stored.has(key):
			var value: Variant = _validated(key, stored[key])
			if value != null:
				_values[key] = value


func save_settings() -> bool:
	var config: ConfigFile = ConfigFile.new()
	for key: String in KEYS:
		config.set_value(SECTION, key, _values[key])
	return config.save(storage_path) == OK


# ---------------------------------------------------------------- internals

## Reads one section of ConfigFile text line by line. ConfigFile.load() prints an
## ERROR line on a damaged file, and str_to_var() returns null without a message.
## Each value that save_settings() writes is a scalar on one line.
static func _read_section(text: String, section: String) -> Dictionary:
	var result: Dictionary = {}
	var current: String = ""
	for raw_line: String in text.split("\n"):
		var line: String = raw_line.strip_edges()
		if line.is_empty() or line.begins_with(";"):
			continue
		if line.begins_with("["):
			current = line.trim_prefix("[").trim_suffix("]") if line.ends_with("]") else ""
			continue
		var equals: int = line.find("=")
		if current != section or equals <= 0:
			continue
		var value: Variant = str_to_var(line.substr(equals + 1).strip_edges())
		if value != null:
			result[line.substr(0, equals).strip_edges()] = value
	return result


func _store(key: String, raw: Variant) -> void:
	var value: Variant = _validated(key, raw)
	if value == null or value == _values[key]:
		return
	_values[key] = value
	_apply(key)
	save_settings()
	changed.emit(key)


## Returns the clean value, or null when the value has a wrong type or is out of range.
func _validated(key: String, raw: Variant) -> Variant:
	match key:
		"window_mode":
			if raw is String and (_file_data["window_modes"] as Array).has(raw):
				return raw
		"window_scale":
			if raw is float or raw is int:
				var scale: float = float(raw)
				if is_zero_approx(scale):
					return 0.0
				for preset: float in scale_presets():
					if is_equal_approx(scale, preset):
						return preset
		"master_volume":
			if raw is float or raw is int:
				var volume: float = float(raw)
				if volume >= float(_file_data["master_volume_min"]) and volume <= float(_file_data["master_volume_max"]):
					return volume
		"vsync", "sfx_enabled", "reduced_motion", "show_debug_controls":
			if raw is bool:
				return raw
	return null


func _apply(key: String) -> void:
	match key:
		"master_volume", "sfx_enabled":
			_apply_audio()
		"reduced_motion":
			_apply_motion()
		"vsync":
			if _has_window():
				_apply_vsync()
		"window_mode", "window_scale":
			if _has_window():
				_apply_window()


func _apply_audio() -> void:
	var volume: float = get_master_volume()
	if volume > 0.0:
		AudioServer.set_bus_volume_db(MASTER_BUS, linear_to_db(volume))
	AudioServer.set_bus_mute(MASTER_BUS, volume <= 0.0 or not get_sfx_enabled())


func _apply_motion() -> void:
	UITheme.reduced_motion = get_reduced_motion()


func _apply_vsync() -> void:
	var mode: DisplayServer.VSyncMode = DisplayServer.VSYNC_ENABLED if get_vsync() else DisplayServer.VSYNC_DISABLED
	DisplayServer.window_set_vsync_mode(mode)


func _apply_window() -> void:
	var window: Window = get_window()
	var usable: Rect2i = _usable_rect()
	var min_size: Vector2i = Vector2i(int(rules["min_window_width"]), int(rules["min_window_height"]))
	window.min_size = min_size.min(usable.size)
	if get_window_mode() == "fullscreen":
		window.mode = Window.MODE_FULLSCREEN
		return
	if window.mode != Window.MODE_WINDOWED:
		window.mode = Window.MODE_WINDOWED
	var size: Vector2i = window_size_for_scale(effective_scale(), rules).max(window.min_size)
	window.size = size
	window.position = usable.position + (usable.size - size) / 2


func _usable_rect() -> Rect2i:
	var screen: int = DisplayServer.window_get_current_screen()
	return DisplayServer.screen_get_usable_rect(screen)


func _has_window() -> bool:
	return DisplayServer.get_name() != "headless"


static func _read_defaults_file() -> Dictionary:
	var parsed: Variant = JSON.parse_string(FileAccess.get_file_as_string(DEFAULTS_PATH))
	if parsed is Dictionary:
		return parsed as Dictionary
	push_error("Settings: %s is missing or invalid" % DEFAULTS_PATH)
	return {}


static func _platform_defaults(file_data: Dictionary, env: Dictionary) -> Dictionary:
	var defaults: Dictionary = (file_data.get("defaults", {}) as Dictionary).duplicate()
	defaults["window_scale"] = float(defaults.get("window_scale", 0.0))
	defaults["master_volume"] = float(defaults.get("master_volume", 0.0))
	if is_steam_deck(env):
		defaults.merge(file_data.get("steam_deck_defaults", {}) as Dictionary, true)
	return defaults


static func _current_environment() -> Dictionary:
	return {"SteamDeck": OS.get_environment("SteamDeck")}
