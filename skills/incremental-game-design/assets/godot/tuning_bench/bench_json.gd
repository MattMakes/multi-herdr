extends RefCounted
## JSON helpers of the tuning bench.
##
## The parser of Godot makes each number a float. This parser keeps an integer as an integer.
## The bench needs that: an integer number of the game stays an integer after a control changes it.

var _s: String = ""
var _i: int = 0
var _n: int = 0
var error: String = ""


## Parse a JSON text. Return null when the text is not correct. `error` then has the cause.
func parse(text: String) -> Variant:
	_s = text
	_i = 0
	_n = text.length()
	error = ""
	var value: Variant = _value()
	if error != "":
		return null
	_space()
	if _i < _n:
		error = "text after the end of the JSON value at character %d" % _i
		return null
	return value


func _space() -> void:
	while _i < _n:
		var c: int = _s.unicode_at(_i)
		if c == 32 or c == 10 or c == 13 or c == 9:
			_i += 1
		else:
			return


func _value() -> Variant:
	_space()
	if _i >= _n:
		error = "the JSON text ends too early"
		return null
	var c: int = _s.unicode_at(_i)
	if c == 123:
		return _object()
	if c == 91:
		return _array()
	if c == 34:
		return _string()
	if _s.substr(_i, 4) == "true":
		_i += 4
		return true
	if _s.substr(_i, 5) == "false":
		_i += 5
		return false
	if _s.substr(_i, 4) == "null":
		_i += 4
		return null
	return _number()


func _object() -> Variant:
	var out: Dictionary = {}
	_i += 1
	_space()
	if _i < _n and _s.unicode_at(_i) == 125:
		_i += 1
		return out
	while true:
		_space()
		if _i >= _n or _s.unicode_at(_i) != 34:
			error = "a key is necessary at character %d" % _i
			return null
		var key: Variant = _string()
		if error != "":
			return null
		_space()
		if _i >= _n or _s.unicode_at(_i) != 58:
			error = "a colon is necessary at character %d" % _i
			return null
		_i += 1
		var value: Variant = _value()
		if error != "":
			return null
		out[key] = value
		_space()
		if _i < _n and _s.unicode_at(_i) == 44:
			_i += 1
			continue
		if _i < _n and _s.unicode_at(_i) == 125:
			_i += 1
			return out
		error = "a comma or a closing brace is necessary at character %d" % _i
		return null
	return null


func _array() -> Variant:
	var out: Array = []
	_i += 1
	_space()
	if _i < _n and _s.unicode_at(_i) == 93:
		_i += 1
		return out
	while true:
		var value: Variant = _value()
		if error != "":
			return null
		out.append(value)
		_space()
		if _i < _n and _s.unicode_at(_i) == 44:
			_i += 1
			continue
		if _i < _n and _s.unicode_at(_i) == 93:
			_i += 1
			return out
		error = "a comma or a closing bracket is necessary at character %d" % _i
		return null
	return null


func _string() -> Variant:
	var start: int = _i
	_i += 1
	var escaped: bool = false
	while _i < _n:
		var c: int = _s.unicode_at(_i)
		if c == 92:
			escaped = true
			_i += 2
			continue
		if c == 34:
			_i += 1
			if not escaped:
				return _s.substr(start + 1, _i - start - 2)
			var parsed: Variant = JSON.parse_string(_s.substr(start, _i - start))
			if parsed is String:
				return parsed
			error = "a string is not correct at character %d" % start
			return null
		_i += 1
	error = "a string does not end (start at character %d)" % start
	return null


func _number() -> Variant:
	var start: int = _i
	var is_float: bool = false
	while _i < _n:
		var c: int = _s.unicode_at(_i)
		if (c >= 48 and c <= 57) or c == 45 or c == 43:
			_i += 1
		elif c == 46 or c == 101 or c == 69:
			is_float = true
			_i += 1
		else:
			break
	var token: String = _s.substr(start, _i - start)
	if token == "" or not token.is_valid_float():
		error = "a value is not correct at character %d" % start
		return null
	if is_float:
		return token.to_float()
	return token.to_int()


