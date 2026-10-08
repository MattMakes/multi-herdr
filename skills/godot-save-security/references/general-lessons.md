# General lessons for any engine

> Back to [SKILL.md](../SKILL.md).

These lessons do not depend on Godot. They apply to Unity, Unreal, a custom
engine or a web game.

## 1. The 10 lessons

1. **The client is the player's machine.** Every key, every check and
   every value on it is the player's to read and change. Client checks
   detect and raise cost. Servers enforce.
2. **Accidents beat attackers.** Crashes, full disks, sync conflicts and
   bad mods cause most bad saves. Atomic writes, verified backups and
   validation come before cryptography.
3. **Integrity needs a key; secrecy does not give integrity.** A checksum
   detects damage. A MAC detects an edit by a person without the key. A
   signature detects a forgery even by a person with the full client.
   Encryption alone detects none of these.
4. **Pick the primitive by who writes the data.** The game writes it: a
   MAC. Only the developer writes it: a signature. Another player depends
   on it: a server.
5. **Tag the exact bytes.** Put the body into the envelope as a string,
   and tag that string plus the version, the key id, the counter and the
   flags. A re-serialised body changes key order or float text, and honest
   saves fail.
6. **Version everything.** The envelope version, the body schema version
   and the key id. A newer file is read-only for an older game.
7. **Never destroy evidence.** Copy a bad file aside before anything
   writes over it. Tell the player what loaded.
8. **Flag, do not punish.** A rollback, a bad tag, a mod: mark the
   profile, keep it off shared boards. Bans come from server evidence only.
9. **Untrusted data is data, never code.** Do not deserialize objects,
   scripts or engine resources from a player-writable folder.
10. **Say only what is true.** "Tamper-evident" and "raises the cost" are
    true for a client check. "Secure", "tamper-proof" and "uncheatable" are
    not.

## 2. Mistakes from shipped games

- **A fixed key in the client, AES in ECB mode.** Hollow Knight (Unity)
  encrypts its saves with AES-ECB and a fixed string key. A public browser
  save editor has the key as a constant. The
  encryption stopped nobody who wanted to edit, and it made support and
  modding harder.
- **A fixed password through PBKDF2.** Phasmophobia (Unity) derives its
  AES-CBC key with PBKDF2 from a password in the game. Public tools decrypt
  and edit the saves. A key-derivation function does not help when the
  password ships in the client.
- **Replay checks without timing analysis.** In the 2020 Trackmania case,
  slowed-down runs passed the replay checks. Analysis of stored replays
  found them.
- **Client-set scores by default.** Platform leaderboards and stats trust
  the client unless the developer turns on trusted or server-set writes.

## 3. Common mistakes as a list

1. A key or password as a plain string literal.
2. ECB mode.
3. CBC, CFB or CTR without a MAC. Bit flips go undetected, and CBC on a
   server can be a padding oracle.
4. A plain hash as the "tamper check".
5. A MAC over re-serialised data instead of the stored bytes.
6. `==` for a tag compare on a server instead of a constant-time compare.
7. A fixed or repeated IV or nonce. With AES-GCM, a repeated nonce
   reveals the authentication key.
8. Home-made crypto: a custom XOR "cipher" or a custom hash.
9. Objects, scripts or engine resources loaded from a player-writable
   folder.
10. A save deleted, or progress reset, after a failed check.
11. A backup rotation that moves a bad file over a good backup.
12. No version field.
13. "Secure" or "uncheatable" in docs or store text.
14. A ban from a client-side check.

## 4. Standards to name

- HMAC: RFC 2104. Use HMAC-SHA256.
- AES-GCM: NIST SP 800-38D. ChaCha20-Poly1305: RFC 8439. Use
  XChaCha20-Poly1305 when nonces are random.
- RSA signatures: PKCS#1. Use 2048 bits or more, with SHA-256.
- Update metadata: The Update Framework (TUF) roles and specification.
- OS keystores: Windows DPAPI, macOS Keychain, Linux libsecret. They
  protect the player's secrets from other users, not a developer secret
  from the player.
