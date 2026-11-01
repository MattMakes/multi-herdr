# Review checklist: audit the saves and data of a Godot game

> Back to [SKILL.md](../SKILL.md).

An agent runs this review on a Godot game. Each item has a check that reads
files, runs `grep`, or runs the game headless. Report each item as a row:
item, value found, pass mark, result. Write "not run" and the reason for an
item that you did not run. A "FAIL" needs a fix or a written reason.

Start with the threat-model table ([threat-model.md](threat-model.md)).
Items 20 to 22 apply only when a value reaches other players.

## 1. Saves and `user://`

1. **Inventory.** `grep -rn 'user://' --include='*.gd' .` List each file,
   its writer, and its row in the threat model.
2. **Atomic write.** Each save write goes to a temp file, then
   `DirAccess.rename_absolute`. FAIL on a direct write to the final path.
3. **Rename over an existing file.** The code handles a failed rename: it
   moves the old file aside or removes it, then renames again. Test on the
   Windows export, or write "not run (needs Windows)".
4. **Backups.** 2 backups (`.bak1`, `.bak2`); 1 backup passes with a
   written reason. The rotation copies only files that pass the check. FAIL
   if a bad file can replace a good backup.
5. **No delete on load.** FAIL on `remove_absolute` in a load path without
   a copy to a quarantine folder first. Check the next save too: does it
   overwrite a bad file that nobody copied?
6. **Validation.** The load code checks the type of every field, clamps
   every number to its design range, rejects non-finite numbers
   (`is_finite`), and drops unknown ids. Feed it `-50`, `1e999`, a level
   above the maximum and an unknown id.
7. **Version.** The save has a version field. A version newer than the game
   does not load and is not overwritten (`TOO_NEW`).
8. **No code from `user://`.** FAIL on `load(`, `ResourceLoader.load` or
   `ResourceLoader.load_threaded_request` with a `user://` path to a
   `.tres`, `.res`, `.tscn`, `.scn` or `.gd` file.
9. **No objects from `user://`.** FAIL on `str_to_var`,
   `bytes_to_var_with_objects` or `get_var(true)` on `user://` data.
   `bytes_to_var` without objects passes.
10. **Tag claim.** If the game claims tamper detection: an HMAC-SHA256 or a
    signature covers the exact stored bytes, the version, the key id and the
    counter, and the compare is `Crypto.constant_time_compare`.
11. **Encryption is not the check.** FAIL if `open_encrypted`,
    `open_encrypted_with_pass`, `save_encrypted`, `save_encrypted_pass` or
    `AESContext` is the only integrity check.
12. **No literal secret.**
    `grep -rnE 'open_encrypted_with_pass|save_encrypted_pass|load_encrypted_pass|hmac_digest|HMACContext' --include='*.gd' .`
    Read the key argument of each hit. FAIL on a plain string literal.
13. **Random source.** Keys, IVs and install ids come from
    `Crypto.generate_random_bytes`. FAIL on `randi()`, `randf()` or
    `RandomNumberGenerator` for these.
14. **Key ids.** A tagged save carries a key id, and old keys stay in the
    table. A save under an old key loads and is sealed again.
15. **Recovery UX.** The player sees a message for a recovered, an
    unreadable and a newer save. The recovered message names the backup
    time. No message says "tampering".
16. **Test matrix.** Rows 1 to 12 of the matrix in
    [validation-and-recovery.md](validation-and-recovery.md) run headless
    and pass.

## 2. Game data and packs

17. **Data source.** `res://` data loads only from `res://`, unless mods are
    a documented feature.
18. **Packs.** `grep -rn 'load_resource_pack' --include='*.gd' .` For each
    call: the pack is verified before the mount, or it is a documented mod
    feature. Record the `replace_files` value. FAIL on an unverified
    download mounted with `replace_files = true`.
19. **Export settings.** In `export_presets.cfg`, record `encrypt_pck`,
    `encrypt_directory` and `script_export_mode`. If `encrypt_pck=true`:
    custom templates are set, and the key comes from
    `GODOT_SCRIPT_ENCRYPTION_KEY`. FAIL if a key is in a committed file:
    `git ls-files | xargs grep -ln 'script_encryption_key="[0-9a-f]'`.

## 3. Online values

20. **Owners.** Each leaderboard, shared stat, achievement and economy
    value has a server owner or a platform trusted setting. FAIL on a
    client-set score on a public board.
21. **Server checks.** The server checks auth tickets, applies plausibility
    limits, and stores seeds and input logs of top runs.
22. **Marked profiles.** A modified or modded profile cannot submit to a
    shared board.

## 4. Claims

23. **Words.** `grep -rniE 'tamper-?proof|uncheatable|unhackable|secure save' .`
    over docs, store text and code comments. FAIL on a claim that a client
    check cannot keep.

## 5. Report

| # | Item | Found | Pass mark | Result |
| --- | --- | --- | --- | --- |
| 2 | Atomic write | `meta.gd` writes `save.json.tmp`, then renames | temp file, then rename | pass |
| 6 | Validation | types checked, no ranges | clamp, `is_finite`, drop unknown ids | FAIL |

End with the 3 fixes that give the most value for the threat model, in the
fix order of SKILL.md.
