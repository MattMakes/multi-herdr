class_name BenchAdapter
extends RefCounted

var bench_errors: Array[String] = []

var bench_config: Dictionary = {}
var bench_seed: int = 1

var bench_away_missing: bool = false


func bench_prepare() -> String:
	return ""


func setup(_data: Dictionary) -> void:
	_missing("setup")


func step(_dt: float) -> void:
	_missing("step")


func actions() -> Array:
	_missing("actions")
	return []


func do_action(_id: String) -> bool:
	_missing("do_action")
	return false


func rate() -> float:
	_missing("rate")
	return 0.0


func progress() -> float:
	_missing("progress")
	return 0.0


func max_step() -> float:
	return 1.0


func wealth() -> float:
	return -1.0


func active(_seconds: float) -> void:
	pass


func away(_seconds: float) -> void:
	bench_away_missing = true


func events() -> Array:
	return []


func apply_live(_data: Dictionary) -> bool:
	return false


func isolated() -> bool:
	return false


func _missing(method: String) -> void:
	var line: String = "the adapter does not have the required method %s()" % method
	if not bench_errors.has(line):
		bench_errors.append(line)
