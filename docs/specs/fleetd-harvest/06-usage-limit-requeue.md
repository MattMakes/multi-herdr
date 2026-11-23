# 06: Detect a usage limit and requeue with no attempt lost

Status: proposed. Target: `main` at `06930d95`. Source of the idea: fleetd at `9bc3bf70`.
Report id: FM-5. Size: M.

Labels: `CODE` is read at the pinned commit. `MEASURED` is a read-only query or file read on
2026-11-22. `REPORT` is a report claim that was not checked. `NOT VERIFIED` has no proof.
Paths: `TL` is `crates/horch-core/src/taskledger`. `CMD` is `crates/horch/src/cmd`. `check` is a `watchdog check`.

## Problem

The problem is real on MAIN, and it is larger than report R6 states.

A worker can stop at a provider usage limit while its process stays alive. The worker makes no
progress, so the watchdog treats it as stalled. The watchdog stops the worker, even when the limit
resets a few minutes later. The worker loses its conversation, and its task loses 1 attempt. A task
that loses all 3 attempts is held for the operator.

The exact cause in the code of MAIN:
1. `CODE TL/watchdog.rs:330`: `proof` looks only for movement. It has no usage-limit signal.
2. `CODE TL/watchdog.rs:372`: `stalled` gives `no_progress` when nothing moved.
3. `CODE TL/watchdog.rs:419`: `stall_round` gives `dead` when `round >= run.stall_rounds`.
4. `CODE CMD/taskledger_watch.rs:292`: `reap_dead` calls `stop::reap` for the `dead` verdict.
5. `CODE TL/tasks.rs:1007`: a claim adds 1 to `attempts`.
6. `CODE TL/tasks.rs:1300`: `requeue` computes `used = attempts - attempt_floor`, and holds the task
   when `used >= max`. No end reason removes an attempt from this count.
7. `CODE teammates/_base/fleet-orchestrator.md:131`: the briefing says "Never reap or replace a
   worker that shows the usage-limit message". The watchdog does not obey this rule.

The ticket and R6 need 2 corrections:
1. The reap does not come after 10 minutes. The default is 3 checks of 600 s
   (`CODE TL/schema.rs:45`, `CODE TL/schema.rs:46`). The measured delay is 30 to 65 minutes.
2. The text "Codex quota is exhausted; no progress in 10 min" is not in the code of MAIN. An
   orchestrator typed it as a reap reason. The 18 Claude cases have a generic stall reason, so a
   text search does not find them.

## Evidence on main

All counts are `MEASURED` on 2026-11-22 at 23:17 UTC, from `~/.horch/ledger.sqlite3`, opened
read-only. The ledger covers 2026-10-31 to 2026-11-22.

Query 1 counts the reaps by actor. Query 2 reads the last conversation record of each reaped
Claude worker.

```sh
sqlite3 -readonly ~/.horch/ledger.sqlite3 \
  "SELECT actor, COUNT(*) FROM events WHERE kind='reaped' GROUP BY actor;"
# orchestrator|9
# watchdog|40

sqlite3 -readonly ~/.horch/ledger.sqlite3 "SELECT transcript_path FROM workers
  WHERE harness='claude' AND status='incomplete' AND end_reason LIKE 'dead:%'
  AND transcript_path IS NOT NULL" | while IFS= read -r p; do [ -f "$p" ] && jq -c -R \
  'fromjson? | select(.type=="assistant" or .type=="user") | [.isApiErrorMessage, .error]' "$p" \
  | tail -n 1; done | sort | uniq -c
#   18 [null,null]
#   18 [true,"rate_limit"]
```

Results, all `MEASURED`:
1. 27 of 49 reaps (55%) hit a usage limit: 18 Claude reaps by the watchdog and 9 Codex reaps by an
   orchestrator. The 9 Codex workers have no transcript, so only the orchestrator states the limit.
2. The 18 Claude workers were reaped 30 to 65 minutes after the limit record (median 31).
3. All 18 were reaped before their limit reset, with 15 to 7594 minutes left (median 26).
4. Each of the 27 tasks lost 1 attempt and 1 agent conversation, and used 2 attempts: 24 are `done`
   and 3 are `cancelled`. No task reached 3 attempts, so the hold in the ticket has a count of 0.
