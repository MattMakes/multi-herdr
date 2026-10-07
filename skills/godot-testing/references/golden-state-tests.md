Adds golden-file tests for game state: serialize a system's state to stable JSON, compare it with a committed golden file, and regenerate goldens on purpose; read it when a save format, quest log, inventory or generator output must not change by accident.

# Golden state tests

> ← Back to [SKILL.md](../SKILL.md)

A golden test runs a fixed scenario, turns the resulting state into text,
and compares it with a file committed in the repository. Any change in the
output fails the test until someone looks at the diff and accepts it.

Good targets:
- Save files: load an old save, save it again, compare.
- Procedural generation with a fixed seed: same seed, same map.
- Quest and dialogue state after a scripted sequence of events.
- Balance tables computed from data resources.

Not a target: rendered images. Headless test runs draw nothing (see
[deterministic-tests.md](deterministic-tests.md)).

## Make the text stable

The comparison is only useful if the same state always gives the same
text.

- `JSON.stringify(data, "\t")` sorts dictionary keys by default
  (`sort_keys` is `true`) and indents with tabs, which gives readable
  diffs in review.
- Floats print with many digits and can differ in the last digit between
  platforms. Round them before serializing (`snappedf(value, 0.001)`), or
  store integers (cents, tiles, milliseconds).
- Convert engine types that JSON cannot hold (`Vector2`, `Color`,
  `StringName`) into arrays or strings yourself, in a fixed form.
- Leave out values that change every run: timestamps, instance ids,
  absolute paths.

## A golden helper

```gdscript
# test_support/golden.gd
class_name Golden
extends RefCounted

const DIR: String = "res://tests/goldens"


## Compare `state` with res://tests/goldens/<name>.json.
## Returns "" on a match, or a message with the first differing line.
## With the environment variable UPDATE_GOLDENS=1 it rewrites the golden
## file instead and returns "".
static func check(name: String, state: Variant) -> String:
	var actual: String = JSON.stringify(normalize(state), "\t") + "\n"
	var path: String = DIR.path_join(name + ".json")
	if OS.get_environment("UPDATE_GOLDENS") == "1":
		DirAccess.make_dir_recursive_absolute(ProjectSettings.globalize_path(DIR))
		var out := FileAccess.open(path, FileAccess.WRITE)
		if out == null:
			return "cannot write %s" % path
		out.store_string(actual)
		return ""
	if not FileAccess.file_exists(path):
		return "missing golden %s; run once with UPDATE_GOLDENS=1 and review the file" % path
	var expected: String = FileAccess.get_file_as_string(path)
	if expected == actual:
		return ""
	var exp_lines: PackedStringArray = expected.split("\n")
	var act_lines: PackedStringArray = actual.split("\n")
	for i: int in maxi(exp_lines.size(), act_lines.size()):
		var e: String = exp_lines[i] if i < exp_lines.size() else "<end>"
		var a: String = act_lines[i] if i < act_lines.size() else "<end>"
		if e != a:
			return "%s line %d\n  expected: %s\n  actual:   %s" % [path, i + 1, e, a]
	return "%s differs" % path


## Turn engine types into JSON-safe, stable values.
static func normalize(value: Variant) -> Variant:
	match typeof(value):
		TYPE_FLOAT:
			return snappedf(value, 0.001)
		TYPE_VECTOR2, TYPE_VECTOR2I:
			return [normalize(value.x), normalize(value.y)]
		TYPE_VECTOR3, TYPE_VECTOR3I:
			return [normalize(value.x), normalize(value.y), normalize(value.z)]
		TYPE_COLOR:
			return (value as Color).to_html()
		TYPE_STRING_NAME, TYPE_NODE_PATH:
			return str(value)
		TYPE_DICTIONARY:
			var out: Dictionary = {}
			for key: Variant in value:
				out[str(key)] = normalize(value[key])
			return out
		TYPE_ARRAY, TYPE_PACKED_INT32_ARRAY, TYPE_PACKED_INT64_ARRAY, TYPE_PACKED_FLOAT32_ARRAY, TYPE_PACKED_FLOAT64_ARRAY, TYPE_PACKED_STRING_ARRAY:
			var arr: Array = []
			for item: Variant in value:
				arr.append(normalize(item))
			return arr
	return value
```

In a test (GUT shown; in gdUnit4 use `assert_str(msg).is_empty()`):

<!-- gdscript-check: skip -->
```gdscript
func test_dungeon_seed_42_is_stable() -> void:
	var map: DungeonMap = DungeonGenerator.new().generate(42)
	var msg: String = Golden.check("dungeon_seed_42", map.to_dict())
	assert_eq(msg, "", msg)
```

## Updating goldens is a reviewed change

`gd` runs Godot through `godot-build-verify`'s wrapper; see its `references/commands.md`, Common setup.

```bash
# Regenerate after an intended change, then review the diff like code.
UPDATE_GOLDENS=1 gd -s addons/gut/gut_cmdln.gd -gdir=res://tests -gexit
git diff -- tests/goldens/
```

Rules for a fleet worker:
- Regenerate only when the plan says the output should change. State in
  `DONE:` which goldens changed and why.
- Never regenerate to make a red test green without reading the diff. A
  golden that changed for an unknown reason is a bug report.
- Commit golden files with the code change that caused them, in the same
  commit.
- Keep goldens small: a 20,000-line golden is never reviewed. Test a
  small seed, a small map or one save slot.
