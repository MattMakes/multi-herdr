---
name: godot-save-security
description: "Use when you make saves, settings or shipped game data in Godot 4.7 tamper-evident, or review them: a threat model first, the right technique per asset (validation, HMAC envelope, RSA-signed manifest, server), key handling and rotation, rollback flags, recovery UX that never deletes a save, the real limits of open_encrypted and PCK encryption, and a review checklist."
---

# Godot save and data security

Target engine: **Godot 4.7**. Every GDScript block is typed and parses on
4.7.2.

This skill decides **what to protect, against whom, and how far**. It adds
integrity to a save system. `godot-save-load` owns the save system itself:
formats, `ConfigFile`, JSON, the save architecture and migration.

## The honest limit

Every key that the game uses is on the player's machine. A client-side
check can **detect** an edit, **raise the cost** of an edit and **recover**
from damage. It cannot **enforce** a value. Only a server that the player
does not control can enforce a value.

- Call a client check "tamper-evident". Never call it "tamper-proof",
  "secure" or "uncheatable", in code comments, docs or store text.
- A false alarm on an honest save does more harm than a cheater in a
  single-player game. Crashes, full disks and cloud-sync conflicts cause
  most bad saves.

## Words used in this skill

- **MAC**: a short tag over data, made with a secret key. A person without
  the key cannot make a valid tag for changed data. HMAC-SHA256 is the MAC
  here.
- **Signature**: a tag made with a private key and checked with a public
  key. The game ships the public key only, so the game cannot make a tag.
- **Envelope**: the outer save structure: format id, version, key id,
  counter, body and tag.
- **Rollback**: the player puts back an older valid save.

## Pick the technique per asset

Start with the threat model ([references/threat-model.md](references/threat-model.md)).
Then pick 1 row per asset.

| Asset | Who is harmed by an edit | Technique | Reference |
| --- | --- | --- | --- |
| Single-player save, local best | Only that player | Validation, clamps, atomic write, 2 backups, recovery UX. No crypto. | [validation-and-recovery](references/validation-and-recovery.md) |
| Single-player save that unlocks platform achievements, or a "modified" badge | The player, global unlock rates | Add the HMAC envelope as a **marker**: a bad tag loads and marks the profile | [signed-save](references/signed-save.md) |
| Copied or shared saves | Depends on the game | Key bound to the install or to the account id | [signed-save](references/signed-save.md) |
| Settings | Nobody | Validation and clamps only | [validation-and-recovery](references/validation-and-recovery.md) |
| Shipped balance and content data in the main PCK | Nobody (the player can replace the check too) | Nothing, or a signed manifest as a speed bump | [shipped-data](references/shipped-data.md) |
| Patches, DLC, downloaded tables, mod allow-lists | Every player who installs them | RSA-signed manifest; verify before mount | [shipped-data](references/shipped-data.md) |
| Leaderboards, shared stats, economy, items with value, multiplayer | Other players, revenue | A server or a platform "trusted" write. A client check does not help. | [online](references/online.md) |
| Player's own secrets (refresh tokens) | The player, by malware | OS keystore through a GDExtension | [techniques](references/techniques.md) |

## The rules

Each rule has a pass mark that [the review](references/review-checklist.md)
checks.

1. **Validate every field** of every file from `user://`, also after a good
   tag. Type check, clamp to the design range, reject NaN and infinity,
   drop unknown ids.
2. **Atomic writes and 2 verified backups.** Write a temp file, then rename.
   A file that fails the check never replaces a good backup.
3. **Never delete a bad save.** Copy it to `user://quarantine/` first. Tell
   the player what loaded and from when.
4. **Never load code from `user://`.** No `load()` of `.tres`, `.res`,
   `.tscn`, `.scn` or `.gd` from `user://`; no `str_to_var` or
   `bytes_to_var_with_objects` on `user://` data. Use JSON or `ConfigFile`.
