# 03: Cap the waits that hide a stalled worker

Status: proposed. Target: `main` at `06930d95`. Source of the idea: fleetd at `9bc3bf70`.
Report id: FM-3. Size: S.

Path short forms: `TL` is `crates/horch-core/src/taskledger`. `CMD` is `crates/horch/src/cmd`.
`F18` is `crates/horch-e2e/tests/fleet18_messaging.rs`. `FD` is
`crates/horch-core/src/fleetd` at FINISH.

Labels: `CODE` (read at the pinned commit), `MEASURED` (read-only query, 2026-11-22), `REPORT`,
`NOT VERIFIED`. No `cargo` command ran for this spec, so each test step is `NOT RUN`.

## Problem

The problem is real on MAIN, but the ledger does not prove that a waiting agent was stuck.

A worker can declare that it waits for a build, a review or an answer. While the wait is open, the
watchdog records progress and does not count a stall. If the agent stops responding during the
wait, nothing tells the operator until the wait ends. That time is up to 24 h for a background wait
and up to 7 days for a review or question wait. The worker holds the task for all of this time.

The exact cause:

1. `proof()` returns `Verdict::Progress` for any open wait, before it reads a real signal. The
   condition is `!look.waits.is_empty()`. `CODE TL/watchdog.rs:333`
2. A `waiting:` progress line opens a background wait of 24 h. `CODE TL/workers.rs:377`
3. A code-review wait and a comment-sweep wait last 7 days. `CODE TL/finish/codesweep.rs:618`,
   `CODE TL/finish/comments.rs:96`. A wait held for finish uses the same limit.
   `CODE TL/workers.rs:580`
4. A question wait lasts 7 days from the question. `CODE TL/workers.rs:768`
5. No code checks during the wait that the agent can still respond. `CODE TL/watchdog.rs:330`

The stall steps after a wait already work. When a wait ends, the next checks give `no_progress`,
`no_progress`, `dead`. `CODE F18:339`

## Evidence on main

All counts are `MEASURED` on 2026-11-22 with `sqlite3 -readonly ~/.horch/ledger.sqlite3`. The
ledger covers 2026-10-31 to 2026-11-22.

Query A splits the reaps by whether the worker ever had a wait check:

```sql
WITH r AS (SELECT (julianday(ended_at)-julianday(last_progress_at))*1440 AS gap,
  EXISTS(SELECT 1 FROM checks c WHERE c.worker_id=w.worker_id AND c.note LIKE 'waiting:%') AS waited
  FROM workers w WHERE end_reason LIKE 'dead:%')
SELECT waited, count(*), round(avg(gap),1), round(max(gap),1), round(sum(gap)) FROM r GROUP BY waited;
```

```text
0|40|26.9|39.7|1077.0
1|9|534.1|1431.4|4807.0
```

- F1. The ledger has 49 reaps in 4103 claims. The mean gap from last progress to reap is 120.1 min.
- F2. The 40 reaps with no wait have a mean of 26.9 min, the detection time of 3 checks at 600 s.
- F3. The 9 reaps with a wait hold 4807 of 5884 gap minutes (81.7%).
- F4. Worker `2af0cac1` (T-925) has a gap of 1047 min, but a wait did not cause it. Its last 2 checks
  are 62259 s apart, so no watchdog ran. Spec 04 and FM-7 own that fault.
- F5. Five long gaps (210 to 1431 min) ended with "the agent process is gone". Each worker waited for
  the operator or for other work. Four of them ended between 03:44 and 03:51 on 2026-11-17. The cause
  of that group is `NOT VERIFIED`.
- F6. No count exists of stuck agents. The ledger does not record whether an agent could respond.
- Example: worker `82e75d24`, role `skill-importer-1`, T-2: last progress 2026-11-16T03:53:00Z, reaped
  2026-11-17T03:44:26Z, 185 wait checks. Reason: "T-3 second spawn.rs runs_in fix".
- Example: worker `7f151179`, role `spec-writer-1`, T-6: last progress 2026-11-16T04:28:17Z, reaped
  2026-11-17T03:50:59Z, 139 wait checks on 1 open question.

Query B counts unbroken wait streaks by kind. The kind comes from the check note, so the split is
close but not exact.

