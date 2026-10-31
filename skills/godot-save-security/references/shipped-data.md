# Shipped data: signed manifests, packs, mods and export settings

> Back to [SKILL.md](../SKILL.md).

Data that the developer makes and the game only reads gets a **signature**,
not an HMAC. The game ships only the public key, so the game cannot make a
valid signature, even after full reverse engineering.

## 1. What a signature protects, and what it does not

- **Full value**: content that arrives **after** install. Patches, DLC,
  downloaded balance tables, mod allow-lists, server responses.
- **Speed bump only**: data in the game's own main PCK. A player who
  replaces the main PCK also replaces the check code and the public key.
- **Single-player balance data** needs no protection. An edit harms only
  that player and helps modders. Keep it as plain JSON.

## 2. The manifest format

```json
{"files":{"balance.json":"6db8a363...","extra.pck":"c7d80162..."},"version":1}
```

- `files` maps a path relative to the data folder to the SHA-256 hex of the
  file.
- `version` goes up by 1 for each release. The game stores the last
  version it accepted and passes it as `min_version`, so an older manifest
  is refused (anti-rollback).
- `manifest.sig` is an RSA PKCS#1 v1.5 signature over the SHA-256 of the
  exact bytes of `manifest.json`.

## 3. Verify in the game

```gdscript
# manifest_check.gd
class_name ManifestCheck
extends RefCounted
## Verifies a signed content manifest. manifest.json lists a SHA-256 per
## file; manifest.sig is an RSA signature over the SHA-256 of manifest.json.
## The game ships the public key only, so the game cannot make a new valid
## manifest.


static func sha256_file(file_path: String) -> PackedByteArray:
	var file := FileAccess.open(file_path, FileAccess.READ)
	if file == null:
		return PackedByteArray()
	var context := HashingContext.new()
	context.start(HashingContext.HASH_SHA256)
	while file.get_position() < file.get_length():
		context.update(file.get_buffer(65536))
	return context.finish()


## Returns the manifest's file table, or an empty Dictionary when the
## signature or the format fails, or when the version is below min_version
## (an old, valid manifest served again).
static func verified_files(manifest_path: String, signature_path: String, public_pem: String, min_version: int = 0) -> Dictionary:
	var key := CryptoKey.new()
	if key.load_from_string(public_pem, true) != OK:
		return {}
	var manifest_bytes := FileAccess.get_file_as_bytes(manifest_path)
	var signature := FileAccess.get_file_as_bytes(signature_path)
	if manifest_bytes.is_empty() or signature.is_empty():
		return {}
	var context := HashingContext.new()
	context.start(HashingContext.HASH_SHA256)
	context.update(manifest_bytes)
	if not Crypto.new().verify(HashingContext.HASH_SHA256, context.finish(), signature, key):
		return {}
	var manifest: Variant = JSON.parse_string(manifest_bytes.get_string_from_utf8())
	if typeof(manifest) != TYPE_DICTIONARY or typeof(manifest.get("files")) != TYPE_DICTIONARY:
		return {}
	if int(manifest.get("version", 0)) < min_version:
		return {}
	return manifest["files"]


## Returns the paths that fail. An empty list means every listed file matches.
## base_dir is where the listed relative paths live.
static func verify(manifest_path: String, signature_path: String, public_pem: String, base_dir: String) -> PackedStringArray:
	var failures := PackedStringArray()
	var files := verified_files(manifest_path, signature_path, public_pem)
	if files.is_empty():
		failures.append(manifest_path)
		return failures
	for relative: Variant in files:
		var full := base_dir.path_join(str(relative))
		if sha256_file(full).hex_encode() != str(files[relative]):
			failures.append(full)
	return failures


## Mounts a pack only if its hash is in a verified manifest. replace_files is
## false, so the pack cannot override files of the base game.
static func mount_verified_pack(pack_path: String, files: Dictionary) -> bool:
	var expected: Variant = files.get(pack_path.get_file())
	if typeof(expected) != TYPE_STRING or sha256_file(pack_path).hex_encode() != expected:
		return false
	return ProjectSettings.load_resource_pack(pack_path, false)
```

> proof: headless-run: on Godot 4.7.2, with an RSA-2048 key from
> `Crypto.generate_rsa` and with an RSA-3072 key from the OpenSSL commands
> in section 4: a clean manifest verifies, a pack with a matching hash
> mounts and its files show under `res://`, `min_version` refuses an older
> manifest, a different public key fails, an edited pack is refused, an
> edited data file fails, and an edited manifest fails the signature.

- Ship the public key as a PEM text file in `res://` or as a constant.
  It is not a secret.
- `mount_verified_pack` uses `load_resource_pack(path, false)`. With
  `replace_files = true` (the default), a mounted pack **overrides** files
  of the base game, scripts too. That gives the pack full code execution.
- Verify a pack before you mount it. After the mount it is too late.

## 4. Sign at build time