5. **Integrity needs a key.** A plain hash (MD5, SHA-256, CRC) detects
   damage only. Use HMAC-SHA256 for saves that the game writes, and an RSA
   signature for data that only the developer writes.
6. **Encryption is not integrity.** `FileAccess.open_encrypted*`,
   `ConfigFile.save_encrypted*` and `AESContext` are never the only check.
7. **The tag covers the exact stored bytes** plus the version, the key id,
   the counter and the flags. Compare tags with
   `Crypto.constant_time_compare`.
8. **Keys have ids.** Keep old keys for a migration window. Seal again under
   the current key at the next write.
9. **No secret as a plain string literal.** No key or password in a script
   constant that a `strings` search finds. Random bytes come from
   `Crypto.generate_random_bytes`, never from `randi()`.
10. **Flag, do not punish.** A rollback, a bad tag or a mod marks the
    profile. Only a server-owned reward can refuse a save. No client-side
    ban.
11. **A newer save is read-only.** A save from a newer game version does
    not load and is not overwritten.
12. **Shared values live on a server.** Every public board, shared stat
    and valuable item has a server or platform owner.

## Pick the reference

| Task | Read |
| --- | --- |
| Who tampers, what a client check can do, when you need a server | [references/threat-model.md](references/threat-model.md) |
| Compare checksum, HMAC, signature, AEAD; keys; binding; rollback; Godot crypto API limits | [references/techniques.md](references/techniques.md) |
| Build the HMAC save envelope, the store with backups and the key table | [references/signed-save.md](references/signed-save.md) |
| Field checks, the player message on a bad save, the test matrix | [references/validation-and-recovery.md](references/validation-and-recovery.md) |
| Sign content data, verify a pack or a mod, PCK encryption, export settings | [references/shipped-data.md](references/shipped-data.md) |
| Leaderboards, Steam, auth tickets, plausibility and replay checks | [references/online.md](references/online.md) |
| Lessons for any engine, and mistakes from shipped games | [references/general-lessons.md](references/general-lessons.md) |
| Audit a game and report pass or fail | [references/review-checklist.md](references/review-checklist.md) |

## Workflow

**New game.** Write the threat-model table for each asset. For a
single-player prototype, do only rules 1 to 4 and 11. Add the HMAC envelope
when a platform feature or a "modified" marker needs it. Add a server when
a value reaches another player.

**Existing game.** Run the review first. Fix in this order: data loss
(rules 2, 3, 11), code execution from `user://` (rule 4), validation
(rule 1), false claims in docs, then integrity.

## Other skills own these parts

| Need | Skill |
| --- | --- |
| Save formats, `ConfigFile`, JSON, save architecture, migration, when to save | `godot-save-load` |
| Export presets, custom export templates, PCK files | `godot-export-pipeline` |
| Native code (libsodium, OS keystore) | `godot-gdextension` |
| Game servers, authority, validation of client input | `godot-dedicated-server`, `godot-multiplayer-sync` |
| Currency and payouts that a server must own | `godot-economy-system` |
| Headless test runs and the check sequence | `godot-build-verify`, `godot-testing` |

## Prove it

> proof: headless-run: `SaveKeys`, `SignedSave`, `SaveStore`, `SaveFields`,
> `ManifestCheck`, `sign_manifest.gd` and `make_key_parts.gd` ran on Godot
> 4.7.2: seal, verify, edit, old key, rollback, copy to another install,
> recovery, quarantine, a newer save, a signed manifest, an edited data
> file, an edited pack. Steam, a server and a Windows rename: proof: not
> run (needs a store SDK, a service, Windows).

## Report

Report each checklist item as a row with the value found and the pass
mark. Write "not run" with the reason for a check you did not run. When a
task does not say whether a value reaches other players, treat the game as
single-player. Send 1 `QUESTION:` to the orchestrator that names the online
case. Do not wait for the answer before you continue.
