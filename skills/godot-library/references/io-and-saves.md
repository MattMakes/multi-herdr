# IO and saves, advanced

Target engine: Godot 4.7. This shelf covers paths, file and directory access, serialization
formats and their trust rules, crash-safe and versioned saves, runtime loading of loose
files, background loading and logging.

## Use this shelf when

- You pick a save format and must weigh size, types, security and cloud sync.
- A save must survive a crash, a full disk, a version change or a hostile file.
- You load user content at runtime: images, audio, glTF, fonts, ZIP or PCK mods.
- A load hitches the frame, or storage is slow or not persistent (Web, mobile).

## How the engine works here

- `res://` is a virtual file system. In an export it is a read-only PCK, source assets are
  replaced by imported files, and names are remapped. `FileAccess.file_exists()` and
  `DirAccess` can miss files that `ResourceLoader.exists()` and `ResourceLoader.list_directory()`
  find. Non-resource files (`.txt`, `.json`) are exported only if the export filter lists
  them or the import mode is "Keep File".
- `user://` is always writable. By default it sits inside the editor data folder
  (`.../Godot/app_userdata/<project name>`), so it moves if the project is renamed. Set
  `application/config/use_custom_user_dir` and `custom_user_dir_name` (may contain
  `Studio/Game`) before the first public build. `OS.get_user_data_dir()` returns the OS path.
- On Web, `user://` is a virtual file system kept in IndexedDB. It needs cookies allowed
  (third-party cookies inside an iframe) and is lost in private browsing.
  `OS.is_userfs_persistent()` checks it but can report false positives (docs).
- `FileAccess` is `RefCounted`: the file closes when the last reference dies. A killed
  process (editor stop button, crash) does not flush. `flush()` forces the buffer out and
  costs a disk write each call.
- Three serializers exist. Know which can build objects:

| Format | API | Types kept | Can run code on load |
|---|---|---|---|
| JSON text | `JSON.stringify()` / `JSON.parse_string()` | JSON only; every number is a float | no |
| JSON with native types | `JSON.from_native()` / `JSON.to_native()` (since 4.4) | all Variant types | only with `allow_objects` |
| Variant binary | `var_to_bytes()`, `FileAccess.store_var()` | all Variant types, compact | only with objects allowed |
| Variant text | `var_to_str()`, `ConfigFile` | all Variant types, readable | treat as untrusted input |
| Resource | `ResourceSaver.save()` / `ResourceLoader.load()` | whole object graphs | yes: embedded scripts run |

- The Variant binary format is little-endian, padded to 4 bytes, with a type header per
  value. It is not the format of `.res` or `.scn` files. Objects are stored only as instance
  ids unless full objects are requested; then only `PROPERTY_USAGE_STORAGE` properties go in.

## Advanced techniques

### Choose the format by trust boundary
A file the player can swap (shared saves, cloud saves, mods) is untrusted. For those use
JSON or the Variant binary format with objects off, and validate every field after load.
`ResourceLoader.load()` on a `.tres` or `.res` from outside the game will run a script
embedded in it at load time (reported by GDQuest and the Safe Resource Loader addon; the
class reference gives the same warning for `bytes_to_var_with_objects()`). Keep Resource
saves for data that never leaves the machine, or do not use them for saves at all.
`godot-save-security` has the threat model and signing recipes.

### Crash-safe writes: temp file, then rename
Never open the live save with `FileAccess.WRITE`: that truncates it first, and a crash
leaves an empty file. Write a sibling temp file, check each `store_*` result (they return
`bool` since 4.4), close, then rename over the target.

```gdscript
extends Node

const SAVE_NAME: String = "slot_0.json"

func write_save(text: String) -> Error:
	var tmp_path: String = "user://" + SAVE_NAME + ".tmp"
	var file: FileAccess = FileAccess.open(tmp_path, FileAccess.WRITE)
	if file == null:
		return FileAccess.get_open_error()
	var ok: bool = file.store_string(text)
	file.close()
	if not ok:
		return ERR_FILE_CANT_WRITE
	var dir: DirAccess = DirAccess.open("user://")
	if dir == null:
		return DirAccess.get_open_error()
	if dir.file_exists(SAVE_NAME):
		dir.copy(SAVE_NAME, SAVE_NAME + ".bak")
	return dir.rename(SAVE_NAME + ".tmp", SAVE_NAME)
```

