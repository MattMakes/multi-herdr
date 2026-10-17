# Save integrity and save version migration

This file holds the 2 save methods of `godot-feature-seams-and-saves`: a save store that survives crashes and edits, and a migration that a real old save proves.

## Save integrity

Use this when a save must survive a crash, a failed write, a corrupt or edited file and bad
settings values.

1. **Write the threat model first.** A client-side check can detect an edit. It cannot enforce
   values, because the player owns the machine and the key.
2. **Build the store in a scratch project.** Write 1 test per failure case before you wire it
   into the game: a changed body, a flipped flag, a truncated file, an empty file, a failed
   write, out-of-range settings, a version newer than the build.
3. **Write atomically.** Write to a temp file, then rename it over the main file. Rotate 2 or
   more backups on each good save. Keep only verified files as backups.
4. **Sign the save.** Use an HMAC (a keyed hash) or at least a checksum. Compare signatures with
   a constant-time function. The Godot encrypted file format alone does not detect an edit by
   someone who has the key.
5. **Parse without engine errors.** Use a `JSON` instance and check its error code. The static
   parse prints an engine error on bad input.
6. **Check every field.** Check the `typeof` of each payload key. Turn a wrong type or a
   missing field into the default (for example a number-or-zero helper). Clamp every
   settings value to its range.
7. **Recover.** On a bad main file, restore the newest good backup. Show a notice on the title
   screen, so the player knows.
8. **Save plain dictionaries only.** Do not use `var_to_str` and `str_to_var` on save data,
   because they encode and decode objects.
9. **Version the save.** Add a version field and a round-trip test for each version (see the
   next section).
10. **Keep test saves out of the real save.** Tests write to a git-ignored test output folder,
    never to the player's real save path.
11. If you keep stamped reset copies and prune old ones, test the prune with copies made in
    the same second. Give each new stamp a value above the newest kept stamp.

## Save version migration

Use this when a feature changes the save format or a settings key, and old saves must load.

1. Copy a real save of the old version into a test fixture folder. Do not hand-build a
   dictionary, because a hand-built one has the shape you expect, not the shape players have.
2. Write a failing test. It copies the fixture to a scratch path, loads it and checks the new
   version number and the new fields.
3. Bump the save version constant. Write 1 migration step from the old version to the new one.
   Chain steps, so a save that is 2 versions old runs both.
4. Check that every level, unlock and currency survives, and that new fields get defaults.
5. Check that the old key is no longer read or written after the migration.
6. For a settings key that changes type (for example a boolean to a 3-state value), make the
   new key the authority. Migrate the old value. Keep the old key as a synced alias for old
   callers until they move.
7. Keep old in-memory APIs that callers still use.
8. Add save, load and reset tests for each new persistent field (for example a "seen once"
   flag that returns true one time only).
9. Run the save test folders, then the full suite. If some runs cannot write to `user://`
   (for example in a sandbox), list those failures by name, apart from real ones.
