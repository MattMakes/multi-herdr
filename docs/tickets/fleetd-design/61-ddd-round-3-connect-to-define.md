# 61: DDD revision round 3: connect, organise, define

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** no
**Kind:** docs
**Review:** none
**Check:** `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd --step connect && node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd --step organise && node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd --step define`
**Own:** `ddd/`
**Do not touch:** `crates/`, `docs/specs/fleetd/`

## GOAL

The steps connect, organise, define of the DDD model of the fleetd design agree with the final design (amendments A23 to A37), keep their ids, and each validates with exit 0.

## CONTEXT

- Read `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/ddd-revision-3-context.md` first. It names 2 more files to read, what changed in the design by topic, the small faults to correct, and the known faults of the tools. It is your instruction.
- Your steps, in this order:
  - `connect`: `ddd/05-connect/connect.json`, skill file `/Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/skills/ddd-connect/SKILL.md`
  - `organise`: `ddd/06-organise/organise.json`, skill file `/Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/skills/ddd-organise/SKILL.md`
  - `define`: `ddd/07-define/define.json`, skill file `/Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/skills/ddd-define/SKILL.md`
- Ticket 60 revised the steps understand, discover, decompose and strategize.
- Use each skill file for its format rules, its lint commands and its render commands. This is a revision: do not generate a step again.
- The next ticket reads only the JSON files. Put each fact that a later step needs into `notes_for_downstream[]` of the last step that knows it.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read the context file and the 2 files it names. Read the 3 sections of `docs/specs/fleetd/amendments.md` that it names.
3. Run `grep -q 'A37' docs/specs/fleetd/design.md && grep -q PASS docs/specs/fleetd/design-review-5.md`. If it fails, send `BLOCKED:` and end your turn.
4. Run `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd --status`. Copy the notes addressed to your steps.
5. Read the sections of `docs/specs/fleetd/design.md` that your steps model. Use `git diff 043cc279 HEAD -- docs/specs/fleetd/design.md` to find what is new.
6. For each of your steps, in order: do the section "How to revise a step" of `ddd-revision-context.md`, with the changes of the section "What is different in this round". Commit after each step.
7. Run `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd --status`. No step of your group is `stale`. If one is, stamp and render it again, in order, and commit.
8. If `ddd/diagrams/` holds a changed `.png` file of your steps, read 2 of them and check that they show what the JSON says.

## CONSTRAINTS

- Edit only files under `ddd/`. Edit only the JSON of the steps of your group.
- Keep each id. A removed or renamed id needs a reason in your report.
- Do not open a decision. Do not leave a `chosen` empty.
- Do not model more than the design says. A thing that the design does not state is not in the model.
- Do not edit a rendered file by hand.
- Do not start a subagent or a nested agent CLI.
- Do not ask the operator a question. A blocking problem goes to the orchestrator as `BLOCKED:`.
- Do not use the em dash in a JSON field.

## DONE WHEN

- The **Check** command exits 0.
- `validate ddd --status` shows each step of your group as done and not stale.
- Each topic of the context file that touches your steps is in the model, or your report says why not.
- The id comparison of each step shows no removed id, or your report names each one with its reason.

## REPORT

Record 1 progress note for each step. Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give for each step: the count of items added and changed, each removed id with its reason, each new note for a later step in 1 sentence, and each validator warning. Then give each gap to a step of another group.