5. 14 Claude tasks have a fallback on the same pool, 4 have no fallback, and 9 Codex tasks have a
   fallback on Claude. With this fix, 18 tasks hold and 9 tasks move.

Example 1 (`MEASURED`), worker `919f5324`, role `opus-42`, task T-13:

```text
22:33:35Z claimed (attempt 1)
22:43:29Z transcript: error=rate_limit apiError=usage_limit_reached status=429
          "You've hit your 5-hour limit · resets 4:40pm (America/Phoenix)"
23:13:54Z watchdog reaped: no ledger call, no transcript growth, no state change ...
23:40:00Z the limit resets (resetsAt 1794094800), 26 minutes after the reap
```

Signals for each harness:
1. Claude, `MEASURED`: the record has `type="assistant"`, `isApiErrorMessage=true`,
   `error="rate_limit"`, `apiErrorStatus=429` and `quotaLimits.resetsAt` (Unix seconds). All 18
   records have `resetsAt`. The MAIN fixture has the same form at
   `crates/horch-core/tests/fixtures/telemetry/claude/-work-alpha/11111111-1111-4111-8111-111111111111.jsonl:6`.
2. Claude false positives, `MEASURED`: 3082 worker transcripts hold 21 `rate_limit` records and 1
   `server_error` record. 2 workers continued after a limit record, and the rule "the last
   conversation record is the limit" excludes them. Only the 18 reaped transcripts end on a limit.
3. Codex text, `MEASURED`: the Codex CLI 0.166.0 binary holds `You’ve hit your usage limit`, with a
   curly apostrophe. The MAIN fixture rollout has a straight apostrophe.
4. Codex transcript, `MEASURED`: 38 of 1073 Codex workers have a transcript path, and 0 of the 83
   newest workers (week 44 of 2026 and later) have one. So the fix reads the pane for Codex.
5. Antigravity, OpenCode, Pi and Prime: `NOT VERIFIED`. The ledger holds 11 Antigravity workers with
   no transcript, and 0 workers with the other 3 harness names.

## How fleetd solves it

The rule at FINISH: a usage limit is a state and not a stall, so it burns no attempt. The task
restarts on a fallback in another pool, or it waits for the pool to reset.

Paths below start at `crates/horch-core/src/` at `9bc3bf70`.

1. `CODE harness/claude.rs:1828`, `harness/codex.rs:1758`, `harness/antigravity.rs:339`: each
   adapter reads the limit from the exec stream of its CLI.
2. `CODE fleetd/task_execution/domain/quota.rs:71`: `exit` restarts on a fallback in another pool
   that is not limited, or else waits. `quota.rs:83`: `released` ends the wait at the reset time.
3. `CODE fleetd/fleet_runtime/application/pass.rs:2259`: the pass calls `on_quota` and returns
   before the code that burns an attempt.

The idea to port: detect the limit, keep the attempt, use the ticket's fallback only when it is on
another pool, and wait for the reset. These parts exist only because fleetd is a daemon: the exec
stream wrapper, `exit.json` hints, the `waiting_quota` task state, the paused lease and the pass.

Do not copy these fleetd defects:
1. `CODE harness/codex.rs:1750` matches a straight apostrophe. Codex 0.166.0 prints a curly one.
2. `CODE harness/claude.rs:1838` treats each 5xx status as a usage limit. An outage is not a limit.
3. `CODE fleetd/task_execution/domain/stall.rs:151` ends an attempt from the pool reading alone,
   also when the agent works. Quota data must not stop a worker.

## Fix

Behavior:
1. `H06-R1` Claude detection. Read the last 256 KiB of `transcript_path`. Take the last record whose
   `type` is `assistant` or `user`. It is a limit when `isApiErrorMessage` is true, `error` is
   `rate_limit` and `apiErrorStatus` is 429. Read the reset from `quotaLimits.resetsAt`, then from
   `apiErrorParams.rate_limit_info.resetsAt`. A missing value is `None`.
2. `H06-R2` Codex detection. Read the last 12 non-empty lines of the pane. Replace `’` with `'`.
   Remove leading characters that are not letters. It is a limit when a line starts with
   `You've hit your usage limit` and the pane has no running timer. The reset is `None`.
