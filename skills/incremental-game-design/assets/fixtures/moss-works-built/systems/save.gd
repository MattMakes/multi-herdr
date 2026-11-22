extends Node
## Atomic persistence, corrupt-save recovery, autosave, and capped offline progress.

const SAVE_PATH: String = "user://moss_save.json"
const SCHEMA_VERSION: int = 1

enum LoadStatus { OK = 0, MISSING = 1, CORRUPT_RESET = 2 }

signal offline_progress_applied(effective_sec: float, raw_sec: float, spores_gained: float, dew_gained: int)
signal save_corrupt_recovered()

var autosave_enabled: bool = true
var _autosave_timer: float = 0.0
var last_offline_report: Dictionary = {}
var _restoring_state: bool = false


func _ready() -> void:
	if not Economy.stage_cleared.is_connected(_on_stage_cleared):
		Economy.stage_cleared.connect(_on_stage_cleared)
	if not Economy.prestiged.is_connected(_on_prestiged):
		Economy.prestiged.connect(_on_prestiged)
	if not Garden.tonic_unlocked.is_connected(_on_tonic_unlocked):
		Garden.tonic_unlocked.connect(_on_tonic_unlocked)


func _process(delta: float) -> void:
	if not autosave_enabled or _restoring_state:
		return
	_autosave_timer += maxf(delta, 0.0)
	var interval: float = float(Economy.rules.get("autosave_interval_seconds", 30.0))
	if interval <= 0.0 or _autosave_timer < interval:
		return
	_autosave_timer = fmod(_autosave_timer, interval)
	save_game()


func _notification(what: int) -> void:
	if not autosave_enabled or _restoring_state:
		return
	if what == NOTIFICATION_WM_CLOSE_REQUEST \
		or what == NOTIFICATION_APPLICATION_PAUSED \
		or what == NOTIFICATION_APPLICATION_FOCUS_OUT:
		_save_autosave_event()


func save_game(path: String = SAVE_PATH, now_unix: float = -1.0) -> bool:
	var saved_at_unix: float = now_unix
	if saved_at_unix < 0.0:
		saved_at_unix = Time.get_unix_time_from_system()
	var data: Dictionary = {
		"version": SCHEMA_VERSION,
		"saved_at_unix": saved_at_unix,
		"economy": {
			"spores": Economy.spores,
			"run_spores": Economy.run_spores,
			"stage": Economy.stage,
			"run_elapsed_seconds": Economy.run_elapsed_seconds,
			"highest_stage_reached": Economy.highest_stage_reached,
			"mycelium": Economy.mycelium,
			"lifetime_mycelium": Economy.lifetime_mycelium,
			"levels": Economy.levels.duplicate(true),
			"specimens_collected": Economy.specimens_collected.duplicate(),
		},
		"garden": {
			"dew": Garden.dew,
			"lifetime_dew": Garden.lifetime_dew,
			"beds": Garden.beds,
			"drip_timer": Garden.drip_timer,
			"tonic_levels": Garden.tonic_levels.duplicate(true),
		},
	}
	var temporary_path: String = path + ".tmp"
	var file: FileAccess = FileAccess.open(temporary_path, FileAccess.WRITE)
	if file == null:
		push_error("Save: unable to open temporary save file at %s." % temporary_path)
		return false
	file.store_string(JSON.stringify(data))
	file.flush()
	file.close()
	var rename_error: Error = DirAccess.rename_absolute(
		ProjectSettings.globalize_path(temporary_path),
		ProjectSettings.globalize_path(path)
	)
	if rename_error != OK:
		push_error("Save: unable to replace save file at %s (error %d)." % [path, rename_error])
		return false
	return true


