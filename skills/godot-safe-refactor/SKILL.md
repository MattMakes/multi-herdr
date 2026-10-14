---
name: godot-safe-refactor
description: >-
  Prove that a code change in a Godot project keeps behaviour unchanged, find every site that
  enforces a rule before you change it, scan the code base for measured fix candidates, run
  read-only reviews whose numbered findings small fix units close, and isolate render or feel
  bugs whose cause is unknown. Use it whenever someone refactors gameplay, draw, mesh, model or
  save code; makes hot code faster; switches a renderer or a render path; must prove that a
  planned mode behind a flag leaves the old path unchanged when the flag is off; moves, renames
  or deletes a function, constant or rule; asks "did this change anything",
  "is this dead code", "what should we clean up", "review this build" or "why does this draw
  nothing / feel wrong / judder"; or sees a measure that differs from a recorded table. Use it
  even if nobody says "refactor", "golden trace" or "code review".
---

# Safe Refactor and Review

Target engine: **Godot 4.7**. The Python block in `references/script-first-diff.md` uses
the standard library only.

A refactor in a game breaks things quietly. A renamed private function still has a caller that
uses a string. A speed fix changes a score at float precision. A view and the game logic decide
the same rule in two places and drift apart. The method is simple: record a deterministic
reference before any edit, change one item at a time, and compare the reference by hash after
each item. Find every caller and every copy of a rule first, so that the proof covers the whole
change. This works because a byte-equal trace from a seeded, fixed-step run is a proof that a
test suite alone cannot give: tests check what someone thought of, and the trace checks
everything the game did.

## When to use

- A refactor, a code move or a rename must leave game behaviour unchanged.
- A speed fix in hot code must give the same game results.
- A renderer switch, a draw-path change or a mesh-builder change must give the same pixels or
  vertices.
- A new mode goes in behind a flag, and the old mode must stay identical with the flag off.
- You delete, move or change a name, a rule or a mechanic that several code paths use.
- A measure today differs from a landed table, and you must find whether your tool or a later
  game change caused it.
- You want a ranked, measured list of safe code fixes before a cleanup.
- Someone wants a read-only expert review of a build, with findings that fix units close.
- Something draws wrong or nothing, or feels wrong, and the cause is unknown.

## When not to use

- The change is meant to alter behaviour (a balance change, a new mechanic). Use tests for the
  new behaviour instead, and a reference only for the parts that must stay.
- You only need frame timings. See `godot-perf-measure`.
- You only need pixel comparisons of UI states. See `godot-visual-verification`.
- The failure is a known flaky test or an exit leak with a known shape. See `godot-test-loop`.

## Method: behaviour identity proof

1. **Record the test baseline.** Run the full suite and the focused filters for the affected
   files. Save each pass, fail and skip count. Check: the counts are in a file, not only in your
   scroll-back.
2. **Pick the reference by code type.**

   | Code type | Reference |
   | --- | --- |
   | Gameplay logic | A per-tick trace or a path recording: seeded input, fixed ticks, fixed fps |
   | Hot gameplay code | An event log (1 line per spawn, hit, kill, drop) or a SHA-256 digest per state |
   | Draw code | Window captures of fixed states |
   | Mesh code | A digest of vertices, normals and UVs |
   | Model or table code | A dump of the computed tables or the simulator output |
   | Save code | A fixture save written with the old code, plus a test that loads it |

   Check: the reference covers at least 1 deterministic event of the system that you change. If
   the scenario has no such event, add 1 fixed source for it. For example, if you change combat
   code and the scenario has no kill, add 1 fixed enemy, so kill events enter the proof.
3. **Remove drift.** Use the same seed, a fixed frame rate and no delta smoothing. Pin every
   timing setting that can vary, for example hit-stop. Make the baseline twice and diff the 2
   runs. Check: the 2 runs match byte for byte. If they cannot match, record the normal spread
   and say so in the report. A proof against a noisy reference proves nothing.
4. **Keep an exact base copy.** Extract the base without a git write:
   `git archive HEAD <project dir> | tar -x -C <temp dir>`. A detached worktree also works. For
   a single script, save the old file under a temp path and strip its `class_name` line, so the
   2 versions do not clash. Run the Godot import in the copy once. Check: the base copy gives the
   baseline reference.
5. **Find every caller.** Search the whole repository for each name you move or delete. Include
   string calls and tools outside the game folder. See "Find every enforcement site" below.
   Keep private forwarders or aliases for callers that you may not edit.
6. **Change 1 item per commit.** After each item, compare the reference by hash or `cmp`, and
   compare the test counts. Check: the hashes are equal, and the counts are equal plus only your
   new tests.
7. **For a speed change, run pairs.** Run the base probe and the changed probe back to back
   with the same seed. Equal hashes prove identity. The median and the worst case of each pair
   show the gain. Report only paired results, because machine load drifts between runs. Reject
   a change that alters any result, even at float precision.
8. **Turn a path recording into a regression test.** Record the fixture on the old commit.
   Replay it in a test. Check: change 1 constant and watch the test fail. A replay test that
   cannot fail is decoration.
9. **Put a new mode behind a flag that is off.** Run the full suite with the flag off. Add tests
   for the mode with the flag on. Reset the flag at the end of each test that sets it, because a
   leaked flag changes later tests.
