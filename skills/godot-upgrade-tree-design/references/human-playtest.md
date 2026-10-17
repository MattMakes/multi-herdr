# Human playtest loop

This file belongs to the `godot-upgrade-tree-design` skill. The terms (diag tree, chapter, class, policy) are defined in its SKILL.md.

Use this when people must play a build to answer design questions, and their play must be compared with the bots.

1. Map each playtest question to 1 observable and 1 pass mark. A question that telemetry cannot prove becomes an
   observer check with a Y/N column per tester on the same sheet.
2. Write a telemetry logger that writes 1 JSON line per game event and fails safe. It is off in the test runner.
   Write its tests first: disabled writes no file; each event writes 1 line.
3. Mark each trace as human or bot. Else the report shows bot fields (skill, seed) for a person.
4. Write a report script with tests. It answers each question per tester, not per session, and prints the observer
   checklist. Use the same field names in the protocol document.
5. Before the full loop exists, write an analytical 1-run model from the landed formulas, list its assumptions, and
   compare it with the design target. It answers in seconds where a full simulation runs for minutes.
6. Add a play command that starts the dev mode on its own save and trace path, with a "new" option that moves the old
   save aside. On quit it copies the trace to a dated folder and prints the player's numbers beside the bot baseline.
7. Add a build command that exports each desktop platform with a tester README that says where the trace is.
8. Write the protocol: no minimum play time, allowed and forbidden observer lines, interview questions about this
   build only, the report command, the pass marks, how to collect saves and logs per platform, a 1-page observer
   sheet. Record the run at which a tester stops and the last new find.
9. Before each session, measure the test machine's p99 frame time.
10. Before release to testers, run a readiness gate: import, full suite, report tests, the balance simulator, an
    export of every platform with sizes, and a headless launch of each export. Give a verdict with blocking items.
    Run it again if the branch moves.
11. For a recorded session, extract frames every few seconds, tile them into contact sheets, read them, run the
    report on the log, and compare with the model on the same inputs.
