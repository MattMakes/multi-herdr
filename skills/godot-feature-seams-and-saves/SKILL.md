---
name: godot-feature-seams-and-saves
description: >-
  Add new content, variants and whole modes to a Godot game through data seams and
  off-by-default flags, and keep player saves and settings safe across versions. It covers a
  new playable variant (vehicle, class, character) or encounter batch on a shared base class,
  an optional row key whose default keeps old rows exact, a prototype branch plus
  a flagged landing for a big mode, mode fields on content rows, atomic signed saves with
  backups, save-version migration tested with a real old save, graphics presets with a
  first-launch Auto pick, and setup tools that give the same project file on a second run.
  Use it whenever someone wants to add a character, enemy, boss, level type, game mode or data
  field, bump the save format, rename a settings key, harden saves against corruption or
  edits, or asks "will old saves still load?" or "how do I add this without breaking the
  base game?", even if nobody says "seam" or "migration".
---

# Feature seams and saves

Target engine: **Godot 4.7**. The skill has no GDScript blocks. The 1 Python script uses the standard library only.

A game with persistent progress grows by adding content to shared code, and each addition
can change old behaviour or break old saves without a visible error. This skill adds every
new thing through a seam: a small, tested hook in shared code or data whose default keeps
today's behaviour exactly. Big changes go behind a data flag that is off until they are
ready, and every save format change ships with a migration that a real old save proves. It
works because each step has a check that can fail: the old rows, the flag-off suite and the
old save fixture all must give the same result as before.

## When to use

- You add a playable variant, an enemy, a boss or a batch of content of 1 kind on a shared base.
- New content needs a new field in a data row, and old rows must not change.
- Several people or agents want the same local edit in 1 shared script.
- A large mode or render path needs a try-out before it may touch the shipped game.
- 2 modes share content rows, and content of one mode shows up in the other.
- You change the save format or a settings key, or you harden saves against crashes and edits.
- You add quality presets, or a tool script writes into the project file.

## When not to use

- A pure number tweak in existing rows. Use the balance skills (see Related skills).
- A refactor that must keep behaviour. Use `godot-safe-refactor` for the identity proof.
- An online game where the server owns the save. A client check cannot enforce values there.

## Method: add a variant behind a seam

This is the main procedure. The later sections are the same idea at other scales.

1. **Run the baseline suite before any edit.** Record the pass and fail counts. A later
   failure then shows as new, not as old noise.
2. **Read the base.** Read the design section, the base class, the test dummy and the base
   type's script, data row and notes. Look at the art before you design parts.
3. **List what each subclass uses.** Grep, or write a short script, that lists the base-class
   members each existing subclass reads or overrides. The list shows which hooks already
   exist and which you must add.
4. **Map every seam you need, then ask once.** A seam is a hook in a file you do not own: a
   virtual method, a signal, a row key. Send all of them to the file owner in 1 question,
   with the exact line and place of each. Continue other work while you wait. One question
   costs one round trip; five questions cost five.
5. **Commit the seam alone, with its tests.** Peers can then merge it early, and a review sees
   the shared change apart from the content.
6. **Add the variant as data plus script.** Give it its own data row through the data
   generator, not by hand. Register its behaviour script in a table (for example a table of
   drive scripts keyed by variant id). Write each encounter as an inherited scene of the base
   scene, with its own script.
7. **Keep shared helpers private.** Put helper scripts in a folder you own, with no
   `class_name`. A global class name can clash with a sibling unit that lands the same day.
8. **Ship a placeholder model with a frozen public API.** An art worker can then build the
   real model in parallel to the same API. Tell the lead before any signature changes.
9. **Build in steps with a rig test per step.** Use an isolated test rig: motion, special
   verbs, effects, stat reads, run wiring. Wait for the right state before each check (for
   example idle, not a recovery window). Check: each rig test fails when you break its step.
10. **Check budgets once.** Print the peak of each budget a single time (for example peak
    bullets on screen). Then remove the print. Capture each scene at its key states, such as
    idle, tell, open and window.
11. **Prove fairness against the base type.** Measure the per-frame cost against the base
    type (for example a limit of 0.3 ms). Run the encounter checks with both types at the same
    seeds and compare first-try rates. Run the base type on the same build too, so that a
    balance change merged by someone else does not look like the variant's effect.