```gdscript
# sign_manifest.gd
extends SceneTree
## Build step, never shipped: hashes the files in a folder, writes
## manifest.json and signs it.
## Run: godot --headless -s sign_manifest.gd -- <data_dir> <private.pem>


func _init() -> void:
	var args := OS.get_cmdline_user_args()
	if args.size() != 2:
		printerr("usage: -- <data_dir> <private.pem>")
		quit(2)
		return
	var data_dir := args[0]
	var key := CryptoKey.new()
	if key.load(args[1]) != OK or key.is_public_only():
		printerr("cannot load the private key")
		quit(1)
		return
	var files := {}
	for name: String in DirAccess.get_files_at(data_dir):
		if name == "manifest.json" or name == "manifest.sig":
			continue
		files[name] = ManifestCheck.sha256_file(data_dir.path_join(name)).hex_encode()
	var manifest_bytes := JSON.stringify({"version": 1, "files": files}, "", true).to_utf8_buffer()
	var context := HashingContext.new()
	context.start(HashingContext.HASH_SHA256)
	context.update(manifest_bytes)
	var signature := Crypto.new().sign(HashingContext.HASH_SHA256, context.finish(), key)
	_write(data_dir.path_join("manifest.json"), manifest_bytes)
	_write(data_dir.path_join("manifest.sig"), signature)
	print("signed %d files" % files.size())
	quit(0)


func _write(file_path: String, bytes: PackedByteArray) -> void:
	var file := FileAccess.open(file_path, FileAccess.WRITE)
	file.store_buffer(bytes)
```

> proof: headless-run: the script signed 2 files on Godot 4.7.2, and
> `openssl dgst -sha256 -verify public.pem -signature manifest.sig
> manifest.json` printed `Verified OK`. OpenSSL `-sign` with the same key
> gave the same bytes.

Make the key pair once, on a machine that is not the build server:

```sh
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:3072 -out content_private.pem
openssl pkey -in content_private.pem -pubout -out content_public.pem
```

- The private key never goes into the repository, the game, or a CI log.
  Keep it offline or in the CI secret store, and sign in a separate step.
- `JSON.stringify(data, "", true)` sorts the keys, so the same files give
  the same manifest bytes.
- The signer lists the files of 1 folder. Extend it for subfolders in your
  build.

## 5. Update manifests (reduced TUF)

The Update Framework (TUF) uses 4 roles with separate keys: root, targets,
snapshot and timestamp. It defends against rollback, freeze and
mix-and-match attacks, even without TLS. A small game uses a reduced form:

1. 1 signed manifest with a `version`, an `expires` time and SHA-256
   hashes.
2. Refuse a manifest with a lower `version` than the last one accepted.
3. Refuse a manifest after its `expires` time (freeze attack: an old valid
   manifest served forever).
4. Use the store's own update system (for example Steam depots) where it
   exists. It already signs and checks content.

## 6. Mods

1. Find the pack. Do not mount it yet.
2. Check its hash against a signed allow-list, or ask the player.
3. Mount with `load_resource_pack(path, false)`.
4. Do not block unsigned mods in single-player. Mark the profile as
   "modded", show the list of loaded mods, and keep modded runs off shared
   boards.

## 7. `res://`, `user://` and imported files

- `res://` is read-only in an export. A player changes it by a replaced
  PCK, a patch PCK, or `--main-pack` on the command line.
- `.import` and `.remap` files map a source path to an imported resource.
  A replaced PCK can point them at other resources. They need no separate
  defence: protect the PCK as a whole, or not at all.
- `user://` is fully under the player's control. On the Web export it is
  the browser's IndexedDB, which the developer tools edit.
- **Never load a resource from `user://`.** `load()` or
  `ResourceLoader.load` on a `.tres` or `.res` file can run embedded
  GDScript. A test on Godot 4.7.2 confirmed it: a `.tres` in `user://` with
  a `GDScript` sub-resource ran its `_init` on load. `str_to_var` can make
  an `Object`. Use JSON or `ConfigFile` for data in `user://`.

## 8. PCK encryption and the export settings

- PCK encryption uses the same AES-256-CFB format as `open_encrypted`,
  with the unkeyed MD5.
- The key is 64 hex digits from the preset or the
  `GODOT_SCRIPT_ENCRYPTION_KEY` environment variable. The export template
  holds the key at compile time, so you need custom export templates built
  with `SCRIPT_AES256_ENCRYPTION_KEY`. The official templates cannot read
  an encrypted PCK.
- **Stops**: generic PCK explorers and casual asset rips.
- **Does not stop**: key extraction from the binary, then full decryption
  and decompilation with GDRE Tools. Runtime memory edits.
- Godot 4.7.2 has **no PCK signature**. The PCK directory stores an MD5
  per file, but the engine does not compare it when it opens a file. A
  replaced or edited PCK loads.
- `script_export_mode=2` (binary tokens, compressed) makes the PCK smaller.
  It is not protection: GDRE Tools turns tokens back into GDScript.
- Never commit the key in `export_presets.cfg`. Godot keeps it in
  `.godot/export_credentials.cfg`; set it from the environment in CI.

`godot-export-pipeline` owns the presets and the template builds.