```sql
WITH c AS (SELECT worker_id, check_id, at,
  CASE WHEN note NOT LIKE 'waiting:%' THEN NULL
       WHEN note LIKE 'waiting: #%' THEN 'question'
       WHEN note LIKE '%reviewer tasks%' OR note LIKE '%reviews are in%'
         OR note LIKE '%comment-sweep%' THEN 'reviewers'
       WHEN note LIKE '%finish runs detached%' THEN 'finish'
       ELSE 'background' END AS k FROM checks),
g AS (SELECT *, sum(k IS NULL OR k IS NOT p) OVER (PARTITION BY worker_id ORDER BY check_id) AS n
      FROM (SELECT *, lag(k) OVER (PARTITION BY worker_id ORDER BY check_id) AS p FROM c)),
s AS (SELECT k, (julianday(max(at))-julianday(min(at)))*1440 AS m
      FROM g WHERE k IS NOT NULL GROUP BY worker_id, k, n)
SELECT k, count(*), sum(m>=120), sum(m>=720), round(max(m)) FROM s GROUP BY k;
```

| Kind | Streaks | At least 2 h | At least 12 h | Longest (min) |
| --- | --- | --- | --- | --- |
| background | 99 | 10 | 3 | 1845 |
| finish | 92 | 0 | 0 | 60 |
| question | 15 | 2 | 1 | 1384 |
| reviewers | 106 | 5 | 1 | 852 |

## How fleetd solves it

The rule: a declared wait counts as activity only while something live is behind it, and for at
most a fixed time after the last proof.

- `judge` and `waiting_until` decide if a wait holds. `CODE FD/task_execution/domain/waits.rs:133`, `:159`
- A proof moves the limit to the clock plus `max_wait_s`, default 3600 s.
  `CODE FD/worker_profiles/domain/profile.rs:72`
- A live review task or a running finish step is a proof. An open question is not a proof, so a
  question wait ends at its first limit. `CODE .../waits.rs:133`
- The pass treats a held wait as activity. `CODE FD/fleet_runtime/application/pass.rs:2475`

The idea to port is the bounded wait that only a live proof can extend. The lease tables, the fleet
clock, the `due_at` column, the 5 s pass and the stall ladder rungs exist only because fleetd is a
daemon.

## Fix

The fix keeps `proof()` unchanged. It makes each wait short, extends a review wait only while a
worker holds a review, and asks the waiter to respond before horch reaps it.

### Requirements

1. H03-R1. A `waiting:` progress line opens a background wait of 2 h.
2. H03-R2. A new reviewers wait (code review, comment sweep, held for finish) lasts 2 h.
3. H03-R3. At each watchdog check, if at least 1 task of a reviewers wait has status `running` or
   `landing`, horch sets the deadline to now plus 2 h. Otherwise horch sets the deadline to the
   earlier of the current deadline and now plus 2 h.
4. H03-R4. A question wait lasts 12 h from the time of the question. `sync_questions` uses the newest
   open question of the role. When that question is older than 12 h, `sync_questions` clears the
   wait and does not write it again.
5. H03-R5. The first `no_progress` check after a wait (round 1) tells the waiter that its wait ended
   and what to run. It does not wake the orchestrator. horch records 1 `Wait cap:` event.
6. H03-R6. The second `no_progress` check after a wait (round 2) wakes the orchestrator and names the
   worker, the task and the ended wait.
7. H03-R7. The third `no_progress` check reaps the worker through the existing path.
8. H03-R8. A ledger call before round 3 resets the rounds. A `waiting:` line opens a new 2 h wait.
9. H03-R9. A stall with no wait before it keeps the behavior of MAIN: round 1 wakes the orchestrator.
10. H03-R10. The detached finish wait keeps its limit of `3 * check + 3600` s.
11. H03-R11. A dead agent process is still `dead` at the first check, with or without a wait.

### Files of MAIN to change

1. `TL/workers.rs`
   - Add `pub const BACKGROUND_WAIT_SECS: i64 = 2 * 3600;` and use it in `progress` (line 377).
   - Add `pub const QUESTION_WAIT_SECS: i64 = 12 * 3600;`. Set `REVIEWS_WAIT_SECS` to `2 * 3600`.
   - Add `pub fn renew_review_waits(conn, worker) -> Result<()>` for H03-R3.
   - In `sync_questions` (line 751), search from the newest with `.iter().rev()`, because
     `open_questions` sorts oldest first (`CODE crates/horch-core/src/messaging/mail.rs:317`).
   - In `sync_questions`, if the question is past the cap, call `clear_wait` and go to the next worker.
2. `TL/finish/codesweep.rs:618` and `TL/finish/comments.rs:96`: use `workers::REVIEWS_WAIT_SECS`.
3. `TL/watchdog.rs`
   - In `check_worker` (line 183), call `workers::renew_review_waits` before `workers::waiting`.
   - Add `after_wait: bool` and the ended wait reason to `Checked` (line 59). `after_wait` is true
     when the newest check with `round = 0` has a note that starts with `waiting:`. Read it before
     `record_check`.