3. `H06-R3` Detection scope. Run detection only when `proof` returns `None`. Other harnesses return
   no limit and keep the behavior of today.
4. `H06-R4` Move. Move the task when it has a `fallback_persona`, that persona differs from the
   worker persona, both are in the roster, and `pool_for` gives 2 different known pools. Stop the
   worker with the end reason `usage limit: <text>`. Requeue the task, set `claimable_by` to the
   fallback, and wake the orchestrator with the spawn line. Do not read a quota value for this rule.
5. `H06-R5` Hold. In each other case, keep the worker alive. Write a check row with verdict
   `no_progress`, round 0 and the note `usage limit until <UTC time>: <text>`. The hold ends at the
   reset. With no reset, it ends 6 h after detection. Wake the orchestrator 1 time.
6. `H06-R6` End of hold. At the first check at or after the end, wake the orchestrator 1 time with
   `horch tell <role> "[orchestrator] The usage limit has reset. Continue your task."`. Then the
   normal stall count runs. A record of progress ends the hold at any time.
7. `H06-R7` No attempt lost. `tasks::requeue` computes `used = attempts - attempt_floor - limit_ends`,
   with no cap. `limit_ends` counts the workers of the task with `attempt > attempt_floor` and an end reason that starts with `usage limit:`.
8. `H06-R8` Reaps at a limit. When the agent is alive and detection finds a limit, the watchdog reap
   and `horch ledger reap` end the worker with `usage limit: <text>`, and not with `dead:`.
9. `H06-R9` Briefing. Replace the sentence at `teammates/_base/fleet-orchestrator.md:131` with:
   "horch detects a usage limit, keeps the attempt and wakes you. Do not reap a worker at a limit."
10. `H06-R10` Repeat notice. From the third limit end of 1 task, each wake names the count and asks the orchestrator to check for a false signal. The notice does not change attempts and does not hold the task.

Files of MAIN to change:
1. New `TL/usage_limit.rs`: `UsageLimit { text, resets_at }` and `detect(worker, screen, timer_s)`.
   Add `pub mod usage_limit;` to `TL/mod.rs`.
2. `TL/watchdog.rs`: in `check_worker`, run the hold logic after `proof` returns `None`. Read the
   latest `usage limit until` check row that is newer than `last_progress_at`. Add the limit action
   to `Checked`. A hold skips `stall_round`.
3. `TL/stop.rs`: add `end_at_limit`. It shares the body of `reap`, uses the reason
   `usage limit: <text>`, and writes no `reaped` event.
4. `TL/tasks.rs`: change `requeue` for `H06-R7`. Add `fallback_as` with an `Actor` argument, and let
   `fallback` call it with `Actor::Orchestrator`.
5. `CMD/taskledger_watch.rs`: in `act_on` and `reap_dead`, apply the move, the wakes and
   `end_at_limit`. Load the roster with `load_roster_unwarned` (`CODE CMD/mod.rs:52`).
6. `CMD/taskledger.rs:1398`: in `reap_cmd`, apply `H06-R8`. The briefing file: apply `H06-R9`, and update its 3 pins at `crates/horch-core/tests/golden_prompts.rs:543`, `:1571` and `:1583` in the same commit.

New settings: none. The fix adds these constants in `TL/usage_limit.rs`:
1. `TAIL_BYTES = 262144`. It bounds the read of a large transcript.
2. `SCREEN_LINES = 12`. The text is near the pane bottom, and older output must not match.
3. `UNKNOWN_HOLD_S = 21600`. 6 h exceeds the 5-hour Claude window. Codex is `NOT VERIFIED`.
4. `MAX_HOLD_S = 691200`. 8 days clamps a bad reset value and is longer than the 7-day window.
5. `LIMIT_ENDS_NOTICE = 3`. It equals the default `max_attempts`. It only starts the `H06-R10` notice.

Not ported:
1. The daemon, the exec wrapper, the `waiting_quota` state and leases.
2. A quota reading as a trigger or as a choice of teammate. The operator rule forbids it.
3. Login and error prompts. `MEASURED`: 3082 Claude transcripts hold 0 authentication error records
   and 1 `server_error` record, and that worker continued.
