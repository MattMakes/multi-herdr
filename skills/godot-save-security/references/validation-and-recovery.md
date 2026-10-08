# Validation and recovery: field checks, player messages, test matrix

> Back to [SKILL.md](../SKILL.md).

Validation is the highest-value work for a single-player save. It needs no
key, and it protects against damage, bugs and edits at the same time. Do
it for every file from `user://`, with or without a tag.

## 1. Field checks

```gdscript
# save_fields.gd
class_name SaveFields
extends RefCounted
## Reads one field of a parsed save body with a type check and a range clamp.
## Run it on every field, also after a good tag: a bug in an older version
## can write a bad value with a good tag.


static func read_int(body: Dictionary, key: String, low: int, high: int, fallback: int) -> int:
	var value: Variant = body.get(key)
	# JSON gives every number as a float.
	if typeof(value) != TYPE_FLOAT and typeof(value) != TYPE_INT:
		return fallback
	var number := float(value)
	if not is_finite(number):
		return fallback
	return clampi(int(number), low, high)


static func read_float(body: Dictionary, key: String, low: float, high: float, fallback: float) -> float:
	var value: Variant = body.get(key)
	if typeof(value) != TYPE_FLOAT and typeof(value) != TYPE_INT:
		return fallback
	var number := float(value)
	if not is_finite(number):
		return fallback
	return clampf(number, low, high)


## Keeps only the ids that the game data knows, each clamped to its range.
## limits: id -> [low, high]. An unknown id is dropped, not kept.
static func read_levels(body: Dictionary, key: String, limits: Dictionary) -> Dictionary[String, int]:
	var out: Dictionary[String, int] = {}
	var raw: Variant = body.get(key)
	if typeof(raw) != TYPE_DICTIONARY:
		return out
	var levels: Dictionary = raw
	for id: Variant in levels:
		if typeof(id) != TYPE_STRING or not limits.has(id):
			continue
		var range_pair: Array = limits[id]
		out[id] = read_int(levels, id, int(range_pair[0]), int(range_pair[1]), int(range_pair[0]))
	return out
```

> proof: headless-run: on Godot 4.7.2, `-50` clamps to `0`, `1e999`
> parses as `inf` and falls back, a level of `99` clamps to its maximum, an
> unknown id is dropped, and a string where a number belongs falls back.

Rules for the load function:

1. Read each field through 1 checked reader. Never assign
   `body["coins"]` directly to game state.
2. Take the ranges from the game data (the same table that defines the
   upgrade levels or the item ids), not from new constants.
3. Drop unknown ids. Keep the rest of the save.
4. Reject `NaN` and infinity with `is_finite`. JSON gives `inf` for a large
   exponent, and Godot prints only a warning.
5. Treat a version newer than the game as read-only (`TOO_NEW`).
6. `ConfigFile` settings get the same treatment: a sanitizer per key, with
   type, range and an allow-list of keys.

## 2. What the player sees

Never fail without a message, and never start a new game without a
message. Each outcome of `SaveStore.read` has 1 message:

| Outcome | Message | Buttons |
| --- | --- | --- |
| `OK`, `NEW` | none | none |
| `MODIFIED` | none, or a small "Modified" mark in the profile screen (designer choice) | none |
| `RECOVERED` | "Your last save could not be read. The game loaded a backup from <time>. A copy of the damaged file is in <folder>." | Continue; Open save folder |
| `FAILED` | "Your save could not be read, and no backup could be read. Copies of the files are in <folder>." | Start new game; Open save folder; Quit |
| `TOO_NEW` | "This save is from a newer version of the game. Update the game to continue. The game did not change the save." | Quit |

- Take <time> from the modified time of the backup:
  `FileAccess.get_modified_time(path)`.
- Open the folder with
  `OS.shell_show_in_file_manager(ProjectSettings.globalize_path("user://quarantine"))`.
- Log the reason (`CORRUPT`, `BAD_MAC`, `UNKNOWN_KEY`, `TOO_NEW`) in the
  local log, so a support request can name it.
- Never show "tampering detected" or a similar accusation. Damage is the
  more likely cause.

## 3. Test matrix

Run each row headless before a release. Each row is a fixture file and an
expected outcome.

| # | Fixture | Expected |
| --- | --- | --- |
| 1 | No save | `NEW` |
| 2 | Empty main file | Backup loads (`RECOVERED`), or `FAILED` |
| 3 | Truncated main file (half the bytes) | Same as row 2 |
| 4 | Broken JSON | Same as row 2 |
| 5 | Edited body, `REFUSE` | `RECOVERED` from `.bak1`, reason `BAD_MAC` |
| 6 | Edited body, `LOAD_MARKED` | `MODIFIED`, the flag survives the next write |
| 7 | Envelope sealed under an old key id | `OK`, and the file has the current key id after the read |
| 8 | Envelope version above the game's | `TOO_NEW`, the file does not change |
| 9 | Main and all backups bad | `FAILED`, quarantine has a copy, nothing deleted |
| 10 | An older copy put back over the main file | `OK` with `rolled_back` |
| 11 | A save from another install or account | `BAD_MAC` |
| 12 | Body fields out of range, NaN, unknown ids | Clamped, fallback, dropped |
| 13 | Full disk or a read-only folder | `write` returns an error; the old save and backups do not change |

Row 13 needs a real disk condition. proof: not run (needs a full volume).
Rows 1 to 12 ran on Godot 4.7.2 headless.
