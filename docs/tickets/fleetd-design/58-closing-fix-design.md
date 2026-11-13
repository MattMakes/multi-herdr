# 58: Closing fix of the fleetd design after the third systems review (A37)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** no
**Kind:** docs
**Review:** none
**Check:** `grep -q 'A37' docs/specs/fleetd/design.md && grep -q 'A37' docs/specs/fleetd/amendments.md && grep -q 'not available on this OS' docs/specs/fleetd/design.md && ! grep -q 'The limit, in plain words' docs/specs/fleetd/design.md && ! grep -q 'Single writer; everything else opens read-only' docs/specs/fleetd/design.md && ! grep -q '—' docs/specs/fleetd/design.md && ! grep -q '—' docs/specs/fleetd/amendments.md`
**Own:** `docs/specs/fleetd/design.md`, `docs/specs/fleetd/amendments.md`
**Do not touch:** `ddd/`, each other file in `docs/specs/fleetd/`, `crates/`

## GOAL

`docs/specs/fleetd/design.md` states amendment A37, so the 3 HIGH findings that the third systems review left open (S42-2, T42-1, T43-2) no longer apply.

## CONTEXT

- `docs/specs/fleetd/design.md` is the design of a daemon named `fleetd`. 5 reviewers read it 3 times. Each third report (`docs/specs/fleetd/system-review-*-3.md`) has the verdict PASS. No CRITICAL finding is open.
- 3 HIGH findings are open. The reports `system-review-42-migration-and-requirements-3.md` and `system-review-43-operations-and-agreement-3.md` give the lines.
- The orchestrator decided them as amendment A37, in the section "Systems review round, closing fix" at the end of `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/amendments.md`. That section is your instruction.
- This is a small change on purpose: about 10 sentences. Do not change more.
- 1 judge checks your change after you (ticket 59).
- The connection to GitHub is not always stable. If the push in `horch ledger finish` fails with a network error, run `horch ledger finish` again, up to 3 times.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Copy `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/amendments.md` to `docs/specs/fleetd/amendments.md`. Change nothing in it. Run `git diff --stat`: the change must only add lines at the end of the file. Commit.
3. Read the section "Systems review round, closing fix" of that file. Read the 3 findings in the 2 reports.
4. Search the design for `sandbox`, `no sandbox`, `not supported`, `operator.token`, `06 S9`, `Single writer`, `read-only`, `sole`, `only ledger writer` and `stalled`. Read each place.
5. Apply A37 at each place. Commit.
6. Follow the failure scenario of T42-1 and of T43-2 step by step against the design as it is now: a worker writes a request file with the operator token. Name the sentence that stops it. If no sentence stops it, correct the design. Commit.
7. Run the **Check** command of this ticket. It must exit 0.
8. Run `git diff 5327458b HEAD --stat -- docs/specs/fleetd/design.md`. The change of the design must be small: under 40 lines changed. If it is more, you changed too much: look again.

## CONSTRAINTS

- Change only what A37 needs. Do not improve any other part of the design.
- Do not add a state, a table, a job kind, a command or a requirement.
- When A37 contradicts a decision D1 to D16 or leaves a gap that changes behavior, send a `QUESTION:` to the orchestrator and end your turn.
- Keep the voice of the design. Write short sentences in the active voice.
- Do not write the value of an environment variable or of a token. Never read, print or use `ANTHROPIC_API_KEY`.
- Do not use the em dash.
- Do not edit a file other than the 2 in **Own**.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- No sentence of the design accepts a worker with no sandbox.
- Row 06 S9 of the Preservation analysis and the profile validation rule say the same.
- No sentence states the single ledger writer without "from Phase 5".

## REPORT

Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give each line of the design that you changed, and for each of S42-2, T42-1 and T43-2 the sentence that now stops it.
