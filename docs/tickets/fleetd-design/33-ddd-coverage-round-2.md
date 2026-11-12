# 33: DDD coverage summary, review page and final render, round 2

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** no
**Kind:** docs
**Review:** none
**Check:** `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd && node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs contracts check ddd && test -s ddd/COVERAGE.md && test -s ddd/review.html && test -s ddd/diagrams/final.html && ! grep -q '—' ddd/COVERAGE.md`
**Own:** `ddd/`
**Do not touch:** `crates/`, `docs/specs/fleetd/`, `/Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0`

## GOAL

The revised workspace `ddd/` validates as a whole with no stale step, `ddd/diagrams/final.html` exists, and `ddd/COVERAGE.md` tells the operator what round 2 decided and what is still open.

## CONTEXT

- Read `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/ddd-context.md` first, then `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/ddd-revision-context.md`.
- Tickets 24 to 32 revised each of the 9 DDD steps. `ddd/COVERAGE.md` and `ddd/review.html` are still those of the first draft (ticket 20).
- Your procedure is "Step 4" and "Step 5" of `/Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/skills/ddd/SKILL.md`, and the template `/Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/skills/ddd/references/coverage-summary.md`.
- In the first draft, the render of `ddd/diagrams/final.html` failed. Ticket 21 found the cause. The report is `docs/specs/fleetd/blueprint-render-failure.md`: 1 line of `skills/blueprint/scripts/check-render-output.mjs` in the plugin, with a proven fix.
- The installed plugin belongs to the operator. Do not change it. Step 6 renders with a patched copy in `/tmp`.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read the 2 context files, the 2 skill parts, the template and `docs/specs/fleetd/blueprint-render-failure.md`.
3. Run `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd --status`. Each of the 9 steps must be done. If a step is pending, stale or draft, send `BLOCKED:` with the status table and end your turn.
4. Run `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd` and `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs contracts check ddd`. Record each error and each warning. Do not edit the artifact of a step to remove an error. Report it.
5. Run `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs review ddd`. It builds `ddd/review.html` and `ddd/diagrams/`.
6. Render the blueprint with a patched copy of the plugin:
   1. Make a new empty directory with `mktemp -d /tmp/ddd-plugin-XXXXXX`. Copy the plugin root into it with `cp -R`.
   2. In the copy only, apply the 1-line fix of the report to `skills/blueprint/scripts/check-render-output.mjs`. Run `diff -r` between the installed plugin and the copy. It must show this 1 line only.
   3. Run `node <copy>/shared/bin/ddd.mjs export blueprint ddd --deliver` from the repository root.
   4. Run `test -s ddd/diagrams/final.html`. If the export fails, record the exact error text. Do not claim a render that did not occur.
7. Read `ddd/diagrams/final.html` in part (its size, its title, the count of nodes) and 2 of the `.png` diagrams that changed. Check that they show the 6 contexts and the revised owners.
8. Write `ddd/COVERAGE.md` again from the template, for the revised workspace. Use paths relative to the repository root. Include all that ticket 20 included: each artifact, the status of each step, the count of open questions with each blocking one quoted, each open decision with its options and the recommendation, each validator warning, the deployable count against the scale target, the core domains, the review page, the diagrams, and the hand-off files `ddd/08-code/implementation-plan.md` and `ddd/09-contracts/contracts.md`.
9. Add to `ddd/COVERAGE.md` a section "Decided in round 2" near the top. Give 1 line for each decision of the revision context: the step, the question, the option, who decided (operator D16, or orchestrator D15). Put first the 3 decisions that differ from the recommendation of their step, and mark them.
10. Add to `ddd/COVERAGE.md` 1 short section "Known faults of the tools": `final.html` is rendered with a patched copy of the plugin and the installed plugin is not changed; the renderers write the em dash into some Markdown and HTML files, and those files are not edited by hand.
11. Keep the review order: glossary, decompose, strategize, organise, define, code.
12. Commit every new and changed file under `ddd/`.

## CONSTRAINTS

- Edit only `ddd/COVERAGE.md` and the files that the `review` and `export` commands write.
- Do not change a file under `/Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0`.
- Do not edit a rendered file by hand.
- Do not use the em dash in `ddd/COVERAGE.md`.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The **Check** command exits 0, or you sent `BLOCKED:` with the errors.
- `validate ddd --status` shows no stale step.
- `ddd/COVERAGE.md` lists each decision of round 2, each decision that is still open and each blocking question.

## REPORT

Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give: the count of bounded contexts, the core domains, the count of open decisions, the count of open and of blocking questions, each validator warning, and the result of the blueprint export with the size of `final.html`.
