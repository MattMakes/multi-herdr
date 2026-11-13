# 59: Judge the closing fix of the fleetd design

<!-- horch-ticket: 1 -->
**Persona:** primary `codex-reviewer`, fallback `architect-reviewer` · **Parallel:** no
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/design-review-5.md && grep -q '^## Verdict' docs/specs/fleetd/design-review-5.md && grep -q '^## Answers' docs/specs/fleetd/design-review-5.md && ! grep -q '—' docs/specs/fleetd/design-review-5.md`
**Own:** `docs/specs/fleetd/design-review-5.md`
**Do not touch:** `docs/specs/fleetd/design.md`, `ddd/`, `crates/`

## GOAL

The file `docs/specs/fleetd/design-review-5.md` says for each of the findings S42-2, T42-1 and T43-2 if its failure scenario can still occur in the design, and gives 1 verdict.

## CONTEXT

- `docs/specs/fleetd/design.md` is the design of a daemon named `fleetd`. No code exists for it.
- 5 reviewers read it 3 times. The third reports (`docs/specs/fleetd/system-review-*-3.md`) each have the verdict PASS, with 3 HIGH findings open: S42-2 (in `system-review-42-migration-and-requirements-3.md`), T42-1 (same file) and T43-2 (in `system-review-43-operations-and-agreement-3.md`).
- The orchestrator decided them as amendment A37, in the section "Systems review round, closing fix" of `docs/specs/fleetd/amendments.md`. A fresh author applied it in ticket 58.
- This is the last check of the design. "Good enough to build" passes.
- You are a fresh session on purpose.
- The connection to GitHub is not always stable. If the push in `horch ledger finish` fails with a network error, run `horch ledger finish` again, up to 3 times.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read the 3 findings in the 2 reports. Read the section "Systems review round, closing fix" of `docs/specs/fleetd/amendments.md`.
3. Run `git diff 5327458b HEAD -- docs/specs/fleetd/design.md` and read the whole change.
4. In the design at `HEAD`, search for `sandbox`, `operator.token`, `06 S9`, `Single writer`, `read-only` and `only ledger writer`, and read each place.
5. For each of S42-2, T42-1 and T43-2: follow its failure scenario step by step against the design at `HEAD`. Decide `closed` (give the lines) or `open` (say at which step it still occurs).
6. Check the change of step 3 for a new CRITICAL fault that the change itself made: 2 sections that now contradict each other on a rule, or a thing that cannot be built as written.
7. Write `docs/specs/fleetd/design-review-5.md` with these sections: `## Verdict` (1 line: `PASS` when each of the 3 findings is `closed` and there is no new CRITICAL, else `FAIL`), `## Answers` (1 line for each finding: `- <id> closed: <lines>` or `- <id> open: <the step>`), `## New CRITICAL` (each one with an id V1, V2, the lines, a quote of at most 2 lines, the failure scenario; or the word "None").
8. Run the **Check** command of this ticket. It must exit 0.
9. Commit the file.

## CONSTRAINTS

- Judge only the 3 findings and the change since `5327458b`. Do not review the rest of the design again.
- Do not raise a new HIGH, MEDIUM or LOW finding. This is the guard against gold-plating.
- The decisions are final: D1 to D16 and A1 to A37. That no worker runs without a sandbox is a decision (preservation row 06 S9). Do not call it a fault that a CLI has no worker on an OS.
- Do not edit a file other than the one in **Own**.
- Do not use the em dash. Use a plain dash.
- Never read, print or use `ANTHROPIC_API_KEY`.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- Each of the 3 findings has 1 line in `## Answers`.

## REPORT

Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give the verdict, the answer for each of the 3 findings, and each new CRITICAL in 1 sentence.
