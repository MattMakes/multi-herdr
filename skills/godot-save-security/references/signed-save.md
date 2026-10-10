# Signed save: the HMAC envelope, the store and the key table

> Back to [SKILL.md](../SKILL.md).

This file gives 3 classes and 1 build tool. Use them when the threat model
asks for a tamper-evident save. They sit under the save system of
`godot-save-load`: the game gives a body `Dictionary` to `SaveStore.write`
and gets one back from `SaveStore.read`.

> proof: headless-run: Godot 4.7.2 ran these classes in a scratch project
> with 41 checks: seal and open, an edited body, a flipped flag, an empty
> and a broken file, an unknown key id, a newer version, 3 writes with 2
> backups, recovery from `.bak1` after an edit and after an empty or a
> truncated file, the quarantine copy, `LOAD_MARKED` and its sticky flag, a
> failed load, a read-only newer save, an old key sealed again, a rollback
> flag, and a save copied to another install or account.

## 1. The envelope on disk

```json
{"body":"{\"coins\":3.0}","counter":7,"format":"game-save","kid":2,"mac":"Vb6w...=","modified":false,"v":1}
```

- The body is a JSON **string** inside the envelope. The tag covers its
  exact bytes, so a change in key order or float format cannot break an
  honest save.
- The tag covers the format, the envelope version, the key id, the
  counter, the `modified` flag and the body length. A player cannot clear
  the flag without the key.
- The body stays readable. That helps support and playtests. If the save
  must be secret, see encrypt-then-MAC in [techniques.md](techniques.md).

## 2. The key table

```gdscript
# save_keys.gd
class_name SaveKeys
extends RefCounted
## Builds the HMAC key table: key id -> 32-byte key.
## The build secret is split in 2 parts, so a plain string search of the
## binary does not show it. This raises the cost. It does not stop a player
## with a debugger or a decompiler.

const INSTALL_ID_PATH := "user://install.id"
## key id -> 2 parts. The key is part 0 XOR part 1. Make new parts for each
## key id with make_key_parts.gd. Keep old ids, so old saves still verify.
## These values are placeholders: never ship them.
const PARTS := {
	1: [[0x11, 0x22, 0x33, 0x44], [0x55, 0x66, 0x77, 0x88]],
	2: [[0x9a, 0xbc, 0xde, 0xf0], [0x0f, 0xed, 0xcb, 0xa9]],
}


## account_id binds the save to a platform account (for example a Steam ID).
## An empty account_id binds the save to this install only.
static func build_table(account_id: String = "") -> Dictionary[int, PackedByteArray]:
	var bind := _install_id() if account_id.is_empty() else account_id.to_utf8_buffer()
	var table: Dictionary[int, PackedByteArray] = {}
	var crypto := Crypto.new()
	for key_id: int in PARTS:
		var label := ("save-key|%d|" % key_id).to_utf8_buffer()
		label.append_array(bind)
		table[key_id] = crypto.hmac_digest(HashingContext.HASH_SHA256, _join(PARTS[key_id]), label)
	return table


static func _join(parts: Array) -> PackedByteArray:
	var first: Array = parts[0]
	var second: Array = parts[1]
	var out := PackedByteArray()
	for index in first.size():
		out.append(int(first[index]) ^ int(second[index]))
	return out


## A random id for this install. A copied save fails the check on another
## install. The id sits next to the save, so it does not stop a local edit.
static func _install_id() -> PackedByteArray:
	if FileAccess.file_exists(INSTALL_ID_PATH):
		var stored := FileAccess.get_file_as_bytes(INSTALL_ID_PATH)
		if stored.size() == 16:
			return stored
	var fresh := Crypto.new().generate_random_bytes(16)
	var file := FileAccess.open(INSTALL_ID_PATH, FileAccess.WRITE)
	if file != null:
		file.store_buffer(fresh)
	return fresh
```

- Make the parts with the build tool in section 5. Keep them out of public
  repositories.
- Pass the platform account id when the game has one. The save then moves
  with the account through cloud sync, and a save from another account
  fails the check.
- The install id file is the binding. If the player deletes it, every save
  fails the check. With `LOAD_MARKED`, the save loads and is marked.

## 3. The envelope