10. **Check warnings.** Run the warning check on the base and on your branch. Check: your files
    add 0 warnings.
11. **When an outcome differs, find the first difference.** Diff the event logs or the telemetry
    to the first line that differs. The first difference names the cause. Later differences are
    only its echo. Use `references/script-first-diff.md`: copy the block into a file.

## Find every enforcement site

Use this before you delete, move or rename a name, or before you change a rule or a mechanic.
Search the whole repository with whole-word and string-call searches, then move a shared rule to 1 helper. The full steps are in `references/enforcement-sites.md`.

## Find the cause of a changed measure

Use this when a measure today differs from a table that someone landed earlier. Run the old
tool on today's game first, then bisect extracted commits. The full steps are in `references/changed-measure.md`.

## Code health scan

Use this for a measured, ranked list of safe fixes before a cleanup: warnings, dead code,
unused signals, large functions, module coupling. The full steps are in `references/code-health-scan.md`.

## Review units and fix waves

Use this when a lead wants a read-only expert review of a build, and small fix units then
close the numbered findings. The full steps are in `references/review-and-fix-waves.md`.

## Render and feel bug isolation

Use this when something draws wrong, draws nothing or feels wrong, and the cause is unknown.
Probe outside the repository and change 1 thing at a time. The full steps are in `references/render-feel-isolation.md`.

## Pitfalls

The 8 most important rows. The full table is in `references/pitfalls.md`.

| Symptom | Cause | Fix |
| --- | --- | --- |
| Two base runs give, for example, 90 and 96 hits | Frame timing noise | Fix the frame rate, turn off delta smoothing, run the base twice to measure the spread |
| The seed differs after a change | A new start rule uses the random generator | Diff the telemetry to the first event that differs |
| A rewrite with cached trig changes scores slightly | Float precision | Reject it; keep only changes that are identical and win in paired timing |
| A later run is faster in unchanged code | Machine load drift | Report only back-to-back pairs |
| Tests outside your file list fail after a rename | They call the old name through `call()` | Search string calls first; keep private forwarders |
| `git grep -E '\bname\b'` finds nothing | `\b` is not portable | Use `git grep -w` |
| A rule fix leaves drift | The rule has several copies (logic, view, demo, tool) | Map all copies first; move the rule to 1 helper |
| A "dead" function is live | A string-built call reaches it | Search string calls and built names first |

## Artifacts to build

- **A trace tool:** a headless script that drives the real systems with a fixed seed and fixed
  ticks and writes 1 line per tick or per event. Write a SHA-256 file next to the output.
- **An event logger:** connects to the game signals that matter and writes 1 JSON line per event.
- **A first-difference tool:** compares 2 event logs and prints the first line that differs.
  `references/script-first-diff.md` holds one with an optional float tolerance; copy the block
  into a file.
- **A base-copy command:** the `git archive ... | tar -x -C <temp dir>` line plus the import
  step, written once in the project's dev notes.
- **A replay test:** loads a path or input recording made on the old commit and asserts the
  same result.
- **A mesh digest tool:** hashes the vertex, normal and UV arrays of each built mesh.
- **A save fixture test:** a save file written by the old code and a test that loads it.
- **A warning collector:** a standard-library client that talks to the Godot language server
  and writes 1 JSON line per diagnostic.
- **A metrics script:** function lines, branch counts, duplicates and magic numbers per script.
- **A signal audit script:** emitted versus connected signals.
- **A layout evidence script:** folder sizes, autoload references and calls between modules.
- **A search checklist:** 3 lines: whole-word search, string-call search, a pattern test on a
  known name.
- **A view-logic parity test shape:** every item, every profile, several save snapshots, view
  state equals logic answer.
- **A review report template:** summary, score table, findings with id, severity, file and
  line, stage reason, fix.
- **A fix notes format:** 1 row per finding id: change, test, result.
- **A probe template:** a script outside the repository that sets up 1 case, prints the
  effect state and captures or measures it.

## Related skills

- `godot-test-loop`: the test loop, proof that a regression test can fail, flaky tests and the
  full exit-leak hunt. This skill uses its tests as one half of the proof.
- `godot-headless-runs`: the flags and tool-script rules that a trace or probe run needs.
- `godot-bot-balance-loop`: deterministic bot fights. Its determinism flags are the same ones this
  skill uses for a trace.
- `godot-perf-measure`: timing A/B runs and frame-cost maps. This skill proves identity; that skill
  measures the gain.
- `godot-visual-verification`: real window captures and the pixel-diff noise floor for draw-code
  proofs.
- `godot-balance-model-data`: parity between a balance model and the game, when the reference is a
  model output.
- `godot-fleet-game-dev`: scope grants, blocked reports and the land protocol around fix units.

## References

- `references/script-first-diff.md`: read it when you compare 2 trace or event logs.
- `references/enforcement-sites.md`: read it before you delete, move or rename a name or a rule.
- `references/changed-measure.md`: read it when a measure differs from a landed table.
- `references/code-health-scan.md`: read it before a cleanup, for a ranked list of fixes.
- `references/review-and-fix-waves.md`: read it for a read-only review and its fix units.
- `references/render-feel-isolation.md`: read it when a draw or feel bug has no known cause.
- `references/pitfalls.md`: read it when a symptom is not in the short Pitfalls list.
