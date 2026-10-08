# Techniques: what each one stops, what it costs, and the Godot API

> Back to [SKILL.md](../SKILL.md).

## 1. Technique table

| Technique | Stops | Does not stop | Cost | Godot 4.7 API |
| --- | --- | --- | --- | --- |
| Schema validation and clamps | Crashes and nonsense values from damage or an edit | A valid but generous edit | Very low | `JSON.new().parse`, `typeof`, `clampi`, `clampf`, `is_finite` |
| Atomic write (temp file, then rename) | A torn file after a crash or power loss | Any edit | Very low | `FileAccess.open`, `DirAccess.rename_absolute` |
| 2 verified backups | Loss of all progress after 1 bad file | Any edit | Low | `DirAccess.copy_absolute` |
| Plain checksum (CRC32, MD5, SHA-256) | Accidental damage | **Any** deliberate edit: the editor computes a new checksum | Low | `HashingContext`, `String.sha256_text` |
| Obfuscation (Base64, XOR, `var_to_bytes`) | A player with a text editor only | Anyone who searches online | Low | `Marshalls`, `FileAccess.store_var` |
| Encryption only (ECB, CBC, CFB, CTR) | Reading without the key | Bit flips (CFB, CTR), block moves (ECB), anyone with the key | Low | `AESContext`, `FileAccess.open_encrypted` |
| HMAC-SHA256 envelope | An edit by anyone without the key | A player who extracts the key | Low | `Crypto.hmac_digest`, `HMACContext`, `Crypto.constant_time_compare` |
| AEAD (AES-GCM, ChaCha20-Poly1305) | Reading and edits without the key | A player who extracts the key | Medium: no built-in API | A GDExtension (for example libsodium) |
| RSA signature | Forged data, even after full reverse engineering | Nothing for data that the client must write | Medium | `Crypto.sign`, `Crypto.verify`, `CryptoKey` |
| Install or account binding | A save copied to another install or account | An edit on the same install | Low | `Crypto.generate_random_bytes`, an account id |
| Counter in a second store | Rollback, if the player cannot reach the second store | Rollback, if the player controls both | Low local; real only on a server | Envelope field plus a second file or a server |
| Server-owned state | Every edit above | Bugs in the server checks | High | `HTTPRequest`, a backend, a platform Web API |

## 2. Checksum, HMAC, signature, AEAD

- A **checksum** has no key. The editor computes it again. It detects
  accidents only.
- An **HMAC** uses 1 secret key to make and to check the tag. The game
  holds the key, so a player who extracts it can forge tags. Use HMAC for
  data that the game itself writes: saves.
- A **signature** uses a private key to make the tag and a public key to
  check it. Use it for data that the developer makes and the game only
  reads: content packs, balance JSON, DLC, update manifests, mod
  allow-lists. The game cannot forge a signature, even after full reverse
  engineering.
- **AEAD** (authenticated encryption) gives secrecy and integrity in 1
  step. A nonce never repeats for 1 key. Godot 4.7 has no AEAD API. When
  secrecy matters, use encrypt-then-MAC: encrypt, then put an HMAC with a
  **second** key over the ciphertext and the header. Check the HMAC before
  any decryption.
- **Secrecy is seldom the goal.** A readable save helps support and mods.
  Prefer a readable JSON body plus an HMAC tag, unless the save holds
  personal data or spoilers.

## 3. Key handling

| Key source | Protects against | Notes |
| --- | --- | --- |
| Constant in a script or the binary | People who do not decompile | GDScript ships as tokens in the PCK, and GDRE Tools recovers it. Split and XOR the key against a plain string search. That is cost, not secrecy. |
| Per-install key: HMAC(build secret, install id) | Save sharing between installs | The install id is random bytes in `user://`. A local editor reads both parts. |
| OS keystore: Windows DPAPI, macOS Keychain, Linux libsecret | Other users, malware that copies files, disk theft | The keystore gives the secret to the logged-in user, so it does **not** hide a developer secret from the player. Use it for the player's own secrets. Godot has no API: use a GDExtension. |
| Server-held key or server-signed state | Every local attacker | Needs a backend and an online check. |
| Platform cloud save | Loss, some casual edits | Cloud sync stores the files that the game writes. The player can still edit them before sync. Console sandboxes make edits much harder, not impossible. |

**Key rotation.** Each envelope carries a key id (`kid`). The game keeps
every old key in a table. A save that verifies under an old key is sealed
again under the current key at the next write. A new game version can
retire a key that a public tool uses: forged saves then fail, while honest
old saves still load during the migration window. Remove an old key only
when the game is ready to mark its saves as "modified".