4. An agent process that exits at a limit. The supervisor requeue stays as it is.

Effect at the upgrade:
1. Ledger: no schema change. `VERSION` stays 9 (`CODE TL/schema.rs:8`). Verdicts, event kinds, wait
   kinds and outbox events stay inside their current `CHECK` lists.
2. Old rows: `MEASURED` 0 workers have an end reason that starts with `usage limit:`, so old attempt
   counts do not change.
3. Live run: the fix applies when the watchdog restarts on the new binary. A worker at a limit is
   held at its next stalled check, and its earlier stall rounds reset to 0.

## Reproduce

Automated. Crate `horch-core`. Add both tests to the `tests` module at `TL/watchdog.rs:955`. Reuse
`setup` (line 973), `live` (line 1014), `pass_time` (line 1023) and the loop of
`three_stalled_checks_are_dead_and_progress_resets_the_count` (line 1094). Both tests compile on
MAIN.

A1. `NOT RUN` (the run rules forbid cargo). By the code, MAIN fails with `left: "dead"`.

```rust
#[test]
fn h06_a_worker_at_a_usage_limit_is_not_reaped() {
    let (_d, mut s, run, w) = setup();
    let text = "■ You’ve hit your usage limit. Upgrade to Pro to continue.\n";
    let at_limit = || live(Some(text.to_string()), "idle");
    let obs = Script(RefCell::new(vec![at_limit(), at_limit(), at_limit()]));
    let mut last = String::new();
    for _ in 0..3 {
        let now = pass_time(&s, &w, 600);
        last = check_run_at(&mut s, &run, &obs, false, now).unwrap()[0].verdict.clone();
    }
    assert_ne!(last, "dead");
}
```

A2. `NOT RUN`. By the code, MAIN returns `Requeue::Held`, so the assertion fails.

```rust
#[test]
fn h06_a_usage_limit_end_costs_no_attempt() {
    let (_d, mut s, run, w) = setup();
    let task = workers::get(s.conn(), &w).unwrap().task_id.unwrap();
    s.conn().execute("UPDATE runs SET max_attempts = 1 WHERE run_id = ?1", [&run.run_id]).unwrap();
    let out = s.immediate(|tx| {
        workers::end(tx, &w, crate::taskledger::model::WorkerStatus::Incomplete,
            "usage limit: test", None)?;
        tasks::requeue(tx, &task, "usage limit: test")
    }).unwrap();
    assert!(matches!(out, tasks::Requeue::Queued));
}
```

Manual:
1. `RUN` Run query 1 of `## Evidence on main`. The output is `orchestrator|9` and `watchdog|40`.
2. `RUN` Run query 2 of `## Evidence on main`. The output has `18 [true,"rate_limit"]`.
3. `RUN` Read the events of worker `919f5324` with `sqlite3 -readonly`, for the kinds `claimed`,
   `requeued` and `reaped`. The output is `22:33:35Z claimed`, `23:13:54Z requeued` and
   `23:13:54Z reaped` by the actor `watchdog`, all on 2026-11-07.
4. `NOT RUN` Live fault. Use a scratch run on an account that is at its limit. Spawn 1 worker. Wait
   until its pane shows the limit. Poll `horch ledger show T-1` until 3 due watchdog checks are recorded, for at most 70 minutes. MAIN then shows a `reaped` event, and the next claim prints `attempt 2`.

## Validate

Each test goes in the `tests` module of the file it changes. Each is `NOT RUN`, because the run
rules forbid cargo. "MAIN" is the result today, from reading the code. "No build" means that the
test does not compile on MAIN, because `usage_limit` does not exist.

1. `H06-R1` `h06_r1_claude_limit_record_gives_reset`: fixture and `resetsAt` record. MAIN: no build.
2. `H06-R1` `h06_r1_a_record_after_the_limit_clears_it`: expect `None`. MAIN: no build.
3. `H06-R2` `h06_r2_codex_screen_curly_and_straight`: both match. MAIN: no build.
4. `H06-R2` `h06_r2_quoted_text_or_a_running_timer_is_no_limit`: a mid-line quote, a line above the
   last 12, and a running timer give `None`. MAIN: no build.
