# 34: Fix finding L1 of the round 2 design review (A22)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** no
**Kind:** docs
**Review:** none
**Check:** `grep -q 'expected failures' docs/specs/fleetd/design.md && grep -q 'A22' docs/specs/fleetd/design.md && grep -q 'A22' docs/specs/fleetd/amendments.md && ! grep -q 'made 13 decisions' docs/specs/fleetd/design.md && ! grep -q '—' docs/specs/fleetd/design.md && ! grep -q '—' docs/specs/fleetd/amendments.md`
**Own:** `docs/specs/fleetd/design.md`, `docs/specs/fleetd/amendments.md`
**Do not touch:** `ddd/`, each other file in `docs/specs/fleetd/`

## GOAL

`docs/specs/fleetd/design.md` states amendment A22, so finding L1 of the review no longer applies.

## CONTEXT

- `docs/specs/fleetd/design.md` is the design of a daemon named `fleetd`.
- Ticket 22 applied amendments A19 to A21 to it. A judge reviewed that change in ticket 23. The review is `docs/specs/fleetd/design-review-3.md`: verdict FAIL, 1 CRITICAL finding (L1) and 1 LOW finding (L2).
- The orchestrator decided both findings as amendment A22. It is in the section "Fix round 2" at the end of `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/amendments.md`. That section is your instruction.
- `docs/specs/fleetd/amendments.md` on your branch is an older copy of that file. It does not have the section "Fix round 2". Step 2 updates it.
- You are a fresh session on purpose. You are not the author and not the judge.
- Ticket 35 is next. A fresh judge checks only L1 and your change.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Copy `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/amendments.md` to `docs/specs/fleetd/amendments.md`. Change nothing in it. Run `git diff --stat`: the change must only add lines at the end of the file. Commit.
3. Read the section "Fix round 2" of that file. Read `docs/specs/fleetd/design-review-3.md`.
4. Apply A22. Search the design for `PL-6`, `Phase 0`, `Phase P`, `red test`, `fault-injection tests pass` and `13 decisions`. Make each place agree with A22. Commit.
5. In the `## Amendments` table of the design, add 1 row "Fix round 2: A22 (L1, L2)" with the sections that you changed. Commit.
6. Run the **Check** command of this ticket. It must exit 0.
7. Run `git diff a61d1f61 HEAD --stat -- docs/specs/fleetd/design.md docs/specs/fleetd/amendments.md` and `git diff a61d1f61 HEAD --stat`. The second must name only the 2 files in **Own** and `docs/specs/fleetd/design-review-3.md`.

## CONSTRAINTS

- Change only what A22 and step 5 need. Do not improve any other part of the design.
- Do not change the gate of a phase other than Phase P. Do not renumber a phase, a requirement or a failure mode.
- Keep the voice of the design. It speaks to the operator as "you". Write short sentences in the active voice.
- Do not add a design idea of your own. If A22 leaves a gap or contradicts the design, send a `QUESTION:` to the orchestrator and end your turn.
- Do not use the em dash.
- Do not edit a file other than the 2 in **Own**.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- The test of PL-6 compares the result of each test before the move and after it.
- Phase 0 names the list of expected failures.
- The first sentence of the Amendments section gives the count that the table shows.

## REPORT

Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give the lines of the design that you changed for L1 and for L2, and the count of decisions that you wrote with the rows that you counted.
