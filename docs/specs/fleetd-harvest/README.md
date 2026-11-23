# Guide: port 10 fixes from fleetd to `main`

Status: proposed. Target: `main` at `06930d95` (v0.5.3). Source of the ideas: fleetd at `9bc3bf70`.
Reader: the operator. Counts are from the live ledger, read-only, unless a row names another source.

## What this guide is

fleetd is a daemon that replaces the orchestrator conversation with an idea queue and per-feature
agents. It holds good ideas, but it is 68,849 production lines and has no green CI run. This guide
shows the other path: keep the orchestrator workflow, and port 10 small fixes to `main`. The 10
fixes reduce lost attempts, silent stalls, full-disk failures and unsafe task checks. They do not
remove the 1 long orchestrator conversation, which stays the main limit of this workflow. The specs
measured the ledger again and corrected numbers of the first review (see
[What changed after measurement](#what-changed-after-measurement)). Only 4 fixes have enough
measured harm to do now. Each spec is complete enough for an engineer who does not know fleetd.

Terms used in all 10 specs:

1. A `task check` is the build or test command of a task. A `watchdog check` is 1 observation of 1
   worker.
2. A `turn stop` is the end of 1 model turn. A `process stop` is a signal to a process.

## The 10 fixes

| Fix | Problem in plain words | Measured on `main` | Size | Report id | Real on main |
|---|---|---|---|---|---|
| [01](01-check-sandbox.md) | A task check runs with the operator's full rights. | 414 of 508 checks had full rights. 0 known thefts. | M, 4 d | P2 | yes |
| [02](02-bind-task-at-spawn.md) | Two conflicting workers can start, and 1 finds no task. | 0 of 515 since the spawn gate | S, 0.5 to 1 d | FM-2 | part |
| [03](03-cap-worker-waits.md) | A declared wait hides a stuck worker for up to 7 days. | 9 reaps hold 4807 of 5884 gap minutes | S, 1 d | FM-3 | yes |
| [04](04-restart-hung-watchdog.md) | A live but hung watchdog is never replaced. | 0 recorded incidents | S, 0.75 d | FM-12 | yes |
| [05](05-fast-pid-check.md) | A worker with all processes gone waits for the 600 s check. | 7 cases, 1816 s in 23 days | S, 0.75 d | FM-4 | part |
| [06](06-usage-limit-requeue.md) | A worker at a usage limit is reaped and loses an attempt. | 27 of 49 reaps | M, 2.5 d | FM-5 | yes |
| [07](07-stop-hooks.md) | A worker ends its turn to wait, with no wait record. | 7 reaps, mean 33.1 min | M, 2 d | FM-6 | part |
| [08](08-free-disk-floor.md) | New workers start while the disk is almost full. | 0 in the ledger. 1 failed finish and 6 hand cleanups in the fleetd log | S, 1 d | FM-28 | yes |
| [09](09-verdict-tied-to-commit.md) | A commit during finish can skip a verdict. | 0 of 383 landed tasks | S, 0.5 d | P7 | part |
| [10](10-ticket-ledger-compare.md) | A run can close with a ticket that has no task. | 0 of 412 tickets have no task in any run | S, 1 d | FM-1 | yes |

The total size of all 10 fixes is 4 + (0.5 to 1) + 1 + 0.75 + 0.75 + 2.5 + 2 + 1 + 0.5 + 1 =
**14 to 14.5 worker-days**. The sizes are estimates. Nothing was built. The second review added
work to some specs, and the sizes were not estimated again.

## What changed after measurement

The first review is `ai_docs/explain/fleetd/verdict.md`. It used the counts of report R6.

| Fix | First review said | Spec measured | Effect on priority |
|---|---|---|---|
| 01 | The cut is easy. | 4 worker-days. A third runner exists: `horch worktree land --check`. | Larger. Moves to the last wave. |
| 02 | 85 of 4239 workers find no task. | Commit `306bfe72` added a gate: 85 of 3757 before, 0 of 515 after. | Falls. Can drop. |
| 03 | Mean 120 min to a reap. | 9 reaps with a wait hold 81.7% of the gap. The other 40 have a mean of 26.9 min. | Stays high. Do now. |
| 04 | A confirmed defect. | The defect is in the code. 0 incidents are recorded. | Falls. Can drop. |
| 05 | Check PIDs on the 30 s tick. | 7 slow cases, 1816 s in total, in 23 days. | Small. Do later. |
| 06 | 9 reaps, after 10 min. | 27 of 49 reaps (18 Claude, 9 Codex), after 30 to 65 min. | Rises to first. |
| 07 | 29 reaps: workers finish and forget to report. | 18 usage limits, 7 waits with no record, 1 compaction, 3 with no transcript. 0 forgot `done`. | Falls from 29 to 7. Do now, after 03. |
| 08 | Refuse a spawn below a floor. | 0 ledger events. The fleetd log has 1 failed finish and 6 hand cleanups. | Stays. Do now. |
| 09 | The land step lands an unjudged tip. | `main` already reruns stale steps. 3 narrow gaps remain. 0 of 383 show the fault. | Falls. Can drop. |
| 10 | No count. | 0 of 412 tickets in 14 runs have no task. | Falls. Can drop. |

Counts of 03, 06 and 07 overlap, so do not add them. Spec 06 owns the 18 usage limits that spec 07
also counts.

## Order

The rule: rank by measured harm first, then by size. A fix with 0 recorded occurrences goes in the
last wave, and the operator can drop it.

| Wave | Fix | Decision | Reason |
|---|---|---|---|
| 1 | 06 | do now | Largest measured harm: 27 of 49 reaps. |
| 1 | 03 | do now | 81.7% of reap gap minutes. Spec 07 needs it. |
| 2 | 07 | do now | 7 reaps, each with a lost attempt. It must follow 03. |
| 2 | 08 | do now | Recorded failures in the fleetd log. Small. |
| 3 | 05 | do later | Real but small: 1816 s in 23 days. |
| 4 | 09 | can drop | 0 of 383. Smallest size. |
| 4 | 02 | can drop | 0 occurrences since the gate. |
| 4 | 04 | can drop | 0 incidents. The defect is real, but it has not occurred. |
| 4 | 10 | can drop | 0 tickets with no task in any run. |
| 4 | 01 | can drop | 0 known incidents, and the largest size. See the warning below. |

Warning for fix 01: its count of 0 does not show that checks are safe, because the ledger does not
record what a check reads. One part of spec 01 is a confirmed breach of the first rule of
`CLAUDE.md`: `Landing::run_check` (`crates/horch-core/src/vcs/landing.rs:907`) does not call
`scrub_child_env`, so it passes the forbidden variable to the check. See question G1.

Total size:

1. All 10 fixes: **14 to 14.5 worker-days** (sum above).
2. Only the "do now" fixes: 2.5 (06) + 1 (03) + 2 (07) + 1 (08) = **6.5 worker-days**.
3. "Do later": 0.75 (05). "Can drop": 0.5 + (0.5 to 1) + 0.75 + 1 + 4 = 6.75 to 7.25 worker-days.

Pairs that change the same file, and which goes first:

1. 06 before 03: `watchdog.rs` (`check_worker`) and `taskledger_watch.rs` (`act_on`).
2. 03 before 07: `workers.rs`. Without 03, a refused worker can hide behind a 24 h wait.
3. 06 before 07, and 07 before 08: `cmd/taskledger.rs`.
4. 03 and 06 before 05, and 05 before 04: `watchdog.rs` and `taskledger_watch.rs`.
5. 08 before 02: `spawn.rs` and `tasks.rs`. Spec 08 adds `refuse_before_launch`. Spec 02 then
   changes its first test. The plan refusal always runs before the disk floor.
6. 09 before 01: the `check_in` call in `finish/land.rs`. A sandbox setup failure is reported before
   a tip comparison. The scratch folder lives until the second `HEAD` read.
7. 06 before 10: `fleet-orchestrator.md` and `golden_prompts.rs`.
8. 10 before 01: `runs.rs` and `cmd/taskledger.rs`.

## Shared files

| File of `main` | Specs |
|---|---|
| `crates/horch-core/src/taskledger/watchdog.rs` | 03, 04, 05, 06 |
| `crates/horch/src/cmd/taskledger_watch.rs` | 03, 04, 05, 06 |
| `crates/horch/src/cmd/taskledger.rs` | 01, 06, 07, 08, 10 |
| `crates/horch-core/src/taskledger/tasks.rs` | 02, 06, 08 |
| `crates/horch-core/src/taskledger/mod.rs` | 06, 07, 10 |
| `crates/horch-core/src/taskledger/workers.rs` | 03, 07 |
| `crates/horch-core/src/taskledger/runs.rs` | 01, 10 |
| `crates/horch-core/src/taskledger/finish/land.rs` | 01, 09 |
| `crates/horch/src/cmd/spawn.rs` | 02, 08 |
| `crates/horch-core/src/runtime/mod.rs` | 01, 08 |
| `crates/horch-core/src/harness/codex.rs` | 01, 07 |
| `teammates/_base/fleet-orchestrator.md` | 06, 10 |
| `crates/horch-core/tests/golden_prompts.rs` | 06, 10 |
| `crates/horch-e2e/tests/taskledger.rs` | 02, 04 |
| `crates/horch-e2e/tests/watchdog_liveness.rs` | 04, 05 |

## How to reproduce and validate

Write the failing test first. Each test fails on `main` for the reason its spec gives.

| Fix | Failing test to write first | Manual test in 1 line | Ledger count after 2 weeks |
|---|---|---|---|
| 01 | `h01_a_check_cannot_read_home_or_write_outside_its_folder` | A gate that reads a home file fails. | `no sandbox` is 0, and failed checks stay below 5%. |
| 02 | `h02_a_spawned_worker_keeps_its_task_when_another_persona_spawns_next` | The second conflicting spawn is refused. | Workers ended `no_task` stay 0. |
| 03 | `h03_r1_background_wait_stops_counting_after_two_hours` | A forced watchdog check 3 h later gives `no_progress`. | Longest gap after a wait is at most 150 min (750 for a question). |
| 04 | `a_live_hung_watchdog_is_stopped_and_replaced` | A stopped watchdog is replaced by the next command. | Count of `Watchdog hung:` events. |
| 05 | `h05_r5_a_worker_with_all_3_gone_is_reaped_before_its_600_s_check` | A pass reaps a worker whose PIDs and pane are gone. | Longest `already_gone` delay is at most 90 s. |
| 06 | `h06_a_worker_at_a_usage_limit_is_not_reaped` | A worker at a limit is held, with no `reaped` event. | 0 `dead:` workers end on a limit record. |
| 07 | `h07_r3_refuses_a_working_worker_with_no_wait` | A worker that stops with no wait gets 1 refusal. | 0 reaps in the class "ended its turn to wait". |
| 08 | `h08_r2_spawn_is_refused_below_the_floor` | `horch ledger worktree new` at 1 GB free exits 1. | No real disk-full event. |
| 09 | `h09_r2_a_commit_during_the_land_check_does_not_land` | The first finish stops, and the second lands. | Count of `land_red` events with `tip moved`. |
| 10 | `h10_r4_close_refuses_a_ticket_with_no_task` | Close refuses, and `--leave-out` closes. | Tickets with no match stay at 52 or fall. |

## Baseline to measure again

Measured on 2026-11-23T02:51Z with `sqlite3 -readonly ~/.horch/ledger.sqlite3`. Live fleets add rows,
so these counts are newer than the counts in the specs (for example 51 reaps, not 49).

| Count | Today | Query |
|---|---|---|
| Claims | 4106 | `select count(*) from events where kind='claimed'` |
| Reaps | 51 | `select count(*) from workers where end_reason like 'dead:%'` |
| Mean minutes to a reap | 116.3 | `select avg((julianday(ended_at)-julianday(last_progress_at))*1440) from workers where end_reason like 'dead:%'` |
| `no_task` workers, all | 85 of 4275 | `select sum(status='no_task'), count(*) from workers where kind='worker'` |
| `no_task` workers since the gate | 0 of 518 | Add `and spawned_at >= '2026-11-06T19:46:00Z'` to the query above |
| Silent reaps | 30 | `select count(*) from workers where end_reason like '%no ledger call, no transcript growth, no state change%'` |
| Usage-limit ends | 0 | `select count(*) from workers where end_reason like 'usage limit:%'` |
| Dead-agent checks | 11 | `select count(*) from checks where note='the agent process is gone'` |
| Task checks, failed | 519, 14 | `select count(*), sum(status='failed') from task_steps where name in ('check','recheck') and status in ('passed','failed')` |
| Run gates, failed | 22, 3 | `select count(*), sum(exit_code<>0) from run_gates` |
| Red lands, tip moved | 33, 0 | `select count(*), sum(text like '%tip moved%') from events where kind='land_red'` |
| Hung watchdog events | 0 | `select count(*) from events where kind='note' and text like 'Watchdog hung:%'` |
| Floor refusals | 0 | `select count(*) from workers where status='launch_failed' and end_reason like '%below the floor%'` |
| Tickets left out | 0 | `select count(*) from events where kind='note' and text like 'ticket left out:%'` |

Spec 06 query 2 classifies usage-limit reaps, spec 05 query B gives the PID delay, and the script in
spec 10 counts tickets with no match (52 of 412 today).

## What is not ported, and why

1. The daemon: the orchestrator workflow stays, and the daemon is 68,849 lines.
2. The idea queue and idea-leads: they replace the orchestrator conversation.
3. The operator token: it protects daemon commands that `main` does not have.
4. Bundles and stacks: 12,157 lines for a 2.7% land-conflict rate.
5. Windows work: the ledger has 0 Windows hosts, and fleetd on Windows builds but does not work.
6. The socket channel: it exists only for the daemon.

## Open questions for the operator

Each answer below is the recommendation of the spec.

1. **G1 (this guide).** Should the forbidden-variable leak in `horch worktree land --check` be fixed
   now, apart from spec 01? Recommended: yes. It breaks the first project rule.
2. **01-Q1.** Default network `none`, with `host` or `open` per task? Recommended: yes. A macOS check
   with loopback test servers then needs `host`.
3. **01-Q2.** Does Windows stop each check until the operator sets `unsandboxed`? Recommended: yes.
4. **01-Q3.** Hidden home, or the smaller deny list of fleetd? Recommended: hidden home.
5. **02-Q1.** Should fix 02 land, with 0 occurrences since 2026-11-06? Recommended: only after the
   fixes with measured harm.
6. **02-Q2.** Is the priority inversion acceptable? Recommended: yes.
7. **03-Q1.** Is 12 h the right question cap? Recommended: yes.
8. **03-Q2.** Does a reap after a wait cap use 1 of 3 attempts? Recommended: yes.
9. **04-Q1.** Should each supervisor also trigger the hang check? Recommended: a separate spec.
10. **04-Q2.** Is a detection delay of up to 600 s acceptable? Recommended: yes.
11. **05-Q1.** Should a reap keep a pane that still runs an agent? Recommended: a separate ticket with
    FM-13.
12. **05-Q2.** Should a ticket find out why 5 requeued tasks have no later claim? Recommended: yes.
13. **06-Q1.** Should horch move a limited task to a fallback on another pool without asking?
    Recommended: yes.
14. **06-Q2.** Who resumes a held worker? Recommended: the orchestrator.
15. **07-Q1.** Do you accept 1 extra model turn when a worker stops with no wait? Recommended: yes.
16. **07-Q2.** Should spec 07 wait for spec 03? Recommended: yes.
17. **08-Q1.** Is 20 GB the right default floor? Recommended: yes.
18. **08-Q2.** Should a follow-up add ranked sizes and `target` to triage? Recommended: yes.
19. **08-Q3.** Should `horch worktree new` also check the floor? Recommended: no.
20. **09-Q1.** Must the land refuse uncommitted edits made during a check? Recommended: no.
21. **10-Q1.** Should horch report a ticket folder that no run links? Recommended: no.
22. **10-Q2.** Should a changed ticket refuse the close? Recommended: no.

## How to run this as a fleet

Run 1 wave at a time. Start with wave 1, and measure before you approve a later wave.

1. Write 1 ticket for each fix in `docs/tickets/fleetd-harvest/`, with the format of
   `docs/tickets/trace-fixes/`. Each ticket names its spec, its `**Own:**` files and a `**Check:**`.
2. Write `docs/tickets/fleetd-harvest/MAP.md`. Set `Blocked by` from the pairs in `## Order`.
3. Start the fleet: `horch fleet --verify full`.
4. Create the run: `horch ledger run new fleetd-harvest`.
5. Test the map: `horch ledger add-map --dry-run docs/tickets/fleetd-harvest/MAP.md`.
6. Queue it: `horch ledger add-map docs/tickets/fleetd-harvest/MAP.md`.
7. Run `horch ledger plan-workers`, then `horch spawn <persona>` with no task text, as many as it
   prints. Run it again when a ticket lands.
8. Close the wave with `horch ledger run gate --check "<full check>"`, then the run pull request.
   A merge to `main` publishes a release, so a user-visible fix needs its CHANGELOG line.
9. After 2 weeks, repeat the queries of `## Baseline to measure again` before the next wave.

## What was not verified

No build and no test was run for these specs. No fix was implemented. Sizes are estimates.

A step is 1 numbered item, or 1 named test or ledger count, in `## Reproduce` or `## Validate`.

| Spec | `RUN` | `NOT RUN` |
|---|---|---|
| 01 | 1 | 32 |
| 02 | 0 | 23 |
| 03 | 0 | 24 |
| 04 | 2 | 21 |
| 05 | 2 | 18 |
| 06 | 3 | 20 |
| 07 | 3 | 20 |
| 08 | 3 | 17 |
| 09 | 0 | 15 |
| 10 | 0 | 27 |
| Total | 14 | 217 |

Each `RUN` step is a read-only query or a read of code or help text. The real Claude and Codex hook
behavior of spec 07, the Codex pane text of spec 06, and the macOS network rule of spec 01 on `main`
need a live check before release.
