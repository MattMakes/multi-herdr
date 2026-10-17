# Review units and fix waves

Part of `godot-safe-refactor`. Target engine: **Godot 4.7**.

Use this when a lead wants a read-only expert review of a build (code, UI, performance, QA,
systems, balance, product), and small fix units then close the findings.

1. Name the rubric and its references in the review plan. The reviewer works read-only at a
   named commit. A review of a moving tree cites files that are not there.
2. Run the import and the full suite first. Record the counts.
3. Judge the build at its stage, for example a prototype. Keep component quality apart from
   integration readiness. Passing component tests with a placeholder main scene is its own
   finding.
4. Read the core systems, the UI and the tests against the contracts. For a product review,
   play the build with an autoplay harness as a fresh player and as a mid-game player. Capture
   each UI screen.
5. Confirm each suspected bug with a probe outside the repository. Trace each surprising number
   to its code.
6. Send a cause that a running unit needs to the lead at once. Do not hold it for the report.
7. Write the report: a 5-line summary, a score table, then findings ranked blocker, major,
   minor. Each finding has an id (for example `RS-07`), the file and line, why it matters at
   this stage, and a fix small enough for 1 worker. Quote the rubric rule text.
8. Check every line citation and the id count. Report the count per severity. Do not rank a
   style nit as major.
9. The lead gives each fix unit a short list of ids and a file list. A finding that shows a
   plan step is wrong overrides the plan, and the lead says so.
10. The fix worker writes 1 failing regression test per finding, fixes it and runs the focused
    tests. See `godot-test-loop` for proof that a test can fail.
11. When a fix makes a test outside the worker's scope stale, the worker reports it as blocked,
    with the line, and waits for a grant.
12. The worker runs the full suite and checks that only its files changed. It writes a notes
    file that maps each id to the change, the test and the result. Its report lists each id as
    closed.
