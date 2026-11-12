# 36: DDD revision: close the cross-step gaps

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** no
**Kind:** docs
**Review:** none
**Check:** `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd && node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs contracts check ddd && ! node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd 2>&1 | grep -q 'not present in'`
**Own:** `ddd/`
**Do not touch:** `crates/`, `docs/specs/fleetd/`

## GOAL

The 9 steps of the DDD model in `ddd/` agree with each other: each gap of the gaps file is closed, no step is stale, and `ddd validate ddd` shows no warning "not present in".

## CONTEXT

- Read `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/ddd-context.md` first, then `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/ddd-revision-context.md`.
- Tickets 24 to 32 revised each of the 9 DDD steps, 1 worker for each step. A worker could not edit the JSON of another step. So some differences between steps are left.
- The gaps file is `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/consistency-gaps.md`. It lists each gap and its fix. It is your instruction.
- This ticket is the only one that edits more than 1 step. It edits only what a gap needs.
- An edit of an early step makes each later step stale. So after the edits, each later step gets a new stamp and a new render, in order.
- The next ticket writes the coverage summary and the review page.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read the 2 context files and the gaps file.
3. Run `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd --status` and `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd`. Each of the 9 steps must be done. If a step is pending, stale or draft, send `BLOCKED:` with the status table and end your turn. Record each warning.
4. For each step JSON that you will edit, save the old file: `git show HEAD:<path> > /tmp/<step>-before.json`.
5. Close each gap of the gaps file, from the earliest step to the latest. Edit only the items that the gap names. Keep each id.
6. Close each other warning "not present in <step>" that step 3 showed, in the same way: the earlier step adopts the id with the type that the later step gives it.
7. For each step from the earliest that you edited to `contracts`, in the order of the process: run `ddd stamp` on its JSON file, run the lint and render commands that its skill file names, then run `ddd validate ddd --step <step>`. Do this also for a step that you did not edit. Do not change the content of a step that you did not edit.
8. Run `python3 -I /Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/id-diff.py /tmp/<step>-before.json <step JSON>` for each step that you edited. The list "removed" must be empty, or your report names each removed id with its reason.
9. Run the **Check** command of this ticket. It must exit 0. Run `node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd --status`: no step is stale.
10. Commit every new and changed file under `ddd/`.

## CONSTRAINTS

- Edit only files under `ddd/`.
- Change only what a gap or a warning of step 6 needs. Do not improve a step.
- Do not add a decision. Do not change a chosen option.
- Do not edit a rendered file by hand.
- If a gap cannot be closed without a new design decision, do not guess. Leave it, and name it in your report.
- Do not start a subagent or a nested agent CLI.
- Do not use the em dash in a JSON field.

## DONE WHEN

- The **Check** command exits 0.
- No step is stale.
- Each gap of the gaps file is closed, or your report names it with the reason.

## REPORT

Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give for each gap: the step and the ids that you changed. Give each warning that is left, each removed id with its reason, and each gap that you did not close.