12. **Make every upgrade felt.** Each upgrade node of the variant needs an effect the player
    can see or feel. A node with a number change only is not done.
13. **Swap in the real model.** Delete the placeholder and its warm-up entry when nothing reads
    them. Run the tests again. Check: a grep for the placeholder name returns nothing.

## Optional row keys and flagged mode rollout

The same seam idea at a larger scale: a new optional key in shared data rows, and a large mode
behind an off-by-default data flag, in 2 stages. Read `references/row-keys-and-mode-rollout.md`.

## Save integrity and save version migration

An atomic, signed save store with backups and typed loads, and a migration chain that a real
old save proves. Read `references/save-integrity-and-migration.md`.

## Graphics presets and idempotent setup tools

Quality presets with a first-launch Auto pick, and tool scripts that write the same
`project.godot` on a second run. Read `references/presets-and-setup-tools.md`.

## Pitfalls

The 8 most important rows. The full table of 31 rows is in `references/pitfalls.md`.

| Symptom | Cause | Fix |
| --- | --- | --- |
| The variant's special verb never fires in the encounter check. | No caller wires the verb. | Record the table as "before". Wire the verb. Run "after". |
| An upgrade node does nothing visible. | The node changes a number only. | Build a felt effect for every node. |
| A lookup crashes on a new id. | The id has no row. | Return an empty dictionary; fall back to the default. |
| Old rows fail validation. | A test requires the new field on all rows. | Require it only on the new rows. |
| Content of one mode shows in the other. | A hint or index list does not check the mode. | Filter by the row mode field. |
| An edited save loads without warning. | The encrypted file format has no integrity check against a key holder. | Sign with HMAC; compare in constant time. |
| A save decodes an object. | `str_to_var` decodes objects. | Save plain dictionaries only. |
| Auto picks again on every launch. | The first-launch pick is not saved. | Save in the no-file branch. |

## Artifacts to build

- **Seam test file**: per shared key, 1 test with the key, 2 without, and 1 unknown-key default test.
- **Shared-key list**: 1 comment block or table that names every optional row key and its default.
- **Subclass member lister**: a short script that greps each subclass for base members it uses.
- **Variant rig**: an isolated test scene that drives one variant through motion, verbs and effects.
- **Variant check script**: runs the encounter checks for base and variant at the same seeds; prints first-try rates and frame cost.
- **Proto generator**: a script that applies the proto edits to the current upstream file.
- **Open-items file**: 1 line per item that must close before a flag turns on, with an owner.
- **Save store**: atomic write, backup rotation, HMAC, typed load with defaults, restore notice.
- **Save fixtures folder**: 1 real save per old version, named by version.
- **Migration chain**: 1 function per version step, run in order from the file's version.
- **Preset table**: preset name to a dictionary of setting values, plus the effective-value function.
- **Project setup tool**: 1 defaults table and a writer that gives the same file on a second run; check it with the script in `references/script-check_idempotent.md` (copy the block into a file).

## Related skills

- `godot-safe-refactor`: the identity proof for the flag-off path, and the search for every reader of a key.
- `godot-test-loop`: the test loop, and how to prove that a seam test can fail.
- `godot-headless-runs`: scratch projects for engine probes, and tools that stay off the real save.
- `godot-perf-measure`: the A/B frame-cost measure for a variant, a mode or a graphics option.
- `godot-bot-balance-loop`: the encounter checks that compare a variant with the base type.
- `godot-balance-model-data`: the model term for a new row key and the per-mode model profile.
- `godot-upgrade-tree-design`: felt effects for each upgrade node of a new variant.
- `godot-pixel-art-pipeline`: the real model that replaces the placeholder.
- `godot-visual-verification`: captures of the variant and of proto options for a pick.
- `godot-release-and-store`: save isolation per edition and input defaults for pads.
- `godot-fleet-game-dev`: how to ask for seams, land seam commits and handle sandbox write limits.

## References

- `references/row-keys-and-mode-rollout.md`: read it when content needs a new row field, or when a large mode needs a prototype and a flag.
- `references/save-integrity-and-migration.md`: read it when you harden saves, change the save format or rename a settings key.
- `references/presets-and-setup-tools.md`: read it when you add quality presets, or when a tool writes into `project.godot`.
- `references/pitfalls.md`: read it when a symptom is not in the short Pitfalls list above.
- `references/script-check_idempotent.md`: read it when you check that a setup tool changes the project file once only.
