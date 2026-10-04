Adds a whole-project health review: a headless audit script that measures typing, coupling, path casing and scene weight, and a weighted score with evidence; read it when the orchestrator asks for a project-wide review or a before/after quality comparison, not for a single-change review.

# Project health scoring

> ← Back to [SKILL.md](../SKILL.md)

A change review (SKILL.md) judges a diff. A health review judges the whole
project and must be repeatable: the same project gives the same numbers,
so a later review can show progress. Measure first, then judge.

## Step 1: measure with the audit script

The script reads every `.gd` and `.tscn` file under a folder (skipping
`.godot/`, `addons/` and folders with a `.gdignore`) and prints one line
per finding plus a summary. It edits nothing.

```gdscript
# tools/review_audit.gd - measurable review facts for a Godot project.
# Run: godot --headless --path . -s res://tools/review_audit.gd -- res://
# Prints one line per finding and a summary block. Exit code 0 always:
# the numbers feed a review; they are not a gate by themselves.
extends SceneTree

const SKIP_DIRS: PackedStringArray = [".godot", "addons", ".git"]

var _rx_untyped_var := RegEx.create_from_string("^\\s*var\\s+\\w+\\s*(=|$)")
var _rx_func := RegEx.create_from_string("^\\s*(static\\s+)?func\\s+\\w+\\s*\\((.*)\\)\\s*(->\\s*[\\w\\[\\], ]+)?\\s*:")
var _rx_string_connect := RegEx.create_from_string("\\bconnect\\(\\s*\"")
var _rx_get_parent := RegEx.create_from_string("get_parent\\(\\)")
var _rx_abs_path := RegEx.create_from_string("[\"^$]/root/")
var _rx_untyped_collection := RegEx.create_from_string(":\\s*(Array|Dictionary)\\s*(=|$)")

var counts: Dictionary = {
	"scripts": 0, "funcs": 0, "funcs_no_return_type": 0, "untyped_vars": 0,
	"untyped_collections": 0, "string_connects": 0, "get_parent_calls": 0,
	"absolute_node_paths": 0, "path_case_issues": 0, "scenes": 0, "heavy_scenes": 0,
}


func _init() -> void:
	var args: PackedStringArray = OS.get_cmdline_user_args()
	var start: String = args[0] if args.size() > 0 else "res://"
	_walk(start)
	print("SUMMARY " + JSON.stringify(counts))
	quit(0)


func _walk(dir_path: String) -> void:
	for file_name: String in DirAccess.get_files_at(dir_path):
		var path: String = dir_path.path_join(file_name)
		if file_name != file_name.to_lower():
			counts.path_case_issues += 1
			print("CASE %s (use lowercase snake_case)" % path)
		if file_name.ends_with(".gd"):
			_audit_script(path)
		elif file_name.ends_with(".tscn"):
			_audit_scene(path)
	for sub: String in DirAccess.get_directories_at(dir_path):
		if sub in SKIP_DIRS or sub.begins_with("."):
			continue
		if FileAccess.file_exists(dir_path.path_join(sub).path_join(".gdignore")):
			continue
		if sub != sub.to_lower():
			counts.path_case_issues += 1
			print("CASE %s/ (use lowercase snake_case)" % dir_path.path_join(sub))
		_walk(dir_path.path_join(sub))


func _audit_script(path: String) -> void:
	counts.scripts += 1
	var lines: PackedStringArray = FileAccess.get_file_as_string(path).split("\n")
	for i: int in lines.size():
		var line: String = lines[i]
		if line.strip_edges().begins_with("#"):
			continue
		var where: String = "%s:%d" % [path, i + 1]
		var f: RegExMatch = _rx_func.search(line)
		if f != null:
			counts.funcs += 1
			if f.get_string(3).is_empty():
				counts.funcs_no_return_type += 1
				print("NO_RETURN_TYPE %s" % where)
		if _rx_untyped_var.search(line) != null:
			counts.untyped_vars += 1
			print("UNTYPED_VAR %s" % where)
		if _rx_untyped_collection.search(line) != null:
			counts.untyped_collections += 1
			print("UNTYPED_COLLECTION %s" % where)
		if _rx_string_connect.search(line) != null:
			counts.string_connects += 1
			print("STRING_CONNECT %s" % where)
		if _rx_get_parent.search(line) != null:
			counts.get_parent_calls += 1
			print("GET_PARENT %s" % where)
		if _rx_abs_path.search(line) != null:
			counts.absolute_node_paths += 1
			print("ABSOLUTE_PATH %s" % where)


func _audit_scene(path: String) -> void:
	counts.scenes += 1
	var deps: PackedStringArray = ResourceLoader.get_dependencies(path)
	if deps.size() >= 30:
		counts.heavy_scenes += 1
		print("HEAVY_SCENE %s depends on %d resources" % [path, deps.size()])
```

