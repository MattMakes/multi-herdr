# R8: end review of the change of the run fleetd-finish

**Persona:** primary `codex-sol`, fallback `opus` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/reviews/end-review-delta.md && grep -q '^## Watchlist lines covered' docs/specs/fleetd/reviews/end-review-delta.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/reviews/end-review-delta.md`

Spec: `ai_docs/plans/fleetd-finish/plan.md` (wave F).

## Goal

Review the code of fleetd for the change of this run: `git diff fleet/fleetd-build...HEAD` in your worktree (the isolation fix, T-898, T-899, the 8 other landed task branches, T-900, T-882, the gap fixes) as an engineer who did not write it. Find each fault that can lose work, break a state machine, or let a worker act as the operator. The orchestrator writes 1 fix ticket for each Critical and Important finding.

## Read first

- `docs/specs/fleetd/design.md`: the sections that your scope names, the table "Where each rule becomes green", and "Failure modes and effects".
- `ai_docs/plans/fleetd-build/for-operator/end-review-watchlist.md` in your worktree: 77 lines. Each line is 1 point that an earlier worker marked for this review.
- `ai_docs/plans/fleetd-build/for-operator/main-findings-from-T177-review.md`.
- The audit of your phases in `ai_docs/plans/fleetd-finish/audit/`: it maps each rule to its code path and test.
- `ai_docs/plans/fleetd-build/decisions.md`: search it for the T-number of a watchlist point to get the ruling.

## Scope

- Read the full diff `git diff fleet/fleetd-build...HEAD` and the log `git log --oneline fleet/fleetd-build..HEAD`.
- Security first: FR-10 and `fleet_process`; the wrapper and the operator token (T-899); the shared-ledger guard and the service label of ticket 01.
- This review has no owned watchlist line unless a line names T-892, T-893, T-895, T-897, T-898, T-899 or T-900. Own those.

## Rules


1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

## Steps

1. Read the watchlist. List each line that names code of your scope. You own those lines.
2. For each owned line: read the code, decide if the risk is real, and write a finding or write "checked, no fault" with the file and line that proves it.
3. Then review the scope beyond the watchlist. For each rule of your phases, read the code path and its test. Ask: does the test prove the rule, or only a part? Can the test pass while the rule is broken?
4. Look for these fault classes in each file of the scope:
   - a decision of the pass that reads a file or a client payload, not the ledger;
   - a ledger change and its effect in the wrong order (the row must be first);
   - a state with no exit, or a retry with no bound;
   - a path where 1 bad row fails the whole pass;
   - a timer that does not use the fleet clock;
   - a worker-writable file that fleetd trusts (size, type, owner, link, hash);
   - a request kind whose class or payload a worker can forge;
   - a secret in a log, an env or a file that a worker can read;
   - code under `cfg(target_os)` outside `crates/horch-platform`.
5. Do not run `cargo test`. You can run `cargo check` through `scripts/dev-sandbox.sh` only when a claim needs it.
6. Write the report. Commit it with the subject `R8: the end review of the change of this run: `git diff fleet/fleetd-build...HEAD` in your worktree (the isolation fix, T-898, T-899, the 8 other landed task branches, T-900, T-882, the gap fixes)`.

## Report

Write `docs/specs/fleetd/reviews/end-review-delta.md`:
- `## Summary`: counts of Critical, Important and Minor findings, in 5 lines or less.
- `## Findings`: 1 subsection each, with an id `R8-F<n>`, the rank, the design rule id, file:line, what goes wrong with 1 concrete case, and the smallest fix. Rank Critical when work is lost, a worker gets operator rights, or a live fleet of the user can be harmed. Rank Important when a rule of the design is not met or a state can hang. Rank Minor for the rest.
- `## Watchlist lines covered`: 1 row for each owned line: the line number, the T-number, and "finding R8-F<n>" or "checked, no fault: file:line".
- `## Tests that do not prove their rule`: rule id, test name, what is not proven.
- `## Operator decisions`: each point that needs a product or design decision, with your recommendation.
Mark each claim `verified` or `not verified`.

## Out of scope

- Do not fix code. Do not edit a test, a ticket or the design.

## Done when

The Check passes, each owned watchlist line has 1 row, and the report is committed.

## Added 2026-11-20: deleted and weakened tests

The stale-test tickets of this run (CI FIX-8, FIX-10, baseline tickets) change or delete tests of retired behavior. List each test that the run deleted, and each test whose assert changed (`git diff fleet/fleetd-build...HEAD --stat -- '*tests*'`, then read the diffs). For each one, check that the commit message names the retired behavior and its rule, and that a test of the new behavior exists. A deleted test of a rule that still holds is an Important finding.

## Added 2026-11-20T17:25Z: points that the orchestrator gives to this review

Check each point against the design and write a finding when it is wrong.