5. `H06-R3` A1 passes, and `h06_r3_pi_at_the_same_text_is_reaped` gives `dead`. MAIN: A1 fails. The
   Pi test passes on MAIN and only guards the unchanged behavior.
6. `H06-R4` `h06_r4_other_pool_fallback_moves_the_task`: the task is queued for the fallback.
   MAIN: reaped on the third check, and `claimable_by` does not change.
7. `H06-R4` `h06_r4_same_pool_fallback_holds`: `opus` to `opus-domain` does not move. MAIN: reaped.
8. `H06-R5` `h06_r5_hold_writes_round_0_and_wakes_once`: 3 checks give 1 wake. MAIN: `dead`.
9. `H06-R6` `h06_r6_end_of_hold_wakes_once_then_counts`: after the reset, 1 wake, then `dead` after
   3 more stalled checks. MAIN: `dead` before the reset.
10. `H06-R7` A2 passes, and `h06_r7_a_fourth_limit_end_costs_no_attempt`. MAIN: A2 fails with `Held`.
11. `H06-R8` `h06_r8_reap_cmd_at_limit_ends_as_usage_limit`: the prefix is `usage limit:`, with no
    `reaped` event. MAIN: the prefix is `dead:`, with a `reaped` event.
12. `H06-R9` `h06_r9_briefing_names_the_limit_rule`: MAIN fails, because the sentence is absent.
13. `H06-R10` `h06_r10_third_limit_end_adds_the_notice`: the task stays queued. MAIN: no notice.

Manual after the fix:
1. `NOT RUN` Repeat manual step 4 of `## Reproduce` with no fallback on another pool. Expect a
   `usage limit until` note, 1 wake and no `reaped` event. MAIN: `reaped`.
2. `NOT RUN` After the reset, run the `horch tell` line of the wake. Expect transcript growth and a
   `proxy_checkin` or `progress` verdict. MAIN: the worker no longer exists.
3. `NOT RUN` Repeat with a Codex worker whose ticket has a Claude fallback. Expect `claimable_by` to
   change and a spawn line in the wake. MAIN: the task stays on Codex.

Ledger count after 2 weeks of use: run query 2 for workers with `ended_at` after the upgrade. Expect
0 `[true,"rate_limit"]` rows among `dead:` workers. MAIN: 18. Then count the workers with
`end_reason LIKE 'usage limit:%'`. MAIN: 0. A value above 0 shows that the fix acted.

## Size and risks

Estimate: M, 2.5 worker-days: 1 day for detection, 1 day for the watchdog states and the attempt
rule, and 0.5 day for the commands, the briefing and a live check.

Risks:
1. `R1` A Claude release changes the record. Detection returns `None`, and today's behavior returns.
2. `R2` The Codex pane rendering is `NOT VERIFIED`. The text is `MEASURED` in the binary, but no real
   pane capture exists. Do manual step 3 of `## Validate` before release.
3. `R3` A false Codex match holds a hung worker for at most 6 h per cycle, and it costs no attempt. No cap ends the repeat. Each cycle needs a new spawn, and `H06-R10` tells the orchestrator.
4. `R4` A held task can wait up to 7 days on a weekly limit. The first wake gives the reset time.
5. `R5` The move does not read the fallback pool. A fallback at its own limit holds, and the task
   does not move back, because the worker persona equals the fallback persona.

Files that other specs of this run can also change:
1. `TL/watchdog.rs`: specs 03, 04 and 05. This fix uses no wait row, so the cap of spec 03 does not
   end a hold.
2. `CMD/taskledger_watch.rs`: specs 04 and 05. `TL/tasks.rs`: spec 02 changes the claim.
3. Spec 07 refuses an end of turn with an open task. Its Stop hook must let an agent at a limit stop.

Open questions for the operator:
1. `Q1` Should horch move a limited task to a fallback on another pool without asking? I recommend
   yes, because the fallback is already a ticket field. It changes the briefing rule "never replace
   a worker at a limit" for cases such as the 9 Codex tasks.
2. `Q2` Should the orchestrator resume a held worker, or should the watchdog type the line itself?
   I recommend the orchestrator, because the wake path exists and the change stays small.
