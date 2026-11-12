# 07: Write the amended fleetd design

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** no
**Kind:** docs
**Review:** none
**Check:** `test -s docs/specs/fleetd/design.md && grep -q 'waiting_quota' docs/specs/fleetd/design.md && grep -q '^## Features, landing and integration' docs/specs/fleetd/design.md && grep -q '^## Preservation analysis' docs/specs/fleetd/design.md && grep -q '^## Amendments' docs/specs/fleetd/design.md && ! grep -q 'max_sub_orchestrators' docs/specs/fleetd/design.md && ! grep -q '—' docs/specs/fleetd/design.md`
**Own:** `docs/specs/fleetd/design.md`

## GOAL

The file `docs/specs/fleetd/design.md` is the source design with every amendment A0 to A14 applied, and it does not contradict itself.

## CONTEXT

- The operator wrote a design for a new control plane of `horch`, a daemon named `fleetd`. The operator and the orchestrator then reviewed it and made 13 decisions.
- The source design is `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/source-design.md`. It has 850 lines.
- The amendments are in `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/amendments.md`. This file is your instruction. It names each change and the places to change.
- The decisions and their reasons are in `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/decisions.md`. Read it when an amendment is not clear.
- The research and the inventory are in `docs/specs/fleetd/` on your branch: `research-01-industry-practice.md`, `research-02-merge-tools-and-agent-systems.md`, `research-03-this-repo.md`, `preservation-04-behavior.md`, `preservation-05-contracts-and-values.md`, `preservation-06-wiring-and-platform.md`.
- Ticket 08 is next. A judge in another session reads your file against the amendments. A decision that your file does not show is a CRITICAL miss.
- The next consumer after that is a Domain-Driven Design process. It reads only `docs/specs/fleetd/design.md`. The file must be complete without the amendments file.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read the amendments file fully. Read the decisions file. Read the source design fully.
3. Copy the source design to `docs/specs/fleetd/design.md`. Commit this copy alone, with the message `docs(fleetd): add the source design`. The next commits then show only your changes.
4. Apply amendment A1. Then search the file for each word that the amendment replaces, and fix each place. Commit. Do the same for A2 to A14, 1 commit for each amendment or small group.
5. Apply amendment A0 last, because the summary must be true after all other amendments.
6. Read the whole file from the first line to the last. Fix each sentence that contradicts an amendment. Typical places: a requirement row, a failure mode row, a migration phase, an example, a diagram.
7. Run these searches. Each must print nothing, or only lines that an amendment allows: `grep -n 'max_sub_orchestrators\|idea bump\|max_tokens' docs/specs/fleetd/design.md`, `grep -n 'run-level flag' docs/specs/fleetd/design.md`, `grep -n 'one PR per idea\|branch per idea' docs/specs/fleetd/design.md`.
8. Run the **Check** command of this ticket. It must exit 0.

## CONSTRAINTS

- Keep the voice of the source. It speaks to the operator as "you". Keep its heading style and its table style.
- Keep each part of the source that no amendment covers, word for word. Do not improve it. Do not shorten it.
- Do not add a design idea of your own. If an amendment leaves a gap that you cannot fill from the decisions file, send a `QUESTION:` to the orchestrator and end your turn.
- Do not use the em dash. Use a plain dash.
- Never write a secret or the value of an environment variable. Never read `ANTHROPIC_API_KEY`.
- Do not edit a file other than the one in **Own**.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- Each of A0 to A14 is applied, and the `## Amendments` table has 13 rows, D1 to D13.
- The 3 searches of step 7 give no line that contradicts an amendment.

## REPORT

Record 1 progress note after each amendment. Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give the line count of the file, and each place where you were not sure that the source and an amendment agree.