func load_game(path: String = SAVE_PATH, now_unix: float = -1.0) -> int:
	last_offline_report.clear()
	if not FileAccess.file_exists(path):
		Economy.reset_all()
		Garden.reset_all()
		return LoadStatus.MISSING

	var raw_bytes: PackedByteArray = FileAccess.get_file_as_bytes(path)
	var raw_text: String = raw_bytes.get_string_from_utf8()
	var parser: JSON = JSON.new()
	var parsed: Variant = null
	if not raw_text.is_empty() and parser.parse(raw_text) == OK:
		parsed = parser.data
	if not _is_valid_save_data(parsed, raw_text):
		_recover_corrupt_save(path, raw_bytes)
		return LoadStatus.CORRUPT_RESET

	var data: Dictionary = parsed as Dictionary
	var current_unix: float = now_unix
	if current_unix < 0.0:
		current_unix = Time.get_unix_time_from_system()
	_restoring_state = true
	_restore_state(data)
	_apply_offline_progress(data, current_unix)
	_restoring_state = false
	return LoadStatus.OK


func corrupt_backup_path(path: String) -> String:
	if path.ends_with(".json"):
		return path.trim_suffix(".json") + ".corrupt.bak"
	return path + ".corrupt.bak"


func _is_valid_save_data(parsed: Variant, raw_text: String) -> bool:
	if raw_text.is_empty() or not parsed is Dictionary:
		return false
	var data: Dictionary = parsed as Dictionary
	var version: Variant = data.get("version")
	if not _is_number(version) or int(version) != SCHEMA_VERSION:
		return false
	if not data.get("economy") is Dictionary or not data.get("garden") is Dictionary:
		return false
	if not _is_non_negative_number(data.get("saved_at_unix")):
		return false
	var economy_data: Dictionary = data["economy"] as Dictionary
	var garden_data: Dictionary = data["garden"] as Dictionary
	var economy_numeric_fields: Array[String] = [
		"spores",
		"run_spores",
		"stage",
		"run_elapsed_seconds",
		"highest_stage_reached",
		"mycelium",
		"lifetime_mycelium",
	]
	for field: String in economy_numeric_fields:
		if not _is_non_negative_number(economy_data.get(field)):
			return false
	var garden_numeric_fields: Array[String] = ["dew", "lifetime_dew", "beds", "drip_timer"]
	for field: String in garden_numeric_fields:
		if not _is_non_negative_number(garden_data.get(field)):
			return false
	if float(garden_data["beds"]) < 1.0:
		return false
	if economy_data.has("levels") and not economy_data["levels"] is Dictionary:
		return false
	if economy_data.has("specimens_collected") and not economy_data["specimens_collected"] is Array:
		return false
	if garden_data.has("tonic_levels") and not garden_data["tonic_levels"] is Dictionary:
		return false
	return true


func _is_number(value: Variant) -> bool:
	var value_type: int = typeof(value)
	if value_type != TYPE_INT and value_type != TYPE_FLOAT:
		return false
	var number: float = float(value)
	return not is_nan(number) and not is_inf(number)


func _is_non_negative_number(value: Variant) -> bool:
	return _is_number(value) and float(value) >= 0.0


func _recover_corrupt_save(path: String, raw_bytes: PackedByteArray) -> void:
	push_warning("Save: damaged save file at %s; backing up and resetting." % path)
	var backup_path: String = corrupt_backup_path(path)
	var backup: FileAccess = FileAccess.open(backup_path, FileAccess.WRITE)
	if backup == null:
		push_error("Save: unable to write corrupt save backup at %s." % backup_path)
	else:
		backup.store_buffer(raw_bytes)
		backup.flush()
		backup.close()
	Economy.reset_all()
	Garden.reset_all()
	save_corrupt_recovered.emit()


