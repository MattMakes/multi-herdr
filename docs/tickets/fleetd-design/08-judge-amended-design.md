# 08: Judge the amended fleetd design

<!-- horch-ticket: 1 -->
**Persona:** primary `codex-reviewer`, fallback `architect-reviewer` · **Parallel:** no
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/design-review.md && grep -q '^## Verdict' docs/specs/fleetd/design-review.md`
**Own:** `docs/specs/fleetd/design-review.md`

## GOAL

The file `docs/specs/fleetd/design-review.md` ranks each miss of `docs/specs/fleetd/design.md` against the amendments as CRITICAL, HIGH, MEDIUM or LOW, and gives 1 verdict.

## CONTEXT

- `docs/specs/fleetd/design.md` is a design document for a daemon named `fleetd`. Another agent wrote it in ticket 07. It is the source design with 14 amendment groups applied.
- The amendments are in `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/amendments.md`.
- The decisions are in `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/decisions.md`.
- The source design is `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/source-design.md`.
- You judge a document, not code. You are a different session from the author on purpose.
- "Good enough" passes. The operator does not want a gold-plated document. Do not ask for more detail, better wording or more examples.

## STEPS

1. Read the amendments file. Read the decisions file.
2. Read `docs/specs/fleetd/design.md` from the first line to the last.
3. For each amendment A0 to A14, check that the document states each item of the amendment. Check each "Places" entry of the amendment.
4. Search the document for statements that contradict an amendment. Check these with care: the summary, the requirement tables, the ledger schema, the idea state table, the failure mode table, the migration phases, the examples.
5. Run `git diff` between the first commit of `docs/specs/fleetd/design.md` (the copy of the source) and the tip. Check that the author did not remove or change a part of the source that no amendment covers.
6. Check that the `## Preservation analysis` section holds the rows that amendment A12 says must be present.
7. Rank each miss with this scale:
   - CRITICAL: a builder who follows the document builds the wrong thing, loses work, or breaks a security rule. Examples: a decision is absent; 2 sections contradict each other on a state or an owner; a no-loss invariant is absent; the `ANTHROPIC_API_KEY` rule is weaker than amendment A12.
   - HIGH: a decision is present but a builder can read it in 2 ways, or 1 "Places" entry still has the old text.
   - MEDIUM: a term is not the one in "Terms to use everywhere", or a cross-reference is wrong.
   - LOW: wording, order, format.
8. Write `docs/specs/fleetd/design-review.md` with these sections: `## Verdict` (1 line: `PASS` when there is no CRITICAL finding, else `FAIL`, then the count for each rank), `## CRITICAL`, `## HIGH`, `## MEDIUM`, `## LOW`. Each finding has: an id J1, J2 and so on, the amendment id, the line of the document, a quote of at most 2 lines, and 1 sentence that says what is wrong. Do not write how to fix it in more than 1 sentence.
9. Run the **Check** command of this ticket. It must exit 0.

## CONSTRAINTS

- Do not edit `docs/specs/fleetd/design.md`. You judge it. You do not fix it.
- Do not rank a miss CRITICAL because you prefer another design. The decisions are final.
- Do not edit a file other than the one in **Own**.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- Each amendment A0 to A14 has a line in the review: a finding, or "A<n>: no finding".

## REPORT

Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give the verdict, the count for each rank, and each CRITICAL finding in 1 sentence.