`DirAccess.rename()` overwrites an existing target. `OS.set_use_file_access_save_and_swap()`
makes every write-mode `FileAccess` do the temp-and-swap by itself. On load, fall back to `.bak`
when the main file fails to parse. Check `DirAccess.get_space_left()` before large writes.

### Version and migrate every save
Store a format version at the top. On load run a chain of pure functions (`v1 -> v2 -> v3`)
on the raw dictionary before any game object sees it, then validate. Rules that keep old
saves alive: never reuse a key with a new meaning, give every new field a default, store
stable ids, never node paths, array indexes, instance ids or RIDs. Keep one real save per
shipped version as a test fixture and load them all in CI.

### JSON without type loss
Plain JSON turns ints into floats and cannot hold `Vector2`, `Color` and similar types.
`JSON.from_native()` wraps typed values so `JSON.to_native()` restores them exactly, still
as plain JSON text that diffs and merges well in cloud storage.

```gdscript
var state: Dictionary = {"pos": Vector2i(3, 4), "tint": Color.RED, "hp": 7}
var text: String = JSON.stringify(JSON.from_native(state), "\t")
var back: Variant = JSON.to_native(JSON.parse_string(text))
```

With plain JSON, use `JSON.new().parse()` to get `get_error_line()` and
`get_error_message()`; `parse_string()` only returns null. The parser is lenient (it
accepts trailing commas), so do not use it as a validator.

### Compact binary saves
`FileAccess.open_compressed()` with `COMPRESSION_ZSTD` plus `store_var()` gives small files
with full types. For a custom layout use `store_32()`, `store_float()` and
`store_buffer()`. Trap:
`open_compressed()` reads only files written by Godot, not gzip or zstd files from other
tools. `PackedByteArray.compress()` and `decompress_dynamic()` handle standard buffers.

### Encryption: what it buys
`FileAccess.open_encrypted()` takes a 32-byte key; `open_encrypted_with_pass()` and
`ConfigFile.save_encrypted_pass()` take a password (AES-256). Any key shipped
in the client can be extracted, so this stops casual editing only. To detect tampering add
a MAC (`HMACContext`) or use a server. `FileAccess.get_sha256()` is a checksum against
corruption, not against an attacker who can recompute it.

### Background loading without stalls
`ResourceLoader.load_threaded_request(path, "", use_sub_threads)` starts a load;
`load_threaded_get_status(path, progress)` polls it and fills a one-element progress array;
`load_threaded_get()` returns the resource and blocks if the load is not done, so call it
only on `THREAD_LOAD_LOADED`. `use_sub_threads` loads faster with more threads and can slow
the main thread. Instancing still runs on the calling thread: a big scene loads in the
background and then hitches on `instantiate()`. Split such scenes or add them over frames.

### Cache modes for reload and mods
`ResourceLoader.load(path, "", cache_mode)`: `CACHE_MODE_REUSE` (default) returns the shared
cached instance, so mutating it mutates every user. `CACHE_MODE_IGNORE` gives a private
copy of the main resource; `CACHE_MODE_IGNORE_DEEP` also covers external dependencies.
`CACHE_MODE_REPLACE` refreshes cached instances in place for hot reload of data files.

### Runtime loading of loose files
No import step is needed for: `Image.load_from_file()` (or `load_png_from_buffer()` and
siblings for error handling), `AudioStreamOggVorbis.load_from_file()`,
`AudioStreamWAV.load_from_file()`, `GLTFDocument.append_from_file()` then `generate_scene()`
(FBX through `FBXDocument`), `FontFile.load_dynamic_font()`, and `ZIPReader` / `ZIPPacker`.
Use this for user content only: project assets loaded this way skip import and remaps. Call `Image.generate_mipmaps()` for 3D use. Set
`GLTFState.base_path` when loading glTF from a buffer. `.glb` is faster and smaller than
`.gltf`. An `.ogg` file may hold Theora or Opus, which the audio loader rejects.