func _restore_state(data: Dictionary) -> void:
	Economy.reset_all()
	Garden.reset_all()
	var economy_data: Dictionary = data["economy"] as Dictionary
	var garden_data: Dictionary = data["garden"] as Dictionary
	Economy.spores = float(economy_data["spores"])
	Economy.run_spores = float(economy_data["run_spores"])
	Economy.stage = int(economy_data["stage"])
	Economy.run_elapsed_seconds = float(economy_data["run_elapsed_seconds"])
	Economy.highest_stage_reached = int(economy_data["highest_stage_reached"])
	Economy.mycelium = int(economy_data["mycelium"])
	Economy.lifetime_mycelium = int(economy_data["lifetime_mycelium"])
	var saved_levels: Dictionary = economy_data.get("levels", {}) as Dictionary
	for upgrade_value: Variant in Economy.upgrades:
		var upgrade: Dictionary = upgrade_value as Dictionary
		var id: String = String(upgrade["id"])
		var restored_level: int = maxi(int(saved_levels.get(id, 0)), 0)
		Economy.levels[id] = restored_level
	var saved_specimens: Array = economy_data.get("specimens_collected", []) as Array
	Economy.specimens_collected.clear()
	for specimen_value: Variant in saved_specimens:
		if specimen_value is String and not Economy.specimens_collected.has(String(specimen_value)):
			Economy.specimens_collected.append(String(specimen_value))

	Garden.dew = int(garden_data["dew"])
	Garden.lifetime_dew = int(garden_data["lifetime_dew"])
	Garden.beds = int(garden_data["beds"])
	Garden.drip_timer = float(garden_data["drip_timer"])
	var saved_tonics: Dictionary = garden_data.get("tonic_levels", {}) as Dictionary
	for tonic_value: Variant in Garden.get_tonics():
		var tonic: Dictionary = tonic_value as Dictionary
		var tonic_id: String = String(tonic["id"])
		Garden.tonic_levels[tonic_id] = maxi(int(saved_tonics.get(tonic_id, 0)), 0)

	Garden.sync_tonic_milestones()
	Economy.sync_permanent_unlocks()
	for upgrade_value: Variant in Economy.upgrades:
		var upgrade: Dictionary = upgrade_value as Dictionary
		var id: String = String(upgrade["id"])
		var maximum: int = Economy.effective_max_level(id)
		if maximum >= 0:
			Economy.levels[id] = clampi(Economy.level(id), 0, maximum)


func _apply_offline_progress(data: Dictionary, current_unix: float) -> void:
	var raw_away: float = maxf(0.0, current_unix - float(data["saved_at_unix"]))
	var cap: float = float(Economy.rules.get("offline_cap_seconds", 28800.0))
	var effective_away: float = minf(raw_away, cap)
	if effective_away < 1.0:
		return
	var dew_gained: int = 0
	if Garden.is_open():
		var drip_interval: float = Garden.drip_seconds()
		var total_timer: float = Garden.drip_timer + effective_away
		var drips: int = int(floor(total_timer / drip_interval))
		Garden.drip_timer = fmod(total_timer, drip_interval)
		dew_gained = drips * Garden.beds * Garden.dew_per_bed()
		Garden.dew += dew_gained
		Garden.lifetime_dew += dew_gained
		Garden.sync_tonic_milestones()
	Economy._try_clear_stage()
	var spores_gained: float = Economy.spores_per_sec() * effective_away
	Economy.spores += spores_gained
	Economy.run_spores += spores_gained
	Economy.run_elapsed_seconds += effective_away
	Economy._try_clear_stage()
	last_offline_report = {
		"effective_sec": effective_away,
		"raw_sec": raw_away,
		"spores_gained": spores_gained,
		"dew_gained": dew_gained,
	}
	offline_progress_applied.emit(effective_away, raw_away, spores_gained, dew_gained)


func _on_stage_cleared(_stage: int, _reward: float) -> void:
	_save_autosave_event()


func _on_prestiged(_gain: int) -> void:
	_save_autosave_event()


func _on_tonic_unlocked(_id: String) -> void:
	_save_autosave_event()


func _save_autosave_event() -> void:
	if not autosave_enabled or _restoring_state:
		return
	_autosave_timer = 0.0
	save_game()
