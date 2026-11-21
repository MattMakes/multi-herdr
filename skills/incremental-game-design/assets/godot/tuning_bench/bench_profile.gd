extends "res://addons/tuning_bench/bench_adapter.gd"

const NAMES: Array[String] = ["setup", "step", "actions", "do_action", "rate", "progress", "wealth", "active", "away", "events", "max_step"]

var inner: RefCounted = null
var spent: Dictionary = {}


func wrap(adapter: RefCounted) -> void:
	inner = adapter
	bench_config = adapter.bench_config
	bench_seed = adapter.bench_seed
	bench_errors = adapter.bench_errors
	clear()


func clear() -> void:
	spent = {}
	for name: String in NAMES:
		spent[name] = [0, 0]


func take() -> Dictionary:
	var out: Dictionary = spent
	clear()
	return out


func _note(name: String, started: int) -> void:
	var row: Array = spent[name]
	row[0] += 1
	row[1] += Time.get_ticks_usec() - started


func setup(data: Dictionary) -> void:
	var started: int = Time.get_ticks_usec()
	inner.bench_away_missing = false
	inner.setup(data)
	_note("setup", started)


func step(dt: float) -> void:
	var started: int = Time.get_ticks_usec()
	inner.step(dt)
	var row: Array = spent["step"]
	row[0] += 1
	row[1] += Time.get_ticks_usec() - started


func actions() -> Array:
	var started: int = Time.get_ticks_usec()
	var out: Array = inner.actions()
	_note("actions", started)
	return out


func do_action(id: String) -> bool:
	var started: int = Time.get_ticks_usec()
	var out: bool = inner.do_action(id)
	_note("do_action", started)
	return out


func rate() -> float:
	var started: int = Time.get_ticks_usec()
	var out: float = inner.rate()
	_note("rate", started)
	return out


func progress() -> float:
	var started: int = Time.get_ticks_usec()
	var out: float = inner.progress()
	_note("progress", started)
	return out


func max_step() -> float:
	var started: int = Time.get_ticks_usec()
	var out: float = inner.max_step()
	_note("max_step", started)
	return out


func wealth() -> float:
	var started: int = Time.get_ticks_usec()
	var out: float = inner.wealth()
	_note("wealth", started)
	return out


func active(seconds: float) -> void:
	var started: int = Time.get_ticks_usec()
	inner.active(seconds)
	_note("active", started)


func away(seconds: float) -> void:
	var started: int = Time.get_ticks_usec()
	inner.away(seconds)
	bench_away_missing = inner.bench_away_missing
	_note("away", started)


func events() -> Array:
	var started: int = Time.get_ticks_usec()
	var out: Array = inner.events()
	_note("events", started)
	return out


func apply_live(data: Dictionary) -> bool:
	return inner.apply_live(data)


func isolated() -> bool:
	return inner.isolated()


static func lines(title: String, spent_now: Dictionary, total_usec: int) -> PackedStringArray:
	var out: PackedStringArray = PackedStringArray()
	var total: float = maxf(float(total_usec), 1.0)
	out.append("profile %s: %.2f s" % [title, total / 1.0e6])
	out.append("  %-10s %10s %9s %6s %10s" % ["function", "calls", "seconds", "share", "us/call"])
	var names: Array = spent_now.keys()
	names.sort_custom(func(a: Variant, b: Variant) -> bool:
		if int(spent_now[a][1]) != int(spent_now[b][1]):
			return int(spent_now[a][1]) > int(spent_now[b][1])
		return str(a) < str(b))
	var inside: int = 0
	for name: Variant in names:
		var calls: int = int(spent_now[name][0])
		var usec: int = int(spent_now[name][1])
		inside += usec
		if calls == 0:
			continue
		out.append("  %-10s %10d %9.2f %5.1f%% %10.2f" % [name, calls, float(usec) / 1.0e6, 100.0 * float(usec) / total, float(usec) / float(calls)])
	var rest: int = maxi(total_usec - inside, 0)
	out.append("  %-10s %10s %9.2f %5.1f%%" % ["(bench)", "", float(rest) / 1.0e6, 100.0 * float(rest) / total])
	return out
