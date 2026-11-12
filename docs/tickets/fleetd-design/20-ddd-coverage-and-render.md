# 20: DDD coverage summary, review page and final render

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** no
**Kind:** docs
**Review:** none
**Check:** `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd && test -s ddd/COVERAGE.md && test -s ddd/review.html`
**Own:** `ddd/`
**Do not touch:** `crates/`, `docs/specs/fleetd/`

## GOAL

The workspace `ddd/` validates as a whole, and `ddd/COVERAGE.md` tells the operator what exists, what is uncertain and what to review first.

## CONTEXT

- Read `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/ddd-context.md` first.
- The 9 steps of the DDD process are done. Each wrote its artifacts into `ddd/`.
- Your procedure is "Step 4" and "Step 5" of `/Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/skills/ddd/SKILL.md`, and the template `/Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/skills/ddd/references/coverage-summary.md`.
- The operator did not see any step. `ddd/COVERAGE.md` is the first file that the operator reads.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read the context file, the 2 skill parts and the template.
3. Run `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd --status`. Each of the 9 steps must be done. If a step is pending, stale or draft, send `BLOCKED:` with the status table and end your turn.
4. Run `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd`. Record each error and each warning. Do not edit the artifact of a step to remove an error. Report it.
5. Run `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs review ddd`. It builds `ddd/review.html` and `ddd/diagrams/`.
6. Run `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs export blueprint ddd --deliver`. If it fails, record the exact error text. Do not claim a render that did not occur.
7. Write `ddd/COVERAGE.md` from the template. Use paths relative to the repository root. Include: each artifact, the status of each step, the count of open questions with each blocking one quoted, each open decision (a `decisions[]` entry with no `chosen`) with its options and the recommendation, each validator warning, the deployable count against the scale target, the core domains, the review page, the diagrams, and the hand-off files `ddd/08-code/implementation-plan.md` and `ddd/09-contracts/contracts.md`.
8. In `ddd/COVERAGE.md`, give the review order: glossary, decompose, strategize, organise, define, code.
9. Commit every new and changed file under `ddd/`.

## CONSTRAINTS

- Edit only `ddd/COVERAGE.md` and the files that the `review` and `export` commands write.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The **Check** command exits 0, or you sent `BLOCKED:` with the validator errors.
- `ddd/COVERAGE.md` lists each open decision and each blocking question.

## REPORT

Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give: the count of bounded contexts, the core domains, the count of open decisions, the count of blocking questions, and the result of the blueprint export.