1. RR-12 and design.md line 1015: fleetd has no session resume with an answer (no code sets `workers.resumed_from_session`). Find the test that proves the other arm of RR-12: a fresh worker starts from the question, the answer and the committed work. If no green test proves it, that is an Important finding. Say if line 1015 ("fleetd resumes the worker's session with the answer") is met.
2. Deleted tests of T-15: `g52_resume_uses_own_session`, `g52_done_discovery_uses_the_same_launch_token` (commit subject "T-15: the spawn tests of fleet18_observability read the attempt spawn of fleetd").
3. Deleted test of T-90: `add_map_queues_a_ticket_map_in_one_step`. Rewritten tests of T-90 in push form: each dropped assert is in the commit message.
4. T-78: `each_launch_gets_only_its_own_rules` now asserts on the rule `horch sessions`.
5. T-77: `#[allow(dead_code)]` on `crates/horch/tests/support/mod.rs`.
6. T-209: the cause of 1 `SQLITE_READONLY` of `horch ledger status --json` was not reproduced.
7. T-232 (commit subject "T-232 CI FIX-8: the stale tests of taskledger.rs part 2 run in push form") deleted 9 tests. Its commit message gives the design text for each. Check each reason against the design and the code, most of all: (a) the 2 reviewer-fallback tests (claim: `Tools.reviewers_down` is never filled, and RR-11 covers the fallback by the pool; is a reviewer that cannot run handled?), (b) `a_new_tree_supersedes_the_open_reviews_of_the_old_round`, (c) the 4 land-step tests (claim: FM-43 and I8 tests cover them: name the tests and see them green). Do the same for the dropped asserts of T-90 (commit 9e1efb7d).
8. T-236: `workers::waits_now` leaves out a wait that must end; confirm that a writer (the fleetd pass) ends each such wait.
9. Comment sweeps that removed comments outside the diff of their task. Seen in T-89 (commit 07639b86 removed security comments of T-182 in `fleet_runtime/application/jobs.rs` and `tests/check_sandbox.rs`; restored before the land). Check each commit of this run whose subject holds "comment sweep" or "Comment sweep" (`git log --grep -i 'comment sweep' fleet/fleetd-build..HEAD`): list each one that removed a comment that gives a reason (a security rule, an invariant, a design id) and not a narration. Each such removal is a finding: the comment must come back.
10. T-249 (commit subject "CI FIX-21: watch_once_reports_in_text_and_json pins the dead verdict of an unproven attempt"): the test now expects `dev1 T-1: dead (round 0): the agent process is gone` for a live task attempt with no recorded process and no pane, and the task stays running. Check against the design that this verdict is the designed one and that the attempt ends in bounded time (exit.json or lease expiry).
11. T-246: on macOS a task check can connect to the own LAN address of the host (Seatbelt knows only localhost or *); the test `sr3_check_that_connects_off_host_is_refused` now uses 192.0.2.1. Say if this meets the network rule of a check.
12. T-288 (RR-14): `result.json` got the optional key `unlanded` (status `completed`), which design.md lines 541 to 557 do not show; ingest parks the task. Check the park against RR-14 (the tree stays, the dependents stay pending, a requeue runs in the same tree) and say if a worker can misuse the key (for example to skip the verifier for work that could land).
13. T-287 (RR-13): the test `rr13_drop_with_a_head_that_moved_during_the_checks_keeps_the_commit_in_the_rescue_ref` became `rr13_a_commit_after_an_accepted_drop_is_kept_in_the_rescue_ref`.
14. T-300 (RR-12): a refused resume ends `no_result` and burns 1 attempt of the task (ladder step 6), then a fresh attempt starts after the backoff. design.md line 1015 says only "If the resume fails, a fresh worker starts". Say if the burned attempt is right, or if a failed resume is a fault of the platform that must burn none.
15. T-254: `worker_launch_failed` ends a push attempt at once; the bundle test of `p0_landing` sets `HORCH_FAULT=drop-watch-events`. Check that only the launch helper of fleetd can send the request.
16. T-286 and T-294: asserts on retired wakes ("Tasks can start", "shows no progress") in `the_whole_flow_on_the_fake_herdr` now assert that no such wake is written (FM-16).
17. T-309 (Linux sandbox): the job sandboxes use `bwrap --unshare-pid` with an inner `setsid`, no `--new-session`; the agent path keeps `--new-session` and host PIDs (FR-10 peer check). Check the terminal protection of both kinds and each stop path (ticket 208).
18. T-320 (ticket 209): the operator-boundary probe must not read "not found" as "refused". Check that no other conformance probe or security test has the same false Pass (search for a match on any error where only a permission error proves the rule).
19. T-292: paged and set-based readers of the pass. Check that the lease expiry, the disk budget and the bind compare of Own files give the same result as before.