4. `CMD/taskledger_watch.rs`, `act_on` (line 264): add 2 match arms before the `first_stall` arm.
   - `"no_progress" if c.after_wait && c.round == 1` calls the new `tell_waiter`.
   - `"no_progress" if c.after_wait && c.round == 2` wakes the orchestrator.
5. `CMD/taskledger_wake.rs`: add `pub fn tell_waiter`. It reuses `tell_author` (line 254) and writes
   the `Wait cap:` event. If delivery fails, it wakes the orchestrator at once. The line is:

   ```text
   [horch] NOTE: Your wait ended: <reason>. Continue your task. If you still wait, run horch ledger progress "waiting: <what>". With no ledger call, horch reaps this worker in about 20 min.
   ```

Warning: do not only shorten the 7-day value at `TL/workers.rs:768`. `sync_questions` runs before each
check and writes the question row again. `waiting()` then deletes the expired row, and `clear_wait`
sets `stall_rounds` to 0. The worker stays at round 1, is never reaped, and wakes the orchestrator at
each check. H03-R4 prevents this. MAIN does not show this loop today, because mail removes a question
at the same 7-day limit. `CODE crates/horch-core/src/messaging/mail.rs:14`, `:303`

### Settings

No new user setting and no schema change. The 3 constants are:

1. Background wait: 2 h. Of 99 streaks, 10 reached 2 h (`MEASURED`). A healthy waiter answers 1 line.
   The default detached finish limit is also 2 h (3 x 20 min + 1 h). `CODE TL/finish/land.rs:21`
2. Reviewers wait: 2 h after the last live review. A live review has its own watchdog check.
3. Question wait: 12 h. This covers a night without a reap. Of 15 streaks, 1 reached 12 h (`MEASURED`).

### Not ported

- Leases, the fleet clock and `due_at`: the `deadline` column already holds the limit.
- The child-process proof: no source shows that a background command of each harness stays in the
  agent process group (`NOT VERIFIED`). The message to the waiter works for each harness.
- A queued review as a proof: no worker is active on it. The startable wake covers a queued task.
  `CODE TL/watchdog.rs:707`

### Effect at upgrade

- The fix has no migration, so an old and a new `horch` can use the same ledger.
- A question wait is rewritten at the first pass. A question older than 12 h enters the 3-round steps.
- A reviewers wait falls to at most 2 h at its first check. A background wait keeps its old deadline.

## Reproduce

### Automated (NOT RUN)

Crate: `horch-e2e`. File: `F18`. Copy `g70_pinned_cli_wait_expires_and_dead_agent_is_immediate`
(`F18:1079`). It uses `pinned_worker` (`F18:858`) and `pinned_check` (`F18:930`).

```rust
#[test]
fn h03_r1_background_wait_stops_counting_after_two_hours() {
    use horch_core::taskledger::{model::{EventKind, ProgressSource}, workers};
    let (h, store, run, worker, _agent) = pinned_worker("h03-background");
    workers::progress(store.conn(), &worker, ProgressSource::Agent,
        EventKind::Progress, "waiting: build").unwrap();
    store.conn().execute(
        "UPDATE worker_waits SET deadline = strftime('%Y-%m-%dT%H:%M:%SZ', '2030-01-01T00:00:00Z',
           '+' || (strftime('%s', deadline) - strftime('%s', started_at)) || ' seconds'),
           started_at = '2030-01-01T00:00:00Z'", []).unwrap();
    let c = pinned_check(&h, &run, "2030-01-01T03:00:00Z");
    assert_eq!(c["verdict"], "no_progress", "{c}");
}
```

Result on MAIN: the assertion fails. The verdict is `progress`, because the deadline is
`2030-01-02T00:00:00Z`. `CODE TL/workers.rs:377`

### Manual (NOT RUN)

Warning: use a scratch project and set `HORCH_LEDGER_DB` to a scratch file.

1. NOT RUN. Start a fleet in the scratch project with 1 worker on 1 task.
2. NOT RUN. In the worker pane, run `horch ledger progress "waiting: manual test"`.
3. NOT RUN. Run `sqlite3 -readonly "$HORCH_LEDGER_DB" "select kind, started_at, deadline from worker_waits"`.
   MAIN prints a `background` row with a deadline 24 h after `started_at`.
4. NOT RUN. Leave the worker idle for 2 checks (20 min). Run `horch ledger status`. MAIN still shows
   `waiting: manual test`, and the `checks` table has verdict `progress`.

## Validate

The automated test of `## Reproduce` must pass. Add 1 test for each step below to `F18`, except where
the step names another file. "MAIN" is the result before the fix.

