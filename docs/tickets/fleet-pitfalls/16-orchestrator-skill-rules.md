# 16: Orchestrator skill rules - honest reports, delegate, fan out (P05, P08, P13, P15, P25)

**Issue:** [#64](https://github.com/MattMakes/multi-herdr/issues/64) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Check:** `cargo test -p horch-core --test golden_prompts --test skills_catalog && cargo test -p horch --test baseline_cli && env HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

- **Size budget:** the orchestrate skill is now 11,810 bytes of its 12,288-byte budget, so **478 bytes are left**. These rules must replace text, not add to it. Good places to rewrite:
  - the step that re-runs the plan's tests on each DONE;
  - the step that lands a branch itself;
  - the full-verification step.

  Any change to the skill must also update the skill size recorded in the skills JSON oracle.
- **New:** the skill gained a task-ledger section 0 (queue, plan workers, spawn with no task, end your turn and be woken, `ledger status`, `ledger stop`, `ledger unfinished`). The orchestrator briefing gained the same. "End the turn and wait to be woken" is done.
- **P05 (verified labels): not present.**
- **P13 (read-only verification): still needed, and current text contradicts it.**
  - The skill says "Land the branch, or spawn a fresh worker to land it".
  - The briefing says "Land or reassign every claim".
  - The skill has the orchestrator run `ledger dismiss`, and `dismiss --cleanup` removes a worktree.
- **P15 (targeted tests): changed by the ledger.** `finish` already runs each ticket's check, and a recheck after the sweeps, with evidence in `horch ledger steps T-<n>`. The skill and briefing still tell the orchestrator to re-run tests on every DONE. The rule becomes: on DONE, read the `ledger steps` evidence and do not re-run; run 1 full gate on the run branch, through a worker, before the run's PR merges.
- **P25 (1 unit per item): not present.**
- **P08 (work in flight): partly there.** `ledger unfinished` covers past unlanded work. Nothing covers live orchestrators, other active runs or open PRs.
- **Seeing workers:** `horch peek` comes from #59. Until then, the skill can name `horch inbox`, `horch ledger show <worker>` and `horch ledger events`.
- **P29 (DONE sent twice, orchestrator side):** still invited by:
  - the assign example "Send DONE: when the tests pass" (also pinned in the orchestrator style golden);
  - the report keywords;
  - the skill's REPORT item "when to send horch tell orchestrator".
- **Conflict to reconcile:** the briefing still says to read `horch sessions` before every spawn, while its ledger section says to spawn per `horch ledger plan-workers`.

## What to build

The orchestrator reports facts, not guesses, and delegates instead of doing worker work. These are text changes to the orchestrate skill, and to the orchestrator briefing where it conflicts:

- **Honest reports (P05).** Label each claim to the operator as verified (with the evidence) or not verified. Report a failed delivery or command as failed.
- **Read-only verification (P13).** The orchestrator's own checks only read. Landing, installs, cleanups and `dismiss --cleanup` go to a worker.
- **Evidence over re-runs (P15).** On DONE, read `horch ledger steps`, and do not re-run the tests. 1 worker runs 1 full gate on the run branch before the run's PR merges. Hold back test-heavy spawns when `spawn` prints a load note.
- **Fan-out (P25).** Reviews, audits and research get 1 unit per item.
- **Work in flight (P08).** Before planning, check `horch ledger status` for other active runs, the `spawn` notes for a live orchestrator, and open PRs in the project.
- **Seeing workers.** Use `horch inbox` and `horch peek` (or `ledger show`/`ledger events`). Never build pane-reading loops, cron jobs or monitors.
- **One DONE (P29).** Workers report done only with `done`. Remove "Send DONE: when the tests pass" and the REPORT item about a separate tell.
- **Sessions vs plan-workers.** In a ledger run, spawn per `plan-workers`; `horch sessions` is for runs without a ledger.

## Acceptance criteria

- [ ] The skill size budget test passes, and the skills JSON oracle records the new size.
- [ ] No line in the skill or orchestrator briefing tells the orchestrator to land, install, clean up or run the full suite itself, or tells a worker to send `DONE:` with `horch tell`.
- [ ] Orchestrator golden changes are in named sanctioned blocks, and the Claude and Codex orchestrators still differ only in their tier block.
- [ ] `teammates --check` passes, including the exec-policy rule check for any newly named command.

## Blocked by

- #59 (Pitfalls 03: inbox status and horch peek, which the skill teaches)
