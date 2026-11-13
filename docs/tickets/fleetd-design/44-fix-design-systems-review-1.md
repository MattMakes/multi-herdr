# 44: Fix the fleetd design after the systems review, 1 of 3: the control loop, effects, the fleet clock and the launcher

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** no
**Kind:** docs
**Review:** none
**Check:** `grep -q 'CREATE TABLE jobs' docs/specs/fleetd/design.md && grep -q 'CREATE TABLE requests' docs/specs/fleetd/design.md && grep -q 'fleet clock' docs/specs/fleetd/design.md && grep -q 'request_id' docs/specs/fleetd/design.md && grep -q 'launcher' docs/specs/fleetd/design.md && grep -q 'A23' docs/specs/fleetd/amendments.md && ! grep -q '—' docs/specs/fleetd/design.md && ! grep -q '—' docs/specs/fleetd/amendments.md`
**Own:** `docs/specs/fleetd/design.md`, `docs/specs/fleetd/amendments.md`
**Do not touch:** `ddd/`, each other file in `docs/specs/fleetd/`, `crates/`

## GOAL

`docs/specs/fleetd/design.md` states amendments A23, A24, A25 and A29, so the findings that those amendments answer no longer apply.

## CONTEXT

- `docs/specs/fleetd/design.md` is the design of a daemon named `fleetd`. It has about 1,300 lines. No code exists for it.
- 5 reviewers read the whole design, 1 lens each. Each verdict is NOT SOUND. The reports are the 5 files `docs/specs/fleetd/system-review-*.md`.
- The orchestrator decided each finding. The decisions are amendments A23 to A31, in the section "Systems review round" at the end of `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/amendments.md`. That section is your instruction. Its last table gives the amendment for each finding.
- This is fix 1 of 3. Tickets 45 and 46 apply A26 to A31 after you. Do not apply them.
- The 3 fix tickets change the same file, so they run one after the other.
- After ticket 46, 5 fresh reviewers check for each finding that its failure scenario cannot occur in the design any more. So a sentence that names a rule is not enough: the mechanism must be in the design, in each section that touches it.
- The decisions file is `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/decisions.md`.
- You are a fresh session on purpose. You are not an author of the design and not a reviewer.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Copy `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/amendments.md` to `docs/specs/fleetd/amendments.md`. Change nothing in it. Run `git diff --stat`: the change must only add lines at the end of the file. Commit.
3. Read "Terms to use everywhere" at the top of the amendments file, the 4 rules at the top of the section "Systems review round", and your amendments (A23, A24, A25 and A29). Read the last table of the section and note the findings of your amendments.
4. For each of those findings, read it in its report: the lines, the failure scenario and the smallest change. Read the section `## Checked and sound` of each report that has 1 of your findings: do not change what it names. Read the design from its first line to its last 1 time.
5. Apply A23. Go to each place that the amendment names. Search the design for the terms of the amendment, and make each place agree with it. Commit.
6. Apply A24. Go to each place that the amendment names. Search the design for the terms of the amendment, and make each place agree with it. Commit.
7. Apply A25. Go to each place that the amendment names. Search the design for the terms of the amendment, and make each place agree with it. Commit.
8. Apply A29. Go to each place that the amendment names. Search the design for the terms of the amendment, and make each place agree with it. Commit.
9. For each finding of your amendments, read its failure scenario again against the design as it is now. Follow the scenario step by step. If the scenario can still occur, the amendment is not applied in full: correct the design. Commit.
10. Search the design for a sentence that your change made false: an old sentence in another section that still states the old rule. Correct each one. Commit.
11. Run the **Check** command of this ticket. It must exit 0.
12. Run `git diff 62337c29 HEAD --stat`. It must name only the 2 files in **Own**.

## CONSTRAINTS

- Change only what your amendments need. Do not improve any other part of the design. Do not apply an amendment of another ticket.
- An amendment gives the rule. You choose the wording, the column names and the place. When an amendment leaves a gap that changes behavior, or contradicts a decision D1 to D16, send a `QUESTION:` to the orchestrator and end your turn. Do not add a design idea of your own.
- A decision of the operator (D1 to D4, D6 to D14, D16) does not change.
- Do not renumber a phase, a requirement, a failure mode or an invariant. A new one gets the next free number of its group.
- Keep the voice of the design. It speaks to the operator as "you". Write short sentences in the active voice. A new state table uses the columns of the worktree state table.
- Do not write the value of an environment variable. Never read, print or use `ANTHROPIC_API_KEY`.
- Do not use the em dash.
- Do not edit a file other than the 2 in **Own**.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- No effect (a process start, a file write, a git ref move) is inside the ledger transaction in the reconcile pass.
- The reconcile pass shows 3 parts, and the stop of orphans comes before `dispatch`.
- No gate, merge, rebase or network call runs inside a pass.
- For each finding of your amendments, your report says which lines now stop its failure scenario.

## REPORT

Record 1 progress note for each amendment. Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give for each of your amendments the sections and lines of the design that you changed; for each finding, 1 sentence on what now stops its scenario; each wording or column name that you chose and the amendment did not give; and each place that the next ticket must know.
