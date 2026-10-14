---
name: godot-balance-model-data
description: >-
  Keep a game's numbers in a scripted balance model (for example Python modules) that checks
  design targets and exports the JSON the Godot game loads, and change that data without drift.
  It covers the edit-export-diff loop, FAIL-key baselines, deterministic exports, parity
  fixtures between the model and GDScript, what-if and lever searches inside the model, and
  design docs whose tables must match the model. Use it whenever someone wants to change a
  price, a yield, a pacing curve, a content table or an upgrade row that a script exports; asks
  why the model and the game disagree, why a parity test fails, or why a chapter runs too long;
  wants to tune or fit levers before touching game data; or writes a design doc full of numbers,
  even if nobody says "balance model" or "source of truth".
---

# Balance model as the data source

Target engine: **Godot 4.7**. The Python block in `references/` uses the standard library only.

An incremental game has hundreds of numbers: prices, yields, health, pacing, unlock gates.
When people edit them by hand in JSON or in game code, the numbers drift apart and nobody can
say whether a change broke a design target. The fix is one scripted model that owns every
number, prints numbered PASS or FAIL lines against the design targets, and exports the JSON
that the game loads. Every change then goes through a fixed loop: baseline, edit the source,
export, diff, compare FAIL keys, regenerate parity fixtures. It works because each step leaves
a byte-level record, so a reviewer sees exactly which rows moved and which targets broke.

## When to use

- A number, row or text that the game loads comes from a script that exports it.
- You add a content block, a new variant, or a new design target.
- You merge two branches that both changed the model or its exports.
- The model and real play disagree, or a parity test fails.
- Pacing or economy must move and you want to test levers before you edit data.
- A design doc or reference sheet states numbers that the model owns.

## When not to use

- The game has no exported data and no model yet, and the task is a one-off number. Start a
  model only when more than a handful of numbers depend on each other.
- You need measured numbers from real play. Make those with bot fights first (see
  `bot-balance-loop`), then bring them into the model.
- You design an upgrade tree shape or a buy policy. That is `godot-upgrade-tree-design`.

## Method

The model is a set of modules, for example one per domain (content, bosses, variants, areas,
pacing). Each module prints numbered target lines, exits non-zero on any FAIL, and writes its
export. The game only reads the exports.

1. **Keep every gameplay number in its owner module.** Game code reads the export; it holds
   no balance constants. Never hand-edit an exported JSON, because the next export overwrites
   it. Check: grep the game code for the number you change. It appears only in the export.
2. **Save a baseline before any edit.** Run every module and the fast simulator. Save each
   output, each exit code, and a checksum of each export (for example
   `shasum exports/*.json > before.sha`). Check: the baseline files exist and are non-empty.
3. **Reduce FAIL lines to keys.** A key is the check name plus its subject, without the
   numbers, for example `cost wall: node_12`. Record the set of pre-existing FAIL keys. Why:
   old FAIL lines hide new ones, and a module can exit 1 on old rows, so the exit code alone
   proves nothing. Check: you can print the baseline key set
   (`references/script-fail_keys.md` holds a differ; copy the block into a file).
4. **Make a base copy for later diffs.** Use `git archive HEAD <every dir the model reads> |
   tar -x -C /tmp/base`, or a detached base worktree. Copy every folder the model imports, not
   only the module you edit. Check: the base copy prints the same FAIL keys as your baseline.
5. **Edit the source row with an asserted replace.** Match by unique text and require exactly
   1 match. Add a new target as a report-only check first, and confirm the old output does not
   change. Check: the replace helper reports 1 match.
6. **Export and read the diff.** Run the exporter. Read `git diff --stat` and `git diff -U0`
   on the exports. Only your rows or keys may change. Some projects write each export to the
   model folder and copy it into the game folder; compare the 2 copies with `cmp`. Check: every changed line traces to your edit.
7. **Export a second time and compare hashes.** The two runs must be byte-equal. Why: an
   unordered source (a set, a dict built from a set, a file glob) makes the export change on
   each run and floods every later diff. Check: `shasum -c` passes on the second run.
8. **Run every dependent module and diff the FAIL keys.** Compare against the baseline and the
   base copy. Report only new FAIL keys and the keys you cleared. Check: the new-key list is
   empty, or each new key has a written cause and a question to the design owner.
9. **Regenerate the parity fixtures in the same change.** Run the parity tests (see
   `references/model-game-parity.md`). When a value must differ on purpose, add a documented skip
   entry with its reason. Check: the parity tests pass and the fixture hash is stable.
10. **Restore every generated file outside your scope.** A module run often rewrites exports
    that another change owns. Use `git checkout -- <file>`, or a row-level restore script that
    keeps unowned rows byte-equal. Check: `git status` lists only your files.