```gdscript
# signed_save.gd
class_name SignedSave
extends RefCounted
## A tamper-evident envelope: a JSON body plus a key id, a counter, a
## "modified" flag and an HMAC-SHA256 tag. It detects an edit by a person
## without the key. It does not hide the data, and it does not stop a player
## who extracts the key.

enum Status { OK, OK_OLD_KEY, MISSING, CORRUPT, BAD_MAC, UNKNOWN_KEY, TOO_NEW }

const FORMAT := "game-save"
const ENVELOPE_VERSION := 1
## New saves use this key id. Old ids stay in `keys`, so old saves verify.
const CURRENT_KEY_ID := 2

## key id -> 32-byte key, from SaveKeys.build_table().
var keys: Dictionary[int, PackedByteArray] = {}
var _crypto := Crypto.new()


## The result of one open() call.
class ReadResult extends RefCounted:
	var status: Status = Status.MISSING
	## The parsed body. It is set for OK, OK_OLD_KEY and, when the body
	## parses, for BAD_MAC and UNKNOWN_KEY too. Never trust it without a check.
	var body: Dictionary = {}
	var has_body: bool = false
	var counter: int = 0
	var modified: bool = false


func seal(body: Dictionary, counter: int, modified: bool) -> String:
	var body_text := JSON.stringify(body)
	var envelope := {
		"format": FORMAT,
		"v": ENVELOPE_VERSION,
		"kid": CURRENT_KEY_ID,
		"counter": counter,
		"modified": modified,
		"body": body_text,
	}
	var tag := _mac(keys[CURRENT_KEY_ID], CURRENT_KEY_ID, counter, modified, body_text)
	envelope["mac"] = Marshalls.raw_to_base64(tag)
	return JSON.stringify(envelope)


func open(text: String) -> ReadResult:
	var result := ReadResult.new()
	if text.is_empty():
		result.status = Status.CORRUPT
		return result
	var json := JSON.new()
	if json.parse(text) != OK or typeof(json.data) != TYPE_DICTIONARY:
		result.status = Status.CORRUPT
		return result
	var envelope: Dictionary = json.data
	if envelope.get("format") != FORMAT or typeof(envelope.get("body")) != TYPE_STRING \
			or typeof(envelope.get("mac")) != TYPE_STRING:
		result.status = Status.CORRUPT
		return result
	if int(envelope.get("v", 0)) > ENVELOPE_VERSION:
		result.status = Status.TOO_NEW
		return result
	var body_text: String = envelope["body"]
	var body_json := JSON.new()
	if body_json.parse(body_text) == OK and typeof(body_json.data) == TYPE_DICTIONARY:
		result.body = body_json.data
		result.has_body = true
	result.counter = int(envelope.get("counter", 0))
	result.modified = envelope.get("modified", false) == true
	var key_id := int(envelope.get("kid", 0))
	if not keys.has(key_id):
		result.status = Status.UNKNOWN_KEY
		return result
	var expected := _mac(keys[key_id], key_id, result.counter, result.modified, body_text)
	var received := Marshalls.base64_to_raw(envelope["mac"])
	if not _crypto.constant_time_compare(expected, received):
		result.status = Status.BAD_MAC
		return result
	if not result.has_body:
		result.status = Status.CORRUPT
		return result
	result.status = Status.OK if key_id == CURRENT_KEY_ID else Status.OK_OLD_KEY
	return result


static func is_good(status: Status) -> bool:
	return status == Status.OK or status == Status.OK_OLD_KEY


## The tag covers the format, the envelope version, the key id, the counter,
## the modified flag and the exact body bytes. The body length prefix stops
## 2 fields from running into each other.
func _mac(key: PackedByteArray, key_id: int, counter: int, modified: bool, body_text: String) -> PackedByteArray:
	var body_bytes := body_text.to_utf8_buffer()
	var header := "%s|%d|%d|%d|%d|%d|" % [FORMAT, ENVELOPE_VERSION, key_id, counter, int(modified), body_bytes.size()]
	var message := header.to_utf8_buffer()
	message.append_array(body_bytes)
	return _crypto.hmac_digest(HashingContext.HASH_SHA256, key, message)
```

## 4. The store: atomic write, backups, quarantine, recovery