```bash
godot --headless --path . -s res://tools/review_audit.gd -- res:// > audit.txt
grep '^SUMMARY' audit.txt
```

The checks are line-based regular expressions, so they are a measurement,
not a parser: a `var` inside a multi-line string counts too. Read the
finding lines before you quote a number. Tested on Godot 4.7.2 against a
sample script with one planted issue of each kind; every one was found.

If the reviewer seat cannot write files (a read-only review teammate),
ask the orchestrator for a worker to add and run the script, or run the
same checks with `grep` and say so.

## Step 2: score

Each area scores 0 to 10 from the measured ratios, then gets its weight.
Write the measured value next to every score.

| Area | Weight | 10 points | 5 points | 0 points |
|---|---|---|---|---|
| Typing | 25 % | `untyped_vars` + `untyped_collections` ≤ 2 % of declarations, `funcs_no_return_type` ≤ 5 % of `funcs` | about 20 % untyped | most code untyped |
| Coupling | 25 % | No `get_parent` calls into game logic, no absolute node paths, no string `connect` | A few, documented | Common |
| Structure | 20 % | Folders by feature (`player/`, `enemies/`), zero `path_case_issues`, no scene with 30 or more dependencies | Mixed layout | One flat `scripts/` folder; case issues |
| Tests | 15 % | A test suite runs headless in CI and covers core rules | Some tests, not in CI | None |
| Runtime hygiene | 15 % | Zero orphans after a scene round trip; no warnings at startup; budgets met | Some warnings | Errors at startup |

Score = Σ (area points × weight) on a 0 to 10 scale.

| Score | Reading |
|---|---|
| 8.5 and up | Healthy. Changes are cheap and safe. |
| 6.5 to 8.4 | Workable. Name the 3 fixes with the highest payoff. |
| 4.5 to 6.4 | Fragile. Fix coupling and typing before new features. |
| Below 4.5 | Plan a cleanup unit before more work lands. |

The runtime hygiene row needs a run: the smoke run from
**godot-testing** (`references/deterministic-tests.md`) and the orphan
check (`references/budgets-leaks-and-network-tests.md`).

## Step 3: report

Report in this shape, so the next review can compare:

```text
## Project health — <project> @ <commit>

Godot 4.7.2 · audit: tools/review_audit.gd · tests: <command and result>

| Area | Measured | Points | Weight |
|---|---|---|---|
| Typing | 4 untyped of 310 declarations; 12 of 140 funcs without return type | 8 | 25 % |
| ...

Score: 7.3 / 10

### Highest payoff fixes
1. <finding> — <files> — <why it pays>
2. ...
3. ...

### Not measured
- <what could not be checked, and why>
```

Rules:
- Every number comes from the audit output or a command named in the
  report. No score without its measurement.
- "Not measured" is required. A blank area is better than a guess.
- Compare like with like: the same script version and the same folder.
  When the script changes, say so and re-run the old commit if the
  comparison matters.
