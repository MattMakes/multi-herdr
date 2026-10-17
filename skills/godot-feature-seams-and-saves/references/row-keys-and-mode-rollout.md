# Optional row keys and flagged mode rollout

This file holds 2 methods of `godot-feature-seams-and-saves` at a larger scale than 1 variant: a new optional key in shared data rows, and a large mode behind an off-by-default data flag.

## Optional row keys

Use this when content needs a new data field, or 2 or more units plan the same edit in 1
shared script.

1. Find every reader of the row. Include second render paths (for example a node renderer
   and a MultiMesh renderer) and tool scripts. A missed reader ignores the key silently.
2. Find duplicate plans. If 2 units plan the same local change, propose 1 shared seam
   instead, with its exact line and place.
3. Read the field as optional, with a default equal to today's behaviour. Every reader uses
   the same default.
4. Define the rule for short lists. For example, a short list repeats its last entry.
5. Make a lookup of an id with no row return an empty dictionary, then fall back to the
   default. It must not crash.
6. Make a repeated call safe. For example, the seam emits an end signal once per window. A
   leftover local copy then does no harm. Tell the units to remove their local copies.
7. Add the matching term to the balance model, if you keep one, so model and game agree.
8. List every shared key in 1 place, for example a comment block at the top of the base script.
9. Commit the seam alone, apart from the row commits.

Tests for a seam:

- A row with the key changes the result.
- At least 2 rows without the key keep today's exact result.
- Every known key maps to a value, and an unknown key gets a safe default.
- The new field is required only on the new rows. Old rows stay valid without it.

## Flagged mode rollout

Use this for a large change: a new game mode, a new render path, a design option. It must not
change the shipped game until it is ready. This skill owns the mode rollout: the flag, the
stages, the flag-on tests and the turn-on gate. `godot-safe-refactor` owns only the
identity proof (hash or `cmp` compares, paired runs) that Stage 2 step 3 calls for.

**Stage 1: prototype.**

1. Grep the code for each assumption the change breaks: sizes, clamps, bounds, fixed ids.
   Write this assumption map before you design.
2. Make 2 branches from the same base: a proto branch that never lands, and a build branch
   that does. Mockups, A/B switches, debug hooks and merges of unlanded branches go only to
   the proto branch.
3. Build a quick prototype and a drive tool that plays it, logs cost and takes captures.
4. Run the full suite on the prototype. The failures list the old assumptions for you.
5. Put each debug cost in the prototype behind an environment flag. A debug hash every
   frame can add tens of ms and spoil the cost measure.
6. If upstream changes a file the proto edits, take the upstream file. Rebuild the proto
   variant with a generator script that applies the proto edits to any upstream copy.
7. Write the design doc: the assumption map, the measured cost, the stages and real captures.
   Send the options or measures to the decision maker and wait for the pick.

**Stage 2: build behind a flag.**

1. Add 1 top-level data flag, off by default. Every new code path checks the flag first.
2. Add the new data rows with the flag off.
3. With the flag off, run the full suite. It must give the same result as the baseline. The
   shipped data export must stay byte-equal. Use the identity proof of
   `godot-safe-refactor` for this compare.
4. If a balance model exists, give the new mode its own profile. The old profile output must
   stay byte-equal.
5. Port only the chosen part from the proto branch, with tests. Keep proto files out.
6. Add tests for the mode with the flag on. Each test sets the flag in setup and resets it in
   teardown, because a leaked flag changes later tests.
7. Measure the perf cost with the flag on.
8. Keep an open-items file for the mode. Clear it before the flag turns on.
9. Turn the flag on only when the open-items file is empty, the flag-off suite still matches
   the baseline and the flag-on tests pass.

**Mode fields on content rows.** When 2 modes share data, give each content row a mode field
(for example `single`, `world`, `both`). Runtime systems, hint lists, index pages and tests
filter by that field, not by a pinned list of ids, so a new row works with no list edit.
Write 1 test per mode. The test sets the mode flag in setup and restores it in teardown.
Prove the filter: disable it, and see the mode test fail.
