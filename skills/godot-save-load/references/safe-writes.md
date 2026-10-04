Adds crash-safe writes (temp file and rename), a rolling backup, a checksum with fallback, encrypted saves, and when to save. Read it before you ship a save system, because a crash during a write otherwise destroys the only save.

> ← Back to [SKILL.md](../SKILL.md)

# Crash-Safe Writes, Backups and Integrity

## The failure

`FileAccess.open(path, FileAccess.WRITE)` truncates the file at once. If the game crashes, the battery dies or the process is killed before the write ends, the save is empty or half written. The player loses all progress, not only the last session.

## Write to a temp file, then rename

Write the new save next to the old one, then swap it in with one rename. A rename in the same directory replaces the target in one step on desktop file systems. On Godot 4.7.2 (macOS), `DirAccess.rename_absolute()` replaces an existing target and returns `OK`.

```gdscript
# safe_save_writer.gd
class_name SafeSaveWriter
extends RefCounted


## Writes text to `path` so that a crash leaves either the old file or the new one.
## Keeps the previous save as `path + ".bak"`.
static func write_text(path: String, text: String) -> Error:
    var tmp := path + ".tmp"
    var bak := path + ".bak"
    var file := FileAccess.open(tmp, FileAccess.WRITE)
    if file == null:
        return FileAccess.get_open_error()
    var ok := file.store_string(text)
    file.flush()
    file.close()
    if not ok:
        DirAccess.remove_absolute(tmp)
        return ERR_FILE_CANT_WRITE
    if FileAccess.file_exists(path):
        var copy_err := DirAccess.copy_absolute(path, bak)
        if copy_err != OK:
            push_warning("SafeSaveWriter: no backup for %s (%s)" % [path, error_string(copy_err)])
    var err := DirAccess.rename_absolute(tmp, path)
    if err != OK:
        push_error("SafeSaveWriter: rename %s -> %s failed (%s)" % [tmp, path, error_string(err)])
    return err
```

- Since 4.4, `store_string()` and the other `FileAccess.store_*` methods return `bool` (GH-78289; checked by unit GW10 against the 4.7.2 doctool dump and godot-docs 4.7). Check every write that matters: a full disk fails here.
- Keep `.tmp`, `.bak` and the save in the same directory. A rename across directories (or volumes) is a copy, not one step.
- On start-up, delete a stale `.tmp` file. It is the trace of a write that did not finish.
- Web exports store `user://` in IndexedDB, and console SDKs have their own save APIs. Test the rename on each target platform; do not assume desktop behavior.

## Checksum and fallback

A checksum detects a file that is truncated or edited. Store the hash of the payload inside the file, then verify it on load. When the check fails, try the backup.

```gdscript
# checked_save.gd
class_name CheckedSave
extends RefCounted


static func encode(state: Dictionary) -> String:
    var body := JSON.stringify(state)
    return JSON.stringify({"sha256": body.sha256_text(), "body": body})


## Returns the state, or null when the text is damaged.
static func decode(text: String) -> Variant:
    var outer: Variant = JSON.parse_string(text)
    if typeof(outer) != TYPE_DICTIONARY:
        return null
    var body: String = str(outer.get("body", ""))
    if body.is_empty() or body.sha256_text() != str(outer.get("sha256", "")):
        return null
    return JSON.parse_string(body)


static func load_with_fallback(path: String) -> Variant:
    for candidate: String in [path, path + ".bak"]:
        if not FileAccess.file_exists(candidate):
            continue
        var state: Variant = decode(FileAccess.get_file_as_string(candidate))
        if state != null:
            if candidate != path:
                push_warning("CheckedSave: %s was damaged; loaded the backup" % path)
            return state
    return null
```

- A checksum is not tamper protection. A player who edits the body can recompute the hash. It protects against damage, not against cheating.
- `FileAccess.get_sha256(path)` hashes a whole file. Use it when you keep the hash in a separate index file, for example a cloud-sync manifest.
- Tell the player when the backup was loaded. Silent rollback looks like a bug.

## Encrypted saves

`FileAccess.open_encrypted_with_pass(path, mode, pass)` encrypts the file. A wrong password makes `open_encrypted_with_pass()` return `null` and `FileAccess.get_open_error()` return `ERR_FILE_CORRUPT` (16), with an engine error in the log.

```gdscript
# encrypted_save.gd
class_name EncryptedSave
extends RefCounted


static func write(path: String, pass_phrase: String, state: Dictionary) -> Error:
    var file := FileAccess.open_encrypted_with_pass(path, FileAccess.WRITE, pass_phrase)
    if file == null:
        return FileAccess.get_open_error()
    return OK if file.store_var(state, false) else ERR_FILE_CANT_WRITE


static func read(path: String, pass_phrase: String) -> Variant:
    var file := FileAccess.open_encrypted_with_pass(path, FileAccess.READ, pass_phrase)
    if file == null:
        return null
    return file.get_var(false)
```

- **The key ships inside the game.** A pass phrase in a script or a constant is in the exported `.pck`, so encryption only stops casual editing. Say this in the design; do not call it security.
- Do not build your own cipher with `AESContext` in ECB mode. ECB shows patterns in the data and gives no integrity check. If you need real protection of server-trusted data (ranked scores, purchases), the server must own that data.
- Combine with the temp-and-rename writer: open the encrypted file on the `.tmp` path, then rename.

## When to save

- Save on explicit events: a checkpoint, a level end, a menu open, the quit request. Never save each physics frame.
- An autosave is a `Timer` (for example 300 s) that calls the same function as a manual save. Skip the tick during combat or a cutscene, and save at the next safe moment.
- Handle the quit request: set `get_tree().auto_accept_quit = false`, save in `_notification(NOTIFICATION_WM_CLOSE_REQUEST)`, then call `get_tree().quit()`. On Android, also save on `NOTIFICATION_APPLICATION_PAUSED`, because the OS can kill a paused app without another callback.
- Never save while a write is still running. Keep one `_saving` flag in the save manager and skip or queue the second request.

## Identity across sessions

- `get_instance_id()` changes each run. Never save it.
- A placed object needs a stable id from the editor (an exported `StringName` set once) or from generation (seed plus index). See the `SaveableComponent` pattern in [save-architecture.md](save-architecture.md).