```gdscript
# save_store.gd
class_name SaveStore
extends RefCounted
## Atomic write, rotated backups, verify-on-load, quarantine and recovery.
## A file that fails the check is copied to user://quarantine/. The store
## never deletes it before the copy.

## What the game does with a save that fails the tag check but parses.
## REFUSE: try the backups. LOAD_MARKED: load it and set the sticky
## "modified" flag. The designer chooses; LOAD_MARKED suits single-player.
enum MacPolicy { REFUSE, LOAD_MARKED }
enum Outcome { OK, NEW, RECOVERED, MODIFIED, TOO_NEW, FAILED }

const BACKUP_COUNT := 2
const QUARANTINE_DIR := "user://quarantine"

var path: String
var sealer: SignedSave
var policy: MacPolicy = MacPolicy.LOAD_MARKED
## True after a load that marked the save. Every later seal keeps it true.
var modified: bool = false
## True after TOO_NEW. write() refuses, so an old game never overwrites a
## save from a newer game.
var read_only: bool = false
var _last_counter: int = 0


## The result of one read().
class LoadResult extends RefCounted:
	var outcome: Outcome = Outcome.NEW
	var body: Dictionary = {}
	## The status of the main file, for the log and the support message.
	var reason: String = ""
	## The backup that loaded, for the player message. Empty if none.
	var backup_path: String = ""
	## A counter below the last one seen: an older save came back.
	var rolled_back: bool = false


func _init(save_path: String, signed_save: SignedSave) -> void:
	path = save_path
	sealer = signed_save
	_last_counter = _read_counter()


func write(body: Dictionary) -> Error:
	if read_only:
		return ERR_LOCKED
	var counter := _last_counter + 1
	var temp_path := path + ".tmp"
	var file := FileAccess.open(temp_path, FileAccess.WRITE)
	if file == null:
		return FileAccess.get_open_error()
	var stored := file.store_string(sealer.seal(body, counter, modified))
	file.close()
	if not stored:
		DirAccess.remove_absolute(temp_path)
		return ERR_FILE_CANT_WRITE
	_rotate_backups()
	var err := _replace(temp_path, path)
	if err != OK:
		return err
	_last_counter = counter
	_write_counter(counter)
	return OK


func read() -> LoadResult:
	var result := LoadResult.new()
	var main := _read_one(path)
	result.reason = SignedSave.Status.keys()[main.status]
	if main.status == SignedSave.Status.MISSING:
		return _from_backups(result, Outcome.NEW)
	if main.status == SignedSave.Status.TOO_NEW:
		read_only = true
		result.outcome = Outcome.TOO_NEW
		return result
	if SignedSave.is_good(main.status):
		modified = main.modified
		_accept(main, result)
		result.outcome = Outcome.MODIFIED if modified else Outcome.OK
		if main.status == SignedSave.Status.OK_OLD_KEY:
			write(result.body)  # Seal again under the current key.
		return result
	_quarantine(path)
	var tag_failed := main.status == SignedSave.Status.BAD_MAC or main.status == SignedSave.Status.UNKNOWN_KEY
	if tag_failed and main.has_body and policy == MacPolicy.LOAD_MARKED:
		modified = true
		_accept(main, result)
		result.outcome = Outcome.MODIFIED
		write(result.body)  # Seal the marked save, so the next load is clean.
		return result
	return _from_backups(result, Outcome.FAILED)


func _from_backups(result: LoadResult, outcome_without_backup: Outcome) -> LoadResult:
	for index in range(1, BACKUP_COUNT + 1):
		var backup_path := "%s.bak%d" % [path, index]
		var backup := _read_one(backup_path)
		if SignedSave.is_good(backup.status):
			modified = backup.modified
			result.backup_path = backup_path
			_accept(backup, result)
			result.outcome = Outcome.RECOVERED
			return result
	result.outcome = outcome_without_backup
	return result


func _read_one(file_path: String) -> SignedSave.ReadResult:
	if not FileAccess.file_exists(file_path):
		return SignedSave.ReadResult.new()
	return sealer.open(FileAccess.get_file_as_string(file_path))


## Flag a lower counter. Do not refuse it: a cloud conflict or a restored
## backup also gives a lower counter.
func _accept(found: SignedSave.ReadResult, result: LoadResult) -> void:
	result.body = found.body
	result.rolled_back = found.counter < _last_counter and result.backup_path.is_empty()
	_last_counter = maxi(_last_counter, found.counter)


## Copy only verified files into the backups, so a bad file never pushes a
## good backup out.
func _rotate_backups() -> void:
	for index in range(BACKUP_COUNT, 0, -1):
		var source := path if index == 1 else "%s.bak%d" % [path, index - 1]
		if not SignedSave.is_good(_read_one(source).status):
			continue
		var target := "%s.bak%d" % [path, index]
		DirAccess.remove_absolute(target)
		DirAccess.copy_absolute(source, target)


## A rename over an existing file fails on some platforms. Then move the old
## file aside, rename, and put the old file back if the rename fails again.
static func _replace(from_path: String, to_path: String) -> Error:
	var err := DirAccess.rename_absolute(from_path, to_path)
	if err == OK or not FileAccess.file_exists(to_path):
		return err
	var aside := to_path + ".old"
	DirAccess.remove_absolute(aside)
	if DirAccess.rename_absolute(to_path, aside) != OK:
		return err
	err = DirAccess.rename_absolute(from_path, to_path)
	if err != OK:
		DirAccess.rename_absolute(aside, to_path)
		return err
	DirAccess.remove_absolute(aside)
	return OK


func _quarantine(file_path: String) -> void:
	DirAccess.make_dir_recursive_absolute(QUARANTINE_DIR)
	var stamp := Time.get_datetime_string_from_system().replace(":", "-")
	var target := "%s/%s.%s" % [QUARANTINE_DIR, file_path.get_file(), stamp]
	DirAccess.copy_absolute(file_path, target)


## The highest counter seen, in a second file. The player can restore both
## files, so this detects a casual rollback only. A server makes it strong.
func _read_counter() -> int:
	var counter_path := path + ".counter"
	if not FileAccess.file_exists(counter_path):
		return 0
	return FileAccess.get_file_as_string(counter_path).to_int()


func _write_counter(counter: int) -> void:
	var file := FileAccess.open(path + ".counter", FileAccess.WRITE)
	if file != null:
		file.store_string(str(counter))
```

