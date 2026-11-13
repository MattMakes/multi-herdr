# 57: Third and last systems review of the fleetd design: operations, trust, and the agreement of the documents

<!-- horch-ticket: 1 -->
**Persona:** primary `codex-reviewer`, fallback `architect-reviewer` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/system-review-43-operations-and-agreement-3.md && grep -q '^## Verdict' docs/specs/fleetd/system-review-43-operations-and-agreement-3.md && grep -q '^## Answers' docs/specs/fleetd/system-review-43-operations-and-agreement-3.md && grep -q '^## New CRITICAL' docs/specs/fleetd/system-review-43-operations-and-agreement-3.md && ! grep -q '—' docs/specs/fleetd/system-review-43-operations-and-agreement-3.md`
**Own:** `docs/specs/fleetd/system-review-43-operations-and-agreement-3.md`
**Do not touch:** `docs/specs/fleetd/design.md`, `ddd/`, `crates/`

## GOAL

The file `docs/specs/fleetd/system-review-43-operations-and-agreement-3.md` says for each of the findings T43-1, T43-2, T43-3 if its failure scenario can still occur in the design, and gives 1 verdict.

## CONTEXT

- `docs/specs/fleetd/design.md` is the design of a daemon named `fleetd`. No code exists for it.
- 5 reviewers read the design 2 times, 1 lens each. Your lens is: operations, trust, and the agreement of the documents. The second report of your lens is `docs/specs/fleetd/system-review-43-operations-and-agreement-2.md`. It left these findings open or new: T43-1, T43-2, T43-3.
- The orchestrator decided each one as amendments A32 to A36, in the section "Systems review round, fix round 2" of `docs/specs/fleetd/amendments.md`.
- A fresh author applied them in ticket 52.
- This is the last round. A finding that you find open goes to the operator, who decides. So be exact: say at which step the scenario still occurs.
- The operator wants a design that is good enough to build. A design of this size always has more small gaps. Your task is not to find them.
- You are a fresh session on purpose. You are not an earlier reviewer and not an author.
- The connection to GitHub is not stable today. If the push in `horch ledger finish` fails with a network error, run `horch ledger finish` again, up to 3 times.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read in `docs/specs/fleetd/system-review-43-operations-and-agreement-2.md` each of the findings T43-1, T43-2, T43-3: the lines, the failure scenario, the smallest change.
3. Read the section "Systems review round, fix round 2" of `docs/specs/fleetd/amendments.md`.
4. Read `docs/specs/fleetd/design.md` at `HEAD` from its first line to its last.
5. For each of the findings T43-1, T43-2, T43-3: follow its failure scenario step by step against the design at `HEAD`. Decide `closed` (a rule of the design now stops the scenario; give the lines) or `open` (say at which step the scenario still occurs, and why). A rule that 1 section states and another section contradicts is `open`.
6. Run `git diff c8bf35ac HEAD -- docs/specs/fleetd/design.md` and read the change. Look for a new CRITICAL fault of your lens that this change made: the system as designed loses work, corrupts the ledger, stops for good, or cannot be built as written. It needs a failure scenario and a line.
7. Write `docs/specs/fleetd/system-review-43-operations-and-agreement-3.md` with these sections:
   - `## Verdict`: 1 line. `PASS` when each finding of step 5 that has the rank CRITICAL is `closed` and there is no new CRITICAL. Else `FAIL`. Then 1 line with the ids of the HIGH findings that are still `open`, or "No HIGH finding is open".
   - `## Answers`: 1 line for each finding: `- <id> (rank) closed: <lines>` or `- <id> (rank) open: <the step that still occurs>`.
   - `## New CRITICAL`: each one with an id (`U43-1`), the lines, a quote of at most 2 lines, the failure scenario and the smallest change. Or the word "None".
8. Run the **Check** command of this ticket. It must exit 0.
9. Commit the file.

## CONSTRAINTS

- Judge only the findings T43-1, T43-2, T43-3 and the change since `c8bf35ac`. Do not review the rest of the design again.
- Give at most 2 new findings, of the rank CRITICAL only. Do not raise a new HIGH, MEDIUM or LOW finding. This is the guard against gold-plating.
- The decisions are final: D1 to D16 and A1 to A36. Do not call a finding open because you prefer another fix than the amendment. Judge only if the fix stops the failure.
- Do not edit a file other than the one in **Own**.
- Do not use the em dash. Use a plain dash.
- Never read, print or use `ANTHROPIC_API_KEY`.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- Each of the findings T43-1, T43-2, T43-3 has 1 line in `## Answers`.

## REPORT

Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give the verdict, each finding that is still open in 1 sentence, and each new CRITICAL in 1 sentence.