### PCK and ZIP packs
`ProjectSettings.load_resource_pack()` mounts a PCK or ZIP over `res://`; later packs
replace files with the same path unless `replace_files` is false. That is the DLC and patch
path. A pack can carry scripts, so a pack from an unknown source is code execution. For
launcher patches the docs advise signing packs with a private key and checking with a
public key in the main pack (`Crypto.verify()`).

### Save cost on slow storage
Serialize on the main thread into a `String` or `PackedByteArray` (fast), then hand the
write to `WorkerThreadPool` so slow flash or IndexedDB does not stall a frame. Coalesce
autosaves: mark dirty and write at checkpoints, on pause and on
`NOTIFICATION_WM_CLOSE_REQUEST` or `NOTIFICATION_APPLICATION_PAUSED` (mobile may kill the
app right after). Keep saves small and split rarely changing data (settings, unlocks) from
the run state, so cloud sync moves less.

## Niche but useful

- `FileAccess.create_temp()` and `DirAccess.create_temp()` (since 4.4) give scratch files
  that delete themselves.
- `ConfigFile` holds any Variant per key and round-trips types. Good for settings. It is
  Variant text: do not parse a `.cfg` from an untrusted source.
- `ProjectSettings.globalize_path()` turns `user://` into an OS path for `OS.shell_open()`.
- `Logger` with `OS.add_logger()` (since 4.5) receives every message and error for your own
  file or telemetry. `_log_message()` is called from any thread: guard it with a `Mutex`.
- Built-in file logging: `debug/file_logging/enable_file_logging` (on for desktop only),
  `log_path` (`user://logs/godot.log`), `max_log_files` (5). Release builds flush stdout
  only at exit unless `application/run/flush_stdout_on_print` is set; stderr always flushes.
- `debug/settings/gdscript/always_track_call_stacks` adds script backtraces to release logs.
- `FileAccess.set_extended_attribute()` (since 4.6) tags files on desktop file systems.

## Traps

- `eof_reached()` turns true only after a read past the end. Loop on
  `get_position() < get_length()`.
- `FileAccess.WRITE` fails if the folder is missing. Call `DirAccess.make_dir_recursive()`.
- `FileAccess.open()` returns null on failure; `get_open_error()` is per thread.
- Outdated belief: "encrypted saves are tamper-proof". The key is in the client.
- Outdated belief: "custom Resources are the safe default for saves". See the trust rule.

## Version notes

- 4.4: all `FileAccess.store_*` methods return `bool`; `JSON.from_native()` and
  `to_native()`; temp file and directory helpers.
- 4.5: `Logger` class; script backtraces in logs.
- 4.6: `FileAccess.create_temp()` takes a `FileAccess.ModeFlags` value; extended attributes.
- 4.7: `ZIPPacker.start_file()` gained optional `permissions` and `modified_time`.

## Related skills

- `godot-save-load` - step-by-step save manager, autosave timing, rolling backups.
- `godot-save-security` - threat model, HMAC envelope, signed manifests, key handling.
- `godot-feature-seams-and-saves` - keeping saves valid while content changes.
- `godot-multithreading` - worker pool rules for background writes.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/io/data_paths.html
- https://docs.godotengine.org/en/stable/tutorials/io/saving_games.html
- https://docs.godotengine.org/en/stable/tutorials/io/binary_serialization_api.html
- https://docs.godotengine.org/en/stable/tutorials/io/runtime_file_loading_and_saving.html
- https://docs.godotengine.org/en/stable/tutorials/io/background_loading.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/logging.html
- https://docs.godotengine.org/en/stable/tutorials/export/exporting_pcks.html
- https://docs.godotengine.org/en/stable/tutorials/export/exporting_for_web.html
- https://www.gdquest.com/library/save_game_godot4 (search summary)
- https://github.com/derkork/godot-safe-resource-loader (search summary)