## Parse a text. Return {"ok": bool, "error": String, "value": Variant}.
static func parse_text(text: String) -> Dictionary:
	var parser: RefCounted = load("res://addons/tuning_bench/bench_json.gd").new()
	var value: Variant = parser.call("parse", text)
	var err: String = parser.get("error")
	return {"ok": err == "", "error": err, "value": value}


## Read and parse a file. Return {"ok": bool, "error": String, "value": Variant}.
static func read_file(path: String) -> Dictionary:
	if not FileAccess.file_exists(path):
		return {"ok": false, "error": "the file %s does not exist" % path, "value": null}
	var file: FileAccess = FileAccess.open(path, FileAccess.READ)
	if file == null:
		return {"ok": false, "error": "cannot open the file %s" % path, "value": null}
	var text: String = file.get_as_text()
	file.close()
	var out: Dictionary = parse_text(text)
	if not out["ok"]:
		out["error"] = "%s: %s" % [path, out["error"]]
	return out


## Write a value as JSON. The write is atomic: a reader sees the old file or the full new file.
static func write_file(path: String, value: Variant, sort_keys: bool = true) -> bool:
	var text: String = JSON.stringify(value, "  ", sort_keys) + "\n"
	var dir: String = path.get_base_dir()
	if dir != "" and not DirAccess.dir_exists_absolute(dir):
		DirAccess.make_dir_recursive_absolute(dir)
	var tmp: String = path + ".tmp"
	var file: FileAccess = FileAccess.open(tmp, FileAccess.WRITE)
	if file == null:
		return false
	file.store_string(text)
	file.close()
	if FileAccess.file_exists(path):
		DirAccess.remove_absolute(path)
	return DirAccess.rename_absolute(tmp, path) == OK


static func _tokens(pointer: String) -> PackedStringArray:
	var out: PackedStringArray = PackedStringArray()
	if pointer == "":
		return out
	var parts: PackedStringArray = pointer.split("/")
	for index: int in range(1, parts.size()):
		out.append(parts[index].replace("~1", "/").replace("~0", "~"))
	return out


## Get the value at a JSON pointer. Return [found, value].
static func pointer_get(doc: Variant, pointer: String) -> Array:
	var node: Variant = doc
	for token: String in _tokens(pointer):
		if node is Dictionary:
			var dict: Dictionary = node
			if not dict.has(token):
				return [false, null]
			node = dict[token]
		elif node is Array:
			var list: Array = node
			if not token.is_valid_int():
				return [false, null]
			var index: int = token.to_int()
			if index < 0 or index >= list.size():
				return [false, null]
			node = list[index]
		else:
			return [false, null]
	return [true, node]


## Set the value at a JSON pointer. Return false when the pointer does not exist.
static func pointer_set(doc: Variant, pointer: String, value: Variant) -> bool:
	var tokens: PackedStringArray = _tokens(pointer)
	if tokens.is_empty():
		return false
	var node: Variant = doc
	for index: int in range(tokens.size()):
		var token: String = tokens[index]
		var last: bool = index == tokens.size() - 1
		if node is Dictionary:
			var dict: Dictionary = node
			if not dict.has(token):
				return false
			if last:
				dict[token] = value
				return true
			node = dict[token]
		elif node is Array:
			var list: Array = node
			if not token.is_valid_int():
				return false
			var at: int = token.to_int()
			if at < 0 or at >= list.size():
				return false
			if last:
				list[at] = value
				return true
			node = list[at]
		else:
			return false
	return false


## Round a number to 6 significant digits. The result files then stay short and stable.
static func sig(value: float) -> float:
	if value == 0.0 or is_nan(value) or is_inf(value):
		return 0.0 if is_nan(value) else value
	var digits: int = 5 - int(floor(log(absf(value)) / log(10.0)))
	if digits > 15:
		return 0.0
	if digits <= 0:
		var down: float = pow(10.0, float(-digits))
		return roundf(value / down) * down
	var up: float = pow(10.0, float(digits))
	return roundf(value * up) / up


## Round a time to 1 millisecond.
static func ms(value: float) -> float:
	return roundf(value * 1000.0) / 1000.0
