# 09: Fix the 4 findings of the design review

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** no
**Kind:** docs
**Review:** none
**Check:** `grep -q 'horch fleetd check' docs/specs/fleetd/design.md && grep -q 'kind: judge' docs/specs/fleetd/design.md && grep -q 'D14' docs/specs/fleetd/design.md && ! grep -q '—' docs/specs/fleetd/design.md && ! grep -q '—' docs/specs/fleetd/design-review.md`
**Own:** `docs/specs/fleetd/design.md`, `docs/specs/fleetd/design-review.md`

## GOAL

`docs/specs/fleetd/design.md` states amendments A15 to A18, so findings J1 to J4 of the review no longer apply.

## CONTEXT

- `docs/specs/fleetd/design.md` is the design of a daemon named `fleetd`. Another agent wrote it in ticket 07 from the amendments A0 to A14.
- A judge in another session reviewed it in ticket 08. The review is `docs/specs/fleetd/design-review.md`: 1 CRITICAL finding (J1) and 3 HIGH findings (J2, J3, J4).
- The orchestrator decided each finding. The decisions are amendments A15 to A18, in the section "Fix round 1" at the end of `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/amendments.md`. That section is your instruction.
- The decisions file is `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/decisions.md`.
- You are a fresh session on purpose. You are not the author and not the judge.
- Ticket 10 is next. A fresh judge checks only J1 to J4 and your change.
- Line numbers in A15 to A18 are lines of the design at commit `0505712d`. Find each place by its text.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read the section "Fix round 1" of the amendments file. Read `docs/specs/fleetd/design-review.md`.
3. Read "Terms to use everywhere" at the top of the amendments file.
4. Apply A15. Search the design for `hang`, `hung`, `WatchdogSec`, `watchdog` and `not supported`. Make each place agree with A15. Commit.
5. Apply A16. Search for `--direct`, `ticket title` and `branch name`. Make each place agree with A16. Commit.
6. Apply A17. Search for `--direct` and `1 task`. Make each place agree with A17. Commit.
7. Apply A18. Search for `judge`, `judges:` and `kind:`. Make each place agree with A18. Commit.
8. In the `## Amendments` table of the design, add 1 row "D14: the DDD run does all 9 steps as an auto draft" with "no section changed", and 1 row "Fix round 1: A15 to A18 (J1 to J4)" with the sections that you changed. Commit.
9. In `docs/specs/fleetd/design-review.md`, replace each em dash character with a plain dash `-`. Change nothing else in that file. Commit.
10. Run the **Check** command of this ticket. It must exit 0.

## CONSTRAINTS

- Change only what A15 to A18 and steps 8 and 9 need. Do not improve any other part of the design.
- Keep the voice of the design. It speaks to the operator as "you".
- Do not add a design idea of your own. If A15 to A18 leave a gap, send a `QUESTION:` to the orchestrator and end your turn.
- Do not use the em dash.
- Do not edit a file other than the 2 in **Own**.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- No sentence of the design says that hang detection is absent on macOS or Windows.
- Each statement about a `--direct` feature branch and a `--direct` plan agrees with A16 and A17.
- The judge task in each plan example has `kind: judge`, and the pairing rule names the judgment kinds only.

## REPORT

Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give for each of J1 to J4 the lines of the design that you changed.
