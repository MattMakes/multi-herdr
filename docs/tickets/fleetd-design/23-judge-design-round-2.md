# 23: Judge the round 2 change of the fleetd design

<!-- horch-ticket: 1 -->
**Persona:** primary `codex-reviewer`, fallback `architect-reviewer` · **Parallel:** no
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/design-review-3.md && grep -q '^## Verdict' docs/specs/fleetd/design-review-3.md && ! grep -q '—' docs/specs/fleetd/design-review-3.md`
**Own:** `docs/specs/fleetd/design-review-3.md`
**Do not touch:** `docs/specs/fleetd/design.md`, `ddd/`

## GOAL

The file `docs/specs/fleetd/design-review-3.md` says for each of A19, A20 and A21 if the design states it, ranks each problem of the change, and gives 1 verdict.

## CONTEXT

- `docs/specs/fleetd/design.md` is the design of a daemon named `fleetd`. It passed a review at commit `f6243ab2` and before.
- The operator then decided D16: the 178 existing OS-specific sites in production code move into the platform layer in 1 dedicated phase. A DDD run also found 2 small faults in the design.
- The orchestrator wrote the 3 changes as amendments A19, A20 and A21, in the section "Revision round 2" of `docs/specs/fleetd/amendments.md`.
- A fresh author applied them in ticket 22.
- The inventory of the OS-specific code is `docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6.
- You are a fresh session on purpose. You are not an author.
- The scope is small on purpose. "Good enough" passes. The operator does not want a gold-plated document.

## STEPS

1. Read the section "Revision round 2" of `docs/specs/fleetd/amendments.md`.
2. Run `git diff f6243ab2 HEAD -- docs/specs/fleetd/design.md`. Read the whole change.
3. For each of A19, A20 and A21: read the places that the amendment names, in the design at `HEAD`. Decide `stated` or `not stated`. An amendment is stated when the design says each item of it and no sentence of the design contradicts it.
4. Check A19 against the inventory. Each row of the table of section 6 must have a concern in the interface list of the design (PL-2 and the row "Platform layer"). Name each row that has none.
5. Check the order of the phases. Phase P must be able to pass its own gate with only what Phase 0 and Phase P give. Phase 1 must not need a thing that the change removed from it.
6. Search the design at `HEAD` for a sentence that the change made false: a sentence that still says that core fleetd code is the only scope of PL-2, that the platform layer lands in Phase 1, that the migration of the existing sites is not decided, or that a task has the state `landed`.
7. Rank each problem that you found: CRITICAL (the design cannot be built as written, or 2 sections contradict each other on a rule), HIGH (a real gap that a builder would have to guess), MEDIUM, LOW.
8. Write `docs/specs/fleetd/design-review-3.md` with these sections: `## Verdict` (1 line: `PASS` when there is no CRITICAL problem, else `FAIL`), `## Answers` (1 line for each: `- A<n> stated` or `- A<n> not stated: <why>`, with the line of the design), `## Findings` (each one with an id L1, L2, its rank, the line, a quote of at most 2 lines and 1 sentence; or the word "None").
9. Run the **Check** command of this ticket. It must exit 0.

## CONSTRAINTS

- Judge only A19 to A21 and the change since `f6243ab2`. Do not review the rest of the design again.
- Do not rank a problem CRITICAL or HIGH because you prefer another design. D16 and A19 to A21 are final. A problem of a decision itself goes in `## Findings` with the rank LOW and the word "decision".
- Give at most 5 findings of the ranks MEDIUM and LOW. This is the guard against gold-plating.
- Do not use the em dash. Use a plain dash.
- Do not edit a file other than the one in **Own**.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- Each of A19, A20 and A21 has 1 line in `## Answers`.
- Each finding has 1 rank.

## REPORT

Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give the verdict, the answer for each of A19 to A21, and each CRITICAL and HIGH finding in 1 sentence.