## 4. Binding a save to an install or an account

- **Install binding**: 16 random bytes in `user://`, mixed into the key.
  A copied save fails on another install. Honest moves to a new PC also
  fail, so offer an export path or bind to the account instead.
- **Account binding**: mix the platform account id (for example a Steam
  ID) into the key. The save moves with the account through cloud sync. It
  does not stop a local edit.
- **No hardware fingerprints.** `OS.get_unique_id()` changes with hardware
  or OS changes, is missing on some platforms, and is personal data in some
  places. A false mismatch damages honest players.

## 5. Anti-rollback

1. Put a monotonic counter inside the tag. Each write adds 1.
2. Keep the highest counter seen in a **second** place: a second file
   (weak), a cloud value (the player can go offline), or a server (strong).
3. On a lower counter, **flag** the save. Do not refuse it: a cloud
   conflict or a restored backup also gives a lower counter.
4. A backup that the game restores itself has a lower counter by design.
   It gets no rollback flag.

## 6. Godot 4.7 crypto facts (from the 4.7.2-stable source)

### 6.1 `FileAccess.open_encrypted` and `ConfigFile.save_encrypted`

- Algorithm: AES-256 in **CFB** mode. The key is 32 bytes.
- The IV is 16 random bytes when the caller gives none. Never pass a fixed
  IV for saves: with 1 key, it leaks the XOR of the first blocks of 2
  saves.
- File layout: magic `GDEC` (4 bytes), MD5 of the plaintext (16), length
  (8), IV (16), ciphertext padded with zeros.
- On read, the engine compares the MD5 of the plaintext and fails with
  `ERR_FILE_CORRUPT`. The MD5 has **no key**. It detects damage and a wrong
  key. It does not detect an edit by a person with the key. The MD5 is in
  clear, so an attacker can confirm a guessed plaintext.
- CFB is malleable. A person who knows the plaintext can probably flip bits
  and write a matching MD5 without the key. This is an inference from the
  code, not a tested result. Treat the format as "not authenticated".
- `open_encrypted_with_pass` and `save_encrypted_pass` use the hex text of
  `MD5(password)` as the key: no salt, no key-derivation function. A weak
  password falls to a dictionary attack.
- `ConfigFile.save_encrypted*` and `load_encrypted*` use the same code and
  have the same limits.

These APIs hide a file from a casual reader. They are not an integrity
check. If a game uses them, add an HMAC over the encrypted file, or use the
envelope in [signed-save.md](signed-save.md) instead.

### 6.2 `Crypto`, `HMACContext`, `HashingContext`, `AESContext`

- `Crypto.hmac_digest(HashingContext.HASH_SHA256, key, msg)` and
  `HMACContext` (`start`, `update`, `finish`) give HMAC-SHA256.
- `Crypto.constant_time_compare(trusted, received)` has no early exit and
  returns false on different lengths. Use it for every tag check. Timing
  matters little for a local save and a lot for a server.
- `Crypto.generate_random_bytes(size)` uses the mbedTLS CTR-DRBG. Use it
  for keys, IVs, nonces and install ids.
- `Crypto.sign(hash_type, hash, key)` and `Crypto.verify(hash_type, hash,
  signature, key)` take a **hash**, not the message. Hash with SHA-256
  first. An RSA key gives PKCS#1 v1.5. Use RSA-2048 or larger. The result
  is the same bytes that `openssl dgst -sha256 -sign` writes.
- `Crypto.generate_rsa(size)` is the only key generator.
  `CryptoKey.load_from_string(pem, true)` loads a public key.
- `Crypto.encrypt` and `Crypto.decrypt` are RSA encryption for small
  payloads. Do not use them for files.
- `AESContext` has ECB and CBC only. ECB shows patterns and has no
  integrity. CBC has no integrity. Use `AESContext` only inside
  encrypt-then-MAC.
- No core API gives AEAD, Ed25519, Argon2 or PBKDF2. A GDExtension adds
  them.

### 6.3 GDExtension for native checks

- Native code holds the key outside the decompilable GDScript tokens. A
  debugger or a disassembler still recovers it.
- A GDExtension adds libsodium (AEAD, Ed25519, Argon2id) or an OS
  keystore.
- Cost: 1 native build per platform, macOS signing and notarization, and
  more attack surface if native code parses untrusted files. Do not write a
  native parser for data that GDScript parses safely.
