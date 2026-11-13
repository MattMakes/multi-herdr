# 52: Fix the fleetd design after the second systems review (A32 to A36)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** no
**Kind:** docs
**Review:** none
**Check:** `grep -q 'worktree_add' docs/specs/fleetd/design.md && grep -q 'docs_sync' docs/specs/fleetd/design.md && grep -q 'horch branch abandon' docs/specs/fleetd/design.md && grep -q -i 'operator token' docs/specs/fleetd/design.md && grep -q 'no sandbox' docs/specs/fleetd/design.md && grep -q 'A32' docs/specs/fleetd/amendments.md && grep -q 'A32' docs/specs/fleetd/design.md && ! grep -q '—' docs/specs/fleetd/design.md && ! grep -q '—' docs/specs/fleetd/amendments.md`
**Own:** `docs/specs/fleetd/design.md`, `docs/specs/fleetd/amendments.md`
**Do not touch:** `ddd/`, each other file in `docs/specs/fleetd/`, `crates/`

## GOAL

`docs/specs/fleetd/design.md` states amendments A32 to A36, so each finding of the second systems review no longer applies.

## CONTEXT

- `docs/specs/fleetd/design.md` is the design of a daemon named `fleetd`. It has about 1,700 lines. No code exists for it.
- 5 reviewers read the whole design 2 times. After the first time, tickets 44 to 46 applied amendments A23 to A31. The second reports are the 5 files `docs/specs/fleetd/system-review-*-2.md`. They close almost each first finding, and they find new gaps that the fixes opened: 4 CRITICAL and some HIGH.
- The orchestrator decided each one as amendments A32 to A36, in the section "Systems review round, fix round 2" at the end of `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/amendments.md`. That section is your instruction. Its last table gives the amendment for each finding.
- This is the last fix round. After you, 5 fresh reviewers check for each finding that its failure scenario cannot occur any more. What they still find open goes to the operator. So a sentence that names a rule is not enough: the mechanism must be in each section that touches it, and no old sentence can contradict it.
- The first fix round opened new gaps because a new state or a new table had no exit, no phase or no owner. For each thing that you add, ask: who owns it, what leaves it, which phase of the Migration plan adds it, and which gate tests it.
- The decisions file is `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/decisions.md`.
- You are a fresh session on purpose. You are not an author of the design and not a reviewer.
- The connection to GitHub is not stable today. If the push in `horch ledger finish` fails with a network error, run `horch ledger finish` again, up to 3 times.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Copy `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/amendments.md` to `docs/specs/fleetd/amendments.md`. Change nothing in it. Run `git diff --stat`: the change must only add lines at the end of the file. Commit.
3. Read "Terms to use everywhere" at the top of the amendments file, the section "Systems review round" (A23 to A31, so that you know the mechanisms that exist), and the section "Systems review round, fix round 2" (A32 to A36).
4. For each finding in the last table of that section, read it in its `-2` report: the lines, the failure scenario and the smallest change. Read the design from its first line to its last 1 time.
5. Apply A32. Commit.
6. Apply A33. Commit.
7. Apply A34. Commit.
8. Apply A35. Commit.
9. Apply A36. Commit.
10. For each thing that steps 5 to 9 added (a job kind, a state, a column, a table, a command, a requirement, a test): find its row in the tables "What each gate needs", "Where each rule becomes green", "New work for each phase" and "The numbers of the design" of the Migration plan, and add or correct the row. Commit.
11. For each finding of the last table, follow its failure scenario step by step against the design as it is now. If the scenario can still occur, correct the design. Commit.
12. Search the design for a sentence that your change made false: an old sentence in another section that still states the old rule. Search for each term that you changed. Correct each one. Commit.
13. In the `## Amendments` table of the design, add 1 row "Systems review round, fix round 2: A32 to A36" with the sections that you changed. Commit.
14. Run the **Check** command of this ticket. It must exit 0.
15. Run `git diff c8bf35ac HEAD --stat -- docs/specs/fleetd/design.md docs/specs/fleetd/amendments.md`. Then run `git status --porcelain`. No other file is changed by you.

## CONSTRAINTS

- Change only what A32 to A36 and steps 10 to 13 need. Do not improve any other part of the design.
- An amendment gives the rule. You choose the wording, the column names and the place. When an amendment leaves a gap that changes behavior, or contradicts a decision D1 to D16 or an amendment A1 to A31, send a `QUESTION:` to the orchestrator and end your turn. Do not add a design idea of your own.
- Do not add a state, a table or a job kind that the amendments do not name. If you think 1 is needed, send a `QUESTION:`.
- A decision of the operator (D1 to D4, D6 to D14, D16) does not change.
- Do not renumber a phase, a requirement, a failure mode or an invariant. A new one gets the next free number of its group.
- Keep the voice of the design. It speaks to the operator as "you". Write short sentences in the active voice.
- Do not write the value of an environment variable or of a token. Never read, print or use `ANTHROPIC_API_KEY`.
- Do not use the em dash.
- Do not edit a file other than the 2 in **Own**.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- No effect in part 3 of the reconcile pass reads or writes a working tree or uses the network.
- The `jobs` table has 1 table of kinds with the class and the input key of each.
- Each bundle state, each landing state, each task state and each feature state has an exit, and each new thing has a phase and a gate.
- For each finding, your report says which lines now stop its failure scenario.

## REPORT

Record 1 progress note for each amendment. Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give for each of A32 to A36 the sections and lines that you changed; for each finding, 1 sentence on what now stops its scenario; and each wording, column or command name that you chose and the amendment did not give.
