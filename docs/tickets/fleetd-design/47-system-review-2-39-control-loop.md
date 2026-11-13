# 47: Second systems review of the fleetd design: the control loop and crash safety

<!-- horch-ticket: 1 -->
**Persona:** primary `codex-reviewer`, fallback `architect-reviewer` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/system-review-39-control-loop-2.md && grep -q '^## Verdict' docs/specs/fleetd/system-review-39-control-loop-2.md && grep -q '^## Answers' docs/specs/fleetd/system-review-39-control-loop-2.md && grep -q '^## New findings' docs/specs/fleetd/system-review-39-control-loop-2.md && ! grep -q '—' docs/specs/fleetd/system-review-39-control-loop-2.md`
**Own:** `docs/specs/fleetd/system-review-39-control-loop-2.md`
**Do not touch:** `docs/specs/fleetd/design.md`, `ddd/`, `crates/`

## GOAL

The file `docs/specs/fleetd/system-review-39-control-loop-2.md` says for each finding of the first review of this lens if its failure scenario can still occur in the design, and gives 1 verdict.

## CONTEXT

- `docs/specs/fleetd/design.md` is the design of a daemon named `fleetd`. No code exists for it.
- A first reviewer read the design at commit `62337c29` with the lens "the control loop and crash safety". The report is `docs/specs/fleetd/system-review-39-control-loop.md`: verdict NOT SOUND, with the findings S39-1 and up.
- 4 other reviewers did the same with other lenses (the other files `docs/specs/fleetd/system-review-*.md`).
- The orchestrator decided each finding as amendments A23 to A31, in the section "Systems review round" of `docs/specs/fleetd/amendments.md`. The last table of that section gives the amendment for each finding.
- 3 fresh authors applied the amendments in tickets 44, 45 and 46.
- Earlier, a judge that read only a diff gave PASS 2 times on this design, and the whole-design review then found faults. So this ticket does not ask "is the amendment stated". It asks "can the failure still occur".
- You are a fresh session on purpose. You are not the first reviewer and not an author.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read `docs/specs/fleetd/system-review-39-control-loop.md`: each finding, and the section `## Answers`.
3. Read the section "Systems review round" of `docs/specs/fleetd/amendments.md`.
4. Read `docs/specs/fleetd/design.md` at `HEAD` from its first line to its last. Do not read only the diff.
5. For each finding S39-n of the first report: follow its failure scenario step by step against the design at `HEAD`. Decide `closed` (a rule of the design now stops the scenario; give the lines) or `open` (the scenario can still occur; say at which step and why). A rule that is stated in 1 section and contradicted in another section is `open`. For a finding with the rank LOW and the word "decision", write `decision` and do not judge it.
6. Run `git diff 62337c29 HEAD -- docs/specs/fleetd/design.md` and read the change. Look for a new fault of your lens that the change made: 2 sections that now contradict each other on a rule, a new state with no exit, a new table that no phase of the Migration plan adds, a new mechanism that an existing rule breaks. Each one needs a failure scenario.
7. Write `docs/specs/fleetd/system-review-39-control-loop-2.md` with these sections:
   - `## Verdict`: 1 line. `PASS` when each CRITICAL finding is `closed` and there is no new CRITICAL finding. Else `FAIL`. Then 1 line with the count of HIGH findings that are still `open`.
   - `## Answers`: 1 line for each finding of the first report: `- S39-n (rank) closed: <lines>` or `- S39-n (rank) open: <the step of the scenario that still occurs>` or `- S39-n decision`.
   - `## New findings`: each one with an id (`T39-1`, `T39-2`), its rank (CRITICAL or HIGH only), the lines, a quote of at most 2 lines, the failure scenario and the smallest change. Or the word "None".
8. Run the **Check** command of this ticket. It must exit 0.
9. Commit the file.

## CONSTRAINTS

- Judge the findings of the first report of your lens and the change since `62337c29`. Do not start a new review of the whole design.
- Give at most 4 new findings, of the ranks CRITICAL and HIGH only. Do not raise a new MEDIUM or LOW finding. This is the guard against gold-plating: the operator wants a design that is good enough to build.
- CRITICAL: the system as designed loses work, corrupts the ledger, stops for good, or cannot be built as written. HIGH: a builder must guess a rule, and a wrong guess causes a failure of the CRITICAL class. A finding needs a failure scenario and a line. A preference for another design is not a finding.
- The decisions are final: D1 to D16 and A23 to A31. Do not rank a problem CRITICAL or HIGH because you prefer another fix than the amendment. Judge only if the fix stops the failure.
- Do not edit a file other than the one in **Own**.
- Do not use the em dash. Use a plain dash.
- Never read, print or use `ANTHROPIC_API_KEY`.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- Each finding of the first report has 1 line in `## Answers`.

## REPORT

Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give the verdict, the count of findings that are closed and open for each rank, each finding that is still open in 1 sentence, and each new finding in 1 sentence.
