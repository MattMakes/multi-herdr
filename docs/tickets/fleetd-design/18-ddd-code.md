# 18: DDD step 8 of 9: ddd-code

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** no
**Kind:** docs
**Review:** none
**Check:** `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd --step code`
**Own:** `ddd/`
**Do not touch:** `crates/`, `docs/specs/fleetd/`

## GOAL

Step 8 of the DDD process, `ddd-code`, is done for the fleetd design: its artifacts are in `ddd/`, and `ddd validate ddd --step code` exits 0.

## CONTEXT

- Read `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/ddd-context.md` first. It gives the tools, the fixed inputs and the rules.
- Your skill file is `/Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/skills/ddd-code/SKILL.md`. It is your procedure. This ticket only adds the fleet rules.
- The step before you is `ddd-define`. It wrote `ddd/07-define/define.json`. The language is Rust. The existing crates are `crates/horch-core` and `crates/horch`; read their layout, and do not edit them. Module paths must follow decision D1 of the design: OS-specific code sits behind 1 platform layer. The migration plan of the design gives the order of the implementation plan. Write no source code.
- The next step reads only your JSON file. Prose in a Markdown artifact does not reach it. Put each fact that a later step needs into the JSON, and into `notes_for_downstream[]` when the contract says so.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read the context file.
3. Run `test -s ddd/07-define/define.json`. If it fails, send `BLOCKED:` and end your turn.
4. Run `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd --status`. If it exits 2 in a step other than step 1, send `BLOCKED:` and end your turn. Copy the lines "notes addressed to ddd-code" into your notes. Your step must act on each one.
5. Read your skill file. Follow it from its first step to its last, in mode `auto` and depth `standard`, with the workspace `ddd/`.
6. Run each lint, check or render command that your skill file names for your step.
7. Run `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd --step code`. Fix each error in your own artifacts and run it again. If an error is in the artifact of an earlier step, do not edit that artifact: send `BLOCKED:` with the error text and end your turn.
8. If `ddd/diagrams/` holds a new `.png` file for your step, read it and check that it shows what your JSON says.
9. Commit every new and changed file under `ddd/`.

## CONSTRAINTS

- Edit only files under `ddd/`.
- Do not run a later step. Do not run an earlier step again.
- Do not start a subagent or a nested agent CLI.
- Do not ask the operator a question. A blocking problem goes to the orchestrator as `BLOCKED:`.

## DONE WHEN

- The **Check** command exits 0.
- `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd --status` shows step `code` as done.
- Each "notes addressed to ddd-code" line has an answer in your artifact.

## REPORT

Record 1 progress note at each numbered step of your skill file. Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give: the count of main items that you wrote (your skill file names them), each open decision (a `decisions[]` entry with no `chosen`) in 1 sentence, each blocking open question, and each validator warning.