Behavior by status of the main file:

| Main file | `REFUSE` | `LOAD_MARKED` |
| --- | --- | --- |
| Good tag, current key | `OK` (or `MODIFIED` if already marked) | same |
| Good tag, old key | `OK`, sealed again under the current key | same |
| Missing | First good backup (`RECOVERED`), else `NEW` | same |
| Bad tag or unknown key, body parses | Quarantine, then backups, else `FAILED` | Quarantine, load it, `MODIFIED`, seal it with the flag |
| Broken JSON or empty | Quarantine, then backups, else `FAILED` | same |
| Newer envelope version | `TOO_NEW`; `write` returns `ERR_LOCKED` | same |

- `rolled_back` is true when the main file has a counter below the last
  counter this install saw. Show nothing to the player for it. Use it to
  keep the run off shared boards.
- The counter file sits next to the save, so a player can put back both.
  It detects a casual rollback only.
- `_replace` handles a platform that refuses a rename over an existing
  file. proof: not run (needs Windows). Test it on each export target.

## 5. Build tool: new key parts

```gdscript
# make_key_parts.gd
extends SceneTree
## Build tool, never shipped: prints 2 random 32-byte parts for one new key id.
## Paste the output into SaveKeys.PARTS under the next free key id.
## Run: godot --headless -s make_key_parts.gd


func _init() -> void:
	var crypto := Crypto.new()
	var parts: Array[String] = []
	for index in 2:
		var bytes := crypto.generate_random_bytes(32)
		var numbers: Array[String] = []
		for value in bytes:
			numbers.append("0x%02x" % value)
		parts.append("[%s]" % ", ".join(numbers))
	print("[%s]," % ", ".join(parts))
	quit(0)
```

Run it once per new key id. Add the output to `SaveKeys.PARTS` under the
next id, then set `SignedSave.CURRENT_KEY_ID` to that id. Keep the old ids.

## 6. Wire it into the save manager

```gdscript
# save_manager.gd (an autoload)
extends Node

signal save_recovered(backup_path: String, reason: String)
signal save_unreadable(reason: String)
signal save_too_new

var _store: SaveStore


func _ready() -> void:
	var sealer := SignedSave.new()
	sealer.keys = SaveKeys.build_table()
	_store = SaveStore.new("user://save.json", sealer)
	_store.policy = SaveStore.MacPolicy.LOAD_MARKED


func load_game() -> Dictionary:
	var result := _store.read()
	match result.outcome:
		SaveStore.Outcome.RECOVERED:
			save_recovered.emit(result.backup_path, result.reason)
		SaveStore.Outcome.FAILED:
			save_unreadable.emit(result.reason)
		SaveStore.Outcome.TOO_NEW:
			save_too_new.emit()
	return result.body


func save_game(body: Dictionary) -> bool:
	return _store.write(body) == OK
```

- The game still checks every field of the body with `SaveFields`
  ([validation-and-recovery.md](validation-and-recovery.md)). A good tag
  proves that the game wrote the data, not that the data is sane: a bug in
  an old version can write a bad value with a good tag.
- `_store.modified` is the profile mark. Show it in the profile screen if
  the designer wants that, and keep marked profiles off shared boards.
- [validation-and-recovery.md](validation-and-recovery.md) gives the text
  for each player message.

> proof: headless-run: this autoload saved twice, read a broken main file,
> returned the body of `.bak1` and emitted `save_recovered` with the reason
> `CORRUPT`.
