# 15: Worker briefing rules that keep workers moving (P07, P11, P14, P19, P23, P24)

**Issue:** [#63](https://github.com/MattMakes/multi-herdr/issues/63) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Check:** `cargo test -p horch-core --test golden_prompts --test context_roster && cargo test -p horch --test context_cli && env HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

The worker briefing gained a task-ledger section: claim, `ledger worktree new`, `ledger progress`, `ledger finish` (check, code-sweep, complexity-sweep, recheck, land, pr, cleanup), "end your turn" while reviews run, and `ledger done`. The other sections are unchanged. Status per rule:

- **P11 (question with a recommendation): partly there.** The style example shows options and "I recommend", but under `BLOCKED:`. The rules still say ask, then end your turn. Ending the turn now risks a reap, so this needs #70.
- **P14 (background commands): still needed, and it conflicts with current text.** The scope section still says "Do not use subagents, the Agent tool, or background tasks", and the golden pins that line. A long command in the foreground counts as progress, because the working timer runs. An idle turn with a background job does not, so this also needs #70.
- **P07 (`BLOCKED:` with the exact command): still needed.**
- **P23 (check every step before done): partly covered.** `ledger done` refuses until every finish step passed. But the `check` step runs only the ticket's check command, and is not applicable when there is none. Nothing checks the ticket's steps or `Own:` files.
- **P24 (`[crushed]` re-read): still needed.**
- **P19 (context warning): still needed, with a gap.** The warning fires only from `horch note`, and ledger workers report with `horch ledger progress`. #53 closes that gap; this ticket only changes the text.
- **P29 (DONE sent twice, worker side): still a risk.** The briefing still says "horch tell orchestrator ... report results", and its style example sends `[role] DONE: ...` with `tell`. `done` already sends the DONE line itself.
- **Golden test structure:** the worker golden test is a series of named replacements on the old golden, with a block literal per section. Add new named blocks, and do not cite block numbers. The final assert message's count is stale; fix it while you are there.

## What to build

Workers stop stalling, sleep-polling, skipping steps and reporting twice. These are text changes to the worker briefing and context messages, in Simplified Technical English. Each rule names the ledger command in a ledger run, and the plain command otherwise.

- **Questions (P11).** Send `QUESTION:` with a recommended option. If the decision is reversible, go on with that option, and say so. If it is not reversible, say so, do what you can meanwhile, then end your turn. #70 keeps you from being reaped while you wait.
- **Background (P14).**
  - Long commands may run in the background.
  - Record `horch ledger progress "waiting: <what>"` (or `horch note "waiting: <what>"` outside a ledger run), then end the turn.
  - Never sleep or tail in a loop. Subagents stay banned.
- **Permission denials (P07).** On a denial, do not retry. Send `BLOCKED:` with the exact command and why it is needed.
- **Before finish (P23).** Before `horch ledger finish` (or `horch done` outside a ledger run), check every ticket step and every `Own:` file. Name in the summary anything not done.
- **Cut-short reads (P24).** When a read comes back with a `[crushed]` note, read the file again with `cat` before you act.
- **Context warning (P19).** Add "If DONE WHEN is met, run horch ledger finish, then horch ledger done (or horch done) instead." The message stays 1 line.
- **One DONE (P29).**
  - `done` is the only DONE path.
  - Remove "report results" from the tell rule, and the style example's `tell` DONE line.
  - Use `NOTE:` for interim results.

## Acceptance criteria

- [ ] Each rule is in its own named sanctioned block in the worker golden test, with its reason. The assert message is accurate.
- [ ] The tests that pin the full warning text are updated to the new line.
- [ ] No briefing tells a worker to sleep, poll or tail in a loop, or to send `DONE:` with `horch tell`.
- [ ] `teammates --check` passes, and context messages stay 1 line.

## Blocked by

- #56 (Pitfalls 14: facts and host line, which changes the same goldens)
- #70 (Pitfalls 22: the waiting state, without which "end your turn and wait" gets workers reaped)
