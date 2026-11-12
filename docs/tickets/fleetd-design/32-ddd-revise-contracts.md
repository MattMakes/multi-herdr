# 32: DDD revision, step 9 of 9: ddd-contracts

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** no
**Kind:** docs
**Review:** none
**Check:** `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd --step contracts && node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs contracts check ddd`
**Own:** `ddd/`
**Do not touch:** `crates/`, `docs/specs/fleetd/`

## GOAL

Step 9 of the DDD model of the fleetd design, `ddd-contracts`, states the decisions and changes of revision round 2, keeps its ids, and `ddd validate ddd --step contracts` exits 0.

## CONTEXT

- Read `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/ddd-context.md` first, then `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/ddd-revision-context.md`. The second file is your instruction: what a revision is, the decisions, the changes for your step and the answers to open questions.
- Your skill file is `/Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/skills/ddd-contracts/SKILL.md`. Use it for the format rules, the lint commands and the render commands. This is a revision: do not generate the step again.
- Your step's JSON file is `ddd/09-contracts/contracts.json`. It exists. It is the first draft of ticket 19.
- The step before you, `ddd-code`, is revised (ticket 31).
- The next step reads only your JSON file. Put each fact that a later step needs into `notes_for_downstream[]`.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read the 2 context files.
3. Run `test -s ddd/09-contracts/contracts.json && grep -q 'Phase P' docs/specs/fleetd/design.md`. If it fails, send `BLOCKED:` and end your turn.
4. Run `git diff f6243ab2 HEAD -- docs/specs/fleetd/design.md` and read the change.
5. Do the 9 steps of the section "How to revise a step" of the revision context, for the step `contracts` and the file `ddd/09-contracts/contracts.json`.
6. If `ddd/diagrams/` holds a changed `.png` file of your step, read it and check that it shows what your JSON says.
7. Commit every new and changed file under `ddd/`.

## CONSTRAINTS

- Edit only files under `ddd/`.
- Keep each id. Do not rewrite an item that the decisions and the design change do not touch.
- Do not edit the JSON file of another step. If an error is in the artifact of an earlier step, send `BLOCKED:` with the error text and end your turn.
- Do not run a later step.
- Do not edit a rendered file by hand.
- Do not start a subagent or a nested agent CLI.
- Do not ask the operator a question. A blocking problem goes to the orchestrator as `BLOCKED:`.
- Do not use the em dash in a JSON field.

## DONE WHEN

- The **Check** command exits 0.
- `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd --status` shows step `contracts` as done, not stale.
- Each decision of the revision context for your step has its chosen option in `decisions[]`.
- The id comparison shows no removed id, or your report names each one with its reason.

## REPORT

Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give: each decision that you recorded, with its option; the count of items added and changed; each removed id with its reason; each decision that is still open; each new note for a later step in 1 sentence; each validator warning.