1. NOT RUN. H03-R1: the test of `## Reproduce`. MAIN fails with `progress`.
2. NOT RUN. H03-R2: in `TL/workers.rs`, rename `a_held_wait_with_no_earlier_wait_lasts_7_days` to
   `..._lasts_2_hours` and assert 7195 to 7200 s. MAIN fails with about 604800 s.
3. NOT RUN. H03-R3 live: a `running` review, deadline 1 min away. After 1 check, the deadline is
   about 2 h away. MAIN fails, because the deadline does not move.
4. NOT RUN. H03-R3 no proof: a queued review, 7-day deadline. After 1 check, the deadline is at most
   2 h away. MAIN fails with about 7 days.
5. NOT RUN. H03-R4 cap: copy `g70_question_wait_clears_on_orchestrator_answer_and_stalls_restart`
   (`F18:415`). Rewrite `created_at` in the question JSON file to 13 h ago. Run `sync_questions`
   before each check. Assert `no_progress`, `no_progress`, `dead`. MAIN fails with `progress`.
6. NOT RUN. H03-R4 newest: questions 13 h old and 1 h old from 1 role. Assert `progress` and the
   newest reason. MAIN fails, because it selects the oldest question.
7. NOT RUN. H03-R5: round 1 tells the waiter, sends no orchestrator wake and writes 1 `Wait cap:`
   event. MAIN fails, because round 1 wakes the orchestrator.
8. NOT RUN. H03-R6: round 2 wakes the orchestrator. MAIN fails, because round 2 sends nothing.
9. NOT RUN. H03-R8: a `waiting:` line after round 1 gives `progress` and a new 2 h deadline. MAIN
   gives `progress`, and the 2 h assertion fails with 24 h.

These 4 tests guard existing behavior. They pass on MAIN, so they do not prove the fix:

10. NOT RUN. H03-R7: the third check gives `dead` and the task is queued again.
11. NOT RUN. H03-R9: a stall with no earlier wait wakes the orchestrator at round 1.
12. NOT RUN. H03-R10: a finish wait with a limit above 2 h keeps its deadline after 1 check.
13. NOT RUN. H03-R11: a dead process during a wait gives `dead`.

Manual steps after the fix, in the scratch project:

1. NOT RUN. Do manual steps 1 to 3 of `## Reproduce`. The deadline is 2 h after `started_at`. MAIN: 24 h.
2. NOT RUN. In the scratch ledger only, set the `deadline` of the row to the current time.
3. NOT RUN. At the next check, the worker pane shows `[horch] NOTE: Your wait ended`. MAIN: the
   orchestrator gets the `shows no progress` line, and the worker gets nothing.
4. NOT RUN. Do not answer. The next check wakes the orchestrator, and the check after it reaps the
   worker. MAIN: the reap occurs at the same check, with no message to the worker.

Ledger counts to measure 2 weeks after the release. Add `AND ended_at >= '<release date>'` to query A.

1. The maximum gap of the `waited = 1` row is at most 150 min for a background or reviewers wait and
   750 min for a question. A watchdog outage (spec 04) can exceed this. Check each exception.
2. The mean reap gap falls. A replay of the caps on today's 49 reaps gives 72.2 min instead of 120.1
   min (`MEASURED`). The replay assumes that no waiter answers, and it keeps the F4 outage.
3. Count the `Wait cap:` events and the workers reaped after one. A high share means that the message
   or the caps need a change.

## Size and risks

Estimate: 1 worker-day.

- R1. A laptop sleep passes the deadlines of all waits, because MAIN uses the wall clock. Each waiter
  gets 1 message. A waiter that does not answer within 2 checks is reaped and uses 1 of 3 attempts.
- R2. A reap of a question waiter closes its pane. The replacement reads the answer from task history.
- R3. The message costs 1 agent turn for each 2 h of a long wait.
- R4. A `waiting:` line during a detached finish replaces the finish limit with 2 h, because both use
  kind `background` (`CODE TL/finish/detach.rs:7`). The worst result is 1 extra message.
- R5. A typed message can move herdr's state counter, which counts as a proxy checkin. A stuck agent
  then gets about 10 min more before the reap. `NOT VERIFIED`

Files that other specs of this run can also change:

- `TL/watchdog.rs`: spec 04 (`needs_restart`), spec 05 (process check), spec 06 (limit detection).
- `CMD/taskledger_watch.rs`: spec 04 and spec 05 (the 30 s reconcile tick).
- Spec 07 (Stop hooks) must allow an end of turn during each wait kind.

Open questions for the operator:

- Q1. Is 12 h the right question cap? I recommend 12 h, so that a question asked in the evening does
  not reach the reap steps before the morning.
- Q2. Should a reap after a wait cap use 1 of the 3 attempts? I recommend yes for now, because it
  keeps `tasks::requeue` unchanged and the waiter has 2 checks to answer.
