# Questions, waits and handoffs

Read this file when you need a decision outside your plan, wait for another unit or a GO, hand off before compaction, or wait on a long run.

## Scope questions and blocks

Map every seam and out-of-scope file you need before you code. Then ask once per decision.

1. Send 1 QUESTION per decision through the message channel, not only a ledger note. Give
   the exact file and lines, 2 or more numbered options, and your recommendation.
2. Write the same question, with your measured numbers, in the unit notes file. A message
   can get lost; a notes file stays.
3. Ask for the smallest grant: 1 file or a line range. Prefer a generic seam (a signal, a
   method, a hook at the end of a shared function) over unit-specific code in a shared file.
4. Continue the independent steps. Restore any trial change the question depends on.
5. After approval, put generic seam lines on a separate seam branch from the newest base,
   with a test, so the steward lands them first.
6. Write the answer and its date in the notes file. Apply only the approved change. A
   standing rule covers the whole class; do not ask again.
7. For legacy tests that a planned change breaks: list each test with its cause, propose the
   smallest change per test, and adapt obsolete tests. Do not delete them, or coverage goes.
8. On a permission denial, stop. Do not retry in another form and do not route around it.
   Send BLOCKED with the command, the reason, and the choices: the owner runs the exact
   command, a narrow permission rule, or a skip.
9. When a plan step cannot pass, restore trial values, write the table, stop, and send
   options. Draft later steps that do not depend on it.
10. When 2 senders give different orders, hold the change and ask both for a ruling.

## Dependency waits

A planned wait for another unit's land is not a block.

1. Report `ready` and the state, not BLOCKED.
2. Check the base log for the merge subject of each dependency. Confirm the work landed with
   `git branch --merged` or `git show --stat`, not only a plan commit or a steward log line.
3. While you wait, read the plan, the code you will change, and the dependency's branch to
   learn its data shape. Do not build on an unlanded branch unless told to.
4. Do the work that does not depend on the land: independent tests, red tests for the gated
   step, the parts all options share.
5. Ask whether you can start with a stub (a constant table with the expected ids). Swap it
   for the real call after the land.
6. Wait without waste. Use a monitor that fires once when all dependencies show, or end your
   turn and let the orchestrator's notice arrive. Do not poll `git log` with `sleep`.
7. When the dependency lands, merge the base, import, run the suite, and rebuild dependent
   fixtures and captures from the new data.
8. If both land orders are possible, gate the new behaviour on the presence of the
   dependency's data at run time. Test both branches.

## Phase-gated delivery

Some units have phases that a lead or the orchestrator reviews before GO.

1. Finish the phase with its tests and commit it. Report the commit, test counts, cost
   numbers and captures.
2. Apply review fixes first.
3. While you wait, prepare the next phase as an uncommitted draft. Write needed out-of-scope
   files into the notes. Make no commit to shared files.
4. Do the no-wait items: captures, a 1-command script that rebuilds your table, findings that
   other units need. Do not idle after READY.
5. On a partial GO, commit only the approved files with their tests. Run the tests again on
   that partial commit.
6. Mark each part with a DONE-PART or READY line.
7. On a CULL order (a fresh worker replaces you), write the handoff, commit it and close.

## Compaction handoff

A long unit outlives one context. Hand off with no loss.

1. Finish the current step and its check. Do not stop inside a run or a timing batch. If a
   run must go on, note its pid and log path.
2. Commit the current state, so the handoff points at a commit. Copy helper scripts from
   `/tmp` into the unit's review folder, because `/tmp` does not survive.
3. Write the handoff: role, plan path, branch and head, done steps, files and their state,
   trials and results, decisions, rules received, open questions with answers, the next step.
4. Commit it, send a note with its path, send COMPACT-READY, and end the turn.
5. A message that arrives during the wait (an answer, a GO, a new rule) goes into the
   handoff at once. Acknowledge it; do not start it.
6. After compaction, read the handoff, the full plan and any addendum. Act on the union of
   the handoff and the resume message. Check `git status`, the last commit and live
   processes of your role.
7. Continue from the next step only. Never repeat done steps; a repeated run can double a
   table or a commit.

The orchestrator side (run the compact at once, wait on new log lines only) is in
[fleet-roles.md](fleet-roles.md#compaction-orchestrator-side).

## Long command waits

Bot sweeps, long batches and full suites often run longer than 1 tool call.

1. Time 1 smoke call. Estimate the wall time of the batch.
2. Split seeds into chunks that end well inside the call limit (for example 2 or 3 seeds per
   call), or start the batch as a tracked background job.
3. Give each Godot call a hard alarm below the limit, inside the slot runner.
4. Write each chunk to its own log with the seed range in the name. Keep resumable state.
5. Wait for the harness notice, or a monitor on an end marker: an exit-code file, a summary
   line, or no matching process. Anchor the monitor pattern, or every log line wakes you.
6. Do work that needs no Godot run while you wait. Start no second Godot process.
7. On the notice, check that the real job ended, not only a wrapper. Grep the logs for
   script errors and the summary line before you trust the result.
8. Re-arm a monitor that expires without an end event. It is not a failure.