11. **Merge by source, not by export.** On a conflict in the source, keep both additive sides.
    On a conflict in an export, finish the source merge and re-run the exporter. Check: no
    conflict marker is left (`git grep -n '^<<<<<<<'`).
12. **Put a design-only variant behind a profile switch.** For example an environment variable
    selects the profile. Check: the default profile keeps every export byte-equal.
13. **Commit the source and every exported copy together.** Run a headless Godot import,
    the tests that grep for the changed keys, and one full test suite (see `godot-test-loop`).
    Why: a folder-only run misses stale parity fixtures, telemetry that does not subscribe to a

## Folded methods

- **Model and game parity**: make the model read the game rows, measure the game first, and
  keep a GDScript parity test. Read `references/model-game-parity.md`.
- **What-if and lever search**: test pacing and economy levers in the fast model before you
  edit data. Read `references/what-if-and-lever-search.md`.
- **Design docs that match the data**: a check script owns every fact that docs repeat. Read
  `references/design-docs-match-data.md`.

## Pitfalls

The 7 most important rows. The full table is in `references/pitfalls.md`.

| Symptom | Cause | Fix |
| --- | --- | --- |
| A report-only model run left tracked exports changed, or re-exported another change's rows. | Modules write exports as a side effect. | Run on a scratch copy, or `git checkout` the files after each run. Check first whether a script writes files. |
| A scripted replace changed 2 rows, or silently matched 0. | Replace by value or by line number. | Replace by unique text and assert exactly 1 match. |
| A base run in a temp folder showed 0 FAIL lines. | The copy missed sibling files that the model imports. | Archive every folder the model opens. |
| New FAIL lines were invisible; a module exited 1 anyway. | Pre-existing FAIL lines. | Compare FAIL key sets. The rule is "the same FAIL keys before and after". |
| Folder-only tests passed; other tests broke later. | A stale parity fixture, a new signal with no subscriber, fixtures that pin old rows. | Run 1 full suite after any data, price or signal change. Plan "regenerate the parity fixture" as a step. |
| The model passed and the game failed. | The model read its own copy of the rows. | Make the model read the game rows exactly. |
| A fit passed every target with absurd values. | No guard rule. | Add a band rule, for example 0.5x to 4x of a reference price. |


## Artifacts to build

- **Model modules**: one per domain; each prints `N. <check>: PASS|FAIL <subject> <numbers>`,
  exits non-zero on FAIL, and has an export function.
- **Export command**: one entry point that runs every exporter in a fixed order with sorted
  keys and a stable float format.
- **FAIL-key differ**: reads 2 outputs and prints new and cleared keys. This skill holds one in
  `references/script-fail_keys.md`; copy the block into a file and run
  `python3 fail_keys.py before.txt after.txt`. It exits 1 when a new key appears. The
  default key is the text before the word FAIL, with each number replaced by `#`; `--pattern` with a `(?P<key>...)` group sets
  your own key.
- **Two-run hash check**: `shasum <exports> > a; export; shasum -c a`.
- **Asserted replace helper**: `rep(path, old, new)` that fails unless it finds exactly 1 match.
- **Row-level restore script**: puts unowned rows of an export back to the base version.
- **Parity fixture generator and GDScript parity test**: fixed inputs, a fixture JSON, a
  tolerance, and a skip table with a reason per entry.
- **Profile switch**: an environment variable that selects a design-only variant.
- **What-if runner and score script**: override parsing, a temp copy of the model, a penalty
  per FAIL key plus the distance to the target lengths.
- **MEASURED table**: game numbers with their seeds and source, plus an error-band check.
- **Doc consistency check and doc table generator**: one check per shared fact, and a
  marked-block writer.
- **Save-compatibility check**: every old row id still exists or maps to a new id.

## Related skills

- `godot-bot-balance-loop`: makes the measured game tables that this model calibrates against.
- `godot-upgrade-tree-design`: designs tree shapes and buy policies with real bot play; this
  skill holds the numbers that design produces.
- `godot-test-loop`: the full suite at the end of a data change and the negative-test idea.
- `godot-feature-seams-and-saves`: optional row keys, flagged modes, and save migration when
  rows change.
- `godot-safe-refactor`: proves that a model refactor keeps every output byte-equal.
- `godot-fleet-game-dev`: asserted scripted edits, ownership of generated files, and how to
  report a block when no lever can pass.

## References

- `references/model-game-parity.md`: read when the model and the game disagree, or at Method
  step 9.
- `references/what-if-and-lever-search.md`: read before you tune levers or fit a pacing curve.
- `references/design-docs-match-data.md`: read when a doc or a reference sheet states model
  numbers.
- `references/pitfalls.md`: read when a symptom is not in the short Pitfalls list.
- `references/script-fail_keys.md`: the FAIL-key differ. Copy the block into a file.
