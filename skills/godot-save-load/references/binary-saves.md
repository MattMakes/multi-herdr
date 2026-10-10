Adds binary saves with `store_var` / `get_var`, the `allow_objects` trust rule, compression, and the JSON number-type trap. Read it when a save must keep Godot types (Vector3, int, Color) exactly, or when save files get large.

> ← Back to [SKILL.md](../SKILL.md)

# Binary Saves and Type Fidelity

## JSON loses types

Two facts, measured on Godot 4.7.2:

- `JSON.parse_string('{"a": 3}')` returns `3` as a **float** (`TYPE_FLOAT`). Every JSON number comes back as a float, including array items.
- JSON has no `Vector2`, `Vector3`, `Color` or `StringName`. You must write them as components and rebuild them.

So a JSON loader converts on the way in: `int(data.get("level", 1))`, `Vector3(float(p["x"]), float(p["y"]), float(p["z"]))`. A loader that skips `int()` stores a float in an `int`-typed property at best, and fails a `match` or a `Dictionary` key lookup at worst (`3.0` and `3` are different keys).

## The binary path keeps types

`FileAccess.store_var()` and `get_var()` write Godot's own binary Variant encoding. A `Dictionary` of `int`, `float`, `String`, vectors, colors, arrays and nested dictionaries comes back with the same types.

```gdscript
# binary_save.gd
class_name BinarySave
extends RefCounted

const VERSION := 3


static func write(path: String, state: Dictionary) -> Error:
    var file := FileAccess.open(path, FileAccess.WRITE)
    if file == null:
        return FileAccess.get_open_error()
    var payload := {"version": VERSION, "state": state}
    # full_objects stays false: plain data only, never encoded Objects.
    if not file.store_var(payload, false):
        return ERR_FILE_CANT_WRITE
    return OK


## Returns {} when the file is missing, unreadable or not a save.
static func read(path: String) -> Dictionary:
    if not FileAccess.file_exists(path):
        return {}
    var file := FileAccess.open(path, FileAccess.READ)
    if file == null:
        push_error("BinarySave.read: cannot open %s (%s)" % [path, error_string(FileAccess.get_open_error())])
        return {}
    # allow_objects stays false. A save file is untrusted input.
    var payload: Variant = file.get_var(false)
    if typeof(payload) != TYPE_DICTIONARY or not payload.has("state"):
        push_error("BinarySave.read: %s is not a save file" % path)
        return {}
    return payload
```

## `allow_objects` is a trust boundary

| Data source | `store_var(..., full_objects)` / `get_var(allow_objects)` |
|---|---|
| Player saves in `user://`, cloud saves, shared saves, mods, downloads | `false`, always |
| A fixture your own build tool wrote and the player cannot edit | `true` only if you must, with a comment that says why |

With `allow_objects = true`, the decoder can build Objects, and an Object can carry a script. A crafted save file then runs code in the game. This is the same risk SKILL.md names for `.tres` and `.res` files. `bytes_to_var()` is the safe in-memory decoder; `bytes_to_var_with_objects()` has the same risk as `allow_objects = true`.

Even with objects off, validate what you read. A player can edit any save: check types with `typeof()` and clamp numbers to the ranges the game allows before you apply them.

## Compression

`FileAccess.open_compressed()` wraps the file in a compressor. The read side must pass the same mode.

```gdscript
# compressed_save.gd
class_name CompressedSave
extends RefCounted


static func write(path: String, state: Dictionary) -> Error:
    var file := FileAccess.open_compressed(path, FileAccess.WRITE, FileAccess.COMPRESSION_ZSTD)
    if file == null:
        return FileAccess.get_open_error()
    return OK if file.store_var(state, false) else ERR_FILE_CANT_WRITE


static func read(path: String) -> Variant:
    var file := FileAccess.open_compressed(path, FileAccess.READ, FileAccess.COMPRESSION_ZSTD)
    if file == null:
        return null
    return file.get_var(false)
```

- Compression pays off for large world state (explored-tile maps, many placed objects). A 2 KB progress save gains nothing.
- For data already in memory, `PackedByteArray.compress()` and `decompress()` do the same job. `decompress()` needs the uncompressed size, so store it next to the data, or use `decompress_dynamic()`.

## Choosing JSON or binary

| Need | Take |
|---|---|
| Players or QA read and edit saves | JSON, with explicit conversion on load |
| Exact types, large state, faster load | `store_var` with `full_objects = false` |
| Both | Binary save, plus a debug command that dumps it as JSON |

Both formats still need the `version` field and the migration chain from [version-migration.md](version-migration.md). After an engine upgrade, load a save written by the previous build in a test before you ship: the binary encoding of some types changed between 4.x releases.
