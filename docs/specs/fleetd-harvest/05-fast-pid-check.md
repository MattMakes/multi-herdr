# 05: Check worker processes on the 30 s tick

Status: proposed. Target: `main` at `06930d95`. Source of the idea: fleetd at `9bc3bf70`.
Report id: FM-4. Size: S.

Paths: `TL` is `crates/horch-core/src/taskledger`. `CMD` is `crates/horch/src/cmd`.
Labels: `CODE`, `MEASURED`, `REPORT`, `NOT VERIFIED`. Each `CODE` label without a commit is at MAIN.

## Problem

The problem is partly real on MAIN. The fix is small, and its measured value is small.

A fleet worker has 2 processes: the agent and its supervisor (`horch worker`). If only the agent
dies, the supervisor requeues the task at once. If both die, nothing reports the death. The task
stays `running` until the next watchdog check of that worker, up to 600 s later. The orchestrator
is not told during that time.

A plain PID check on the 30 s tick is not safe. In 4 of the 11 measured cases, the recorded PIDs
were gone, but the pane was open and its transcript still grew. A plain PID check would reap these
workers sooner. This spec adds a third condition: the pane host must confirm that the pane is gone.

Exact cause on MAIN:

1. `CODE TL/supervisor.rs:201`: `settle` requeues a `working` worker when the agent exits. This
   code runs in the supervisor, so it cannot run when the supervisor is dead.
2. `CODE TL/watchdog.rs:194`: `check_worker` returns early when `!force && elapsed <
   run.check_every_s`. This condition is the delay. It reads no process state.
3. `CODE TL/schema.rs:45`: `check_every_s` has the default 600.
4. `CODE CMD/taskledger_watch.rs:383`: the watchdog loop already runs a pass every
   `(check_every_s / 10).clamp(1, 30)` s, so 30 s at the default. No new loop is necessary.
5. `CODE TL/watchdog.rs:331`: when the check runs, a gone agent gets the verdict `dead` at once.

## Evidence on main

All queries are read-only and ran on 2026-11-22 (`MEASURED`). The ledger holds 4103 claims from
2026-10-31 to 2026-11-22. Each `#` line after a query is its output.

```sh
L=~/.horch/ledger.sqlite3
# A: the count of dead-agent checks. Report R6 gives the same count.
sqlite3 -readonly $L "SELECT count(*), min(at), max(at) FROM checks
  WHERE note='the agent process is gone';"
# 11|2026-10-31T19:35:16Z|2026-11-17T03:50:59Z
# B: the delay in seconds from the last supervisor beat to the reap, by pane result.
sqlite3 -readonly $L "SELECT p.result, count(*), min(d), max(d), sum(d) FROM
  (SELECT c.worker_id, CAST((julianday(c.at)-julianday(w.last_checkin_at))*86400 AS INTEGER) AS d
   FROM checks c JOIN workers w ON w.worker_id=c.worker_id
   WHERE c.note='the agent process is gone') x
  JOIN stops p ON p.worker_id=x.worker_id AND p.target='pane' GROUP BY p.result;"
# already_gone|7|24|584|1816
# pane_closed|4|1320|61882|65864
# C: the state of each process at the reap.
sqlite3 -readonly $L "SELECT s.target, s.result, count(*) FROM stops s
  WHERE s.target IN ('agent','supervisor') AND s.worker_id IN
  (SELECT worker_id FROM checks WHERE note='the agent process is gone') GROUP BY 1,2;"
# agent|already_gone|11
# supervisor|already_gone|11
# D: all requeues, and the requeues that a supervisor made.
sqlite3 -readonly $L "SELECT count(*), sum(text LIKE '%before horch ledger done%')
  FROM events WHERE kind='requeued';"
# 103|0
```

Query C shows that both processes were gone in 11 of 11 cases. Query D matches the text of
`TL/supervisor.rs:232`. An older build can have written other text (`NOT VERIFIED`).

The 2 groups of query B have different causes:

1. Group `already_gone`, 7 workers: the agent, the supervisor and the pane were gone. This is the
   fault of this spec. The total delay is 1816 s (30 min) in 23 days, with a mean of 259 s.
2. Group `pane_closed`, 4 workers: the pane was open. All 4 transcripts were written after the last
   beat, up to 1310 s after it (`MEASURED`). For 3 of the 4, herdr showed `working` at the reap.
   The cause is probably a herdr resume, which runs an agent without its supervisor (`CODE
   CMD/restore.rs:104`). That cause is `NOT VERIFIED`. The watchdog made no check at the due time
   in these 4 cases (`elapsed_s` is 1358 to 62259), so the 30 s tick cannot help them.

Real examples (`MEASURED`):

1. Group 1: worker `7f151179`, task T-6. Last beat 2026-11-17T03:41:14Z. Reap 03:50:59Z. Delay 584 s.
2. Group 2: worker `7de9f68c`, task T-7. Last beat 2026-11-07T07:33:27Z. Transcript written
   07:55:17Z. Reap 07:55:28Z. herdr showed `working`, and the reap closed the pane.

After the reap, the wait for a new worker was longer than the detection delay. Of the 7 tasks in
group 1, 2 were claimed again after 4732 s and 93943 s. The other 5 are still `queued` with 0 later
claims, in runs that the ledger shows as `active` (`MEASURED`).

## How fleetd solves it

1. `CODE 9bc3bf70:docs/specs/fleetd/design.md:1549`: FM-4 checks PID and start time each 5 s pass.
2. `CODE 9bc3bf70:crates/horch-core/src/fleetd/fleet_runtime/adapters/scans.rs:234`:
   `process_liveness` reads the OS facts for 1 recorded process.
3. `CODE 9bc3bf70:.../adapters/scans.rs:244`: `liveness` implements the rule.
4. `CODE 9bc3bf70:.../application/pass.rs:2863`: `seen` maps a process with no result to `Unknown`.

The rule: a process is dead only when its PID does not exist, or when its PID runs with another
start time. If the OS cannot give the start time, the state is unknown, and no action follows.

The idea is the 3-state result. The 5 s pass, the leases and the process scan exist because fleetd
is a daemon. MAIN does not need them.

MAIN has a defect that this rule prevents. `CODE crates/horch-core/src/procid.rs:21`: `is_same`
returns `false` when the start time cannot be read, so MAIN cannot tell "gone" from "unreadable".

## Fix

Requirements:

1. **H05-R1.** A new function gives the state of a recorded process: `Alive`, `Dead` or `Unknown`.
   `Dead` means that the PID does not exist, or that it runs with another start time. `Unknown`
   means that the PID exists and its start time cannot be read. A PID with no recorded start time
   that exists is `Alive`.
2. **H05-R2.** A new function gives the state of a pane: `Present`, `Missing` or `Unknown`.
   `Missing` requires an explicit "not found" answer from the pane host. Each other failure, such
   as a timeout or a stopped server, is `Unknown`.
3. **H05-R3.** A worker that is not due gets an early check only when the agent is `Dead`, the
   supervisor is `Dead` and the pane is `Missing`. Any other combination keeps the 600 s schedule.
   A worker with no recorded agent PID, supervisor PID or pane keeps the 600 s schedule.
4. **H05-R4.** Before an early check, the watchdog reads the worker row again and requires the
   status `working`. It also requires that the worker runs on this host. This prevents a false
   `dead` for a worker that finished during the pass.
5. **H05-R5.** An early check uses the existing path: verdict `dead`, note `the agent process is
   gone`, `reap_dead`, requeue, and the wake of the orchestrator.
6. **H05-R6.** The pane host is called only after both processes are `Dead`. A healthy worker
   causes 0 pane host calls between its due checks.
7. **H05-R7.** A worker whose last check has the verdict `dead` gets no early check. If its reap
   failed, the watchdog retries at the 600 s schedule and not on each tick.

Files of MAIN to change:

1. `crates/horch-core/src/procid.rs`: add `ProcessState`, `classify(exists, recorded, read)` and
   `state(pid, started)`. Keep `alive` and `is_same` unchanged, because other callers use them.
2. `crates/horch-core/src/workspace/herdr.rs`: add `pane_state`. Parse `HerdrFailed.stderr` as
   JSON. The codes `pane_not_found` and `not_found` give `Missing`. `herdr-docs/socket-api.md:717`
   names `not_found`, and `herdr.rs:521` shows `pane_not_found`.
3. `crates/horch-core/src/workspace/hc.rs`: add `pane_state`. Use `HcFailed::is_gone` (line 26).
4. `crates/horch-core/src/workspace/host.rs`: add `PaneState` and delegate `pane_state`.
5. `TL/watchdog.rs`: add `fn recorded_gone(&self, _w: &Worker) -> bool { false }` to `Observer`.
   Add `due_early`. At line 194, add `&& !due_early(store.conn(), observer, w, p.as_ref())?`.
6. `CMD/taskledger_watch.rs`: implement `recorded_gone` on `Live`, with a 2 s pane host timeout.
7. `crates/horch-e2e/src/bin/fake-herdr.rs:257`: make `pane get` print the JSON error
   `{"error":{"code":"pane_not_found","message":"pane <id> not found"},"id":"cli:pane:get"}`.

```rust
fn due_early(conn: &Connection, o: &dyn Observer, w: &Worker, p: Option<&Prev>) -> Result<bool> {
    if p.is_some_and(|p| p.verdict == Verdict::Dead.as_str()) {
        return Ok(false);
    }
    if w.host.as_deref().is_some_and(|h| h != util::host_name().as_str()) {
        return Ok(false);
    }
    if !o.recorded_gone(w) {
        return Ok(false);
    }
    Ok(workers::get(conn, &w.worker_id)?.status == "working")
}
```

New settings: none. The fix uses the existing tick of at most 30 s (`TL/watchdog.rs:695`).

Not ported:

1. The 5 s pass. The existing tick removes most of the delay, and a new loop adds load.
2. The daemon, the leases and the process scan. Rule 7 of the run forbids them.
3. A change to the 600 s path. It still reaps a worker whose pane is open (see Q1).

Effect at the upgrade: the fix has no schema change. `horch install` replaces a running watchdog
(`CODE CMD/install.rs:35`). The first pass of the new watchdog reaps each worker that already meets
H05-R3. The old watchdog reaps the same worker at its next check. A worker row from an older build
has the same columns, so it needs no migration.

## Reproduce

**RP1. `RUN`.** Run query A of `## Evidence on main`. It prints 11 cases.

**RP2. Automated failing test. `NOT RUN`.**

- Crate: `horch-e2e`. File: `crates/horch-e2e/tests/watchdog_liveness.rs`.
- Name: `h05_r5_a_worker_with_all_3_gone_is_reaped_before_its_600_s_check`.
- Copy from: `pinned_worker` (line 31) and `pinned_check` (line 98). Remove `--force`.
- Setup: call `pinned_worker`. Create no fake pane. Kill and wait for the fixture agent. Spawn
  `sleep 60`, read its start time, then kill and wait for it. Write its PID and start time to
  `supervisor_pid` and `supervisor_started`.
- Action: run `horch ledger watch --run <id> --once --json` with
  `HORCH_NOW=2030-01-01T00:00:30Z`. The worker claimed at 00:00:00, so it is not due.
- Assertion: the array has 1 item, with verdict `dead` and note `the agent process is gone`.
- MAIN today: the assertion fails, because the command prints `[]` (`CODE TL/watchdog.rs:194`).

**RP3 to RP7. Manual steps. Each is `NOT RUN`.** Warning: use a scratch ledger. Do not run these
steps with the real ledger.

```sh
# RP3 NOT RUN: make a scratch project and a fake herdr that answers "pane not found".
S=$(mktemp -d); git init -q -b main "$S/p"; git -C "$S/p" commit -q --allow-empty -m base
cd "$S/p"
export HORCH_LEDGER_DB="$S/l.sqlite3" HORCH_STATE_DIR="$S/state" HORCH_NO_WATCHDOG=1
export HORCH_NO_WAKE=1 HORCH_PROJECT_DIR="$S/p" HORCH_WORKSPACE_ID=w1 HORCH_HERDR_BIN="$S/herdr"
printf '#!/bin/sh\necho %s >&2\nexit 1\n' \
  "'{\"error\":{\"code\":\"pane_not_found\",\"message\":\"gone\"},\"id\":\"cli:pane:get\"}'" > "$S/herdr"
chmod +x "$S/herdr"
# RP4 NOT RUN: create a run and a task.
horch ledger run new h05
printf '# t\n' > t.md
horch ledger add --file t.md --primary dev
# RP5 NOT RUN: confirm that the PIDs do not exist. ps prints only its header.
ps -p 99990,99991
# RP6 NOT RUN: add a working worker with these 2 PIDs.
N=$(date -u +%Y-%m-%dT%H:%M:%SZ)
sqlite3 "$HORCH_LEDGER_DB" "INSERT INTO workers (worker_id, run_id, persona, role, host,
  workspace_id, pane_id, supervisor_pid, supervisor_started, agent_pid, agent_started,
  last_checkin_at, task_id, attempt, status, origin, spawned_at, started_at, claimed_at,
  last_progress_at, backend)
  SELECT 'h05-w', run_id, 'dev', 'dev-1', '$(hostname)', 'w1', 'w1:p1', 99990, 1, 99991, 1,
  '$N', task_id, 1, 'working', 'native', '$N', '$N', '$N', '$N', 'herdr' FROM tasks WHERE seq = 1;
  UPDATE tasks SET status='running', current_worker_id='h05-w', attempts=1 WHERE seq = 1;"
# RP7 NOT RUN: run 1 pass without --force.
horch ledger watch --run h05 --once
```

RP7 on MAIN prints `No worker was due for a check.` (`CODE CMD/taskledger_watch.rs:357`). The task
stays `running`. This output is the fault.

## Validate

The tests are `NOT RUN`, because the limits of this run forbid `cargo`.

1. **V1. H05-R5. `NOT RUN`.** The test of RP2 passes. MAIN today: it fails with `[]`.
2. **V2. H05-R1. `NOT RUN`.** `procid.rs`, test `h05_r1_classify_gives_3_states`:
   `classify(false, Some(7), || Some(7))` is `Dead`. `classify(true, Some(7), || Some(8))` is
   `Dead`. `classify(true, Some(7), || Some(7))` is `Alive`. `classify(true, None, || None)` is
   `Alive`. `classify(true, Some(7), || None)` is `Unknown`. MAIN today: it does not compile.
3. **V3. H05-R2. `NOT RUN`.** `herdr.rs` and `hc.rs`, test `h05_r2_only_not_found_is_missing`:
   `pane_not_found` and `not_found` are `Missing`. A connection error, a timeout and text that is
   not JSON are `Unknown`. MAIN today: it does not compile, and `pane_exists` is `false` for each
   failure (`CODE herdr.rs:204`).
4. **V4. H05-R3. `NOT RUN`.** `CMD/taskledger_watch.rs`, test `h05_r3_all_3_must_be_gone`: run the
   12 combinations of agent (dead, live), supervisor (dead, live) and pane (missing, present,
   herdr error). Only dead, dead, missing is `true`. Copy the fixture `herdr` (line 591). MAIN
   today: it does not compile.
5. **V5. H05-R4. `NOT RUN`.** `TL/watchdog.rs`, test `h05_r4_a_finished_or_remote_worker_is_not_due`:
   use an `Observer` whose `recorded_gone` is `true`. Set the row to `done` and keep the old
   `Worker` value. `due_early` is `false`. Then set `host` to another name. `due_early` is `false`.
   MAIN today: it does not compile.
6. **V6. H05-R6. `NOT RUN`.** `CMD/taskledger_watch.rs`, test `h05_r6_a_live_worker_makes_no_pane_call`:
   use a fake pane host that appends each call to a file, and the PID of the test process. Call
   `recorded_gone`. The file does not exist. Copy the test at line 709. MAIN: it does not compile.
7. **V7. H05-R7. `NOT RUN`.** `TL/watchdog.rs`, test `h05_r7_a_failed_reap_waits_for_the_due_check`:
   use an `Observer` whose `recorded_gone` is `true`. The first `check_run_at` gives `dead`. A call
   30 s later gives no check. A call 600 s later gives 1 check. MAIN today: it does not compile.
8. **V8. Guard for an open pane. `NOT RUN`.** `watchdog_liveness.rs`, test
   `h05_r3_a_gone_pid_in_an_open_pane_waits_for_the_due_check`: use the setup of RP2, and create the
   pane with the fake herdr. The command prints `[]`. MAIN today: this test also passes, so it
   proves nothing about the fault. It stops a fix that reaps the 4 workers of group 2 sooner.
9. **V9. Manual, fixed build. `NOT RUN`.** Do RP3 to RP6. Run RP7 with the new binary, with
   `HORCH_LEDGER_DB` still set to the scratch file. Expect
   `dev-1 T-1: dead (round 0): the agent process is gone`. Then `horch ledger show T-1` shows
   `queued`. MAIN today: `No worker was due for a check.`
10. **V10. Manual, real herdr. `NOT RUN`.** Run `herdr pane get w999:p9` and read its error code.
    H05-R2 must accept the code. If it does not, the early check never runs. MAIN today: nothing
    reads the code. The real output is `NOT VERIFIED`, because this run did not call herdr.
11. **V11. Manual, open pane. `NOT RUN`.** Repeat V9 with a fake herdr that prints
    `{"result":{"pane":{"pane_id":"w1:p1","workspace_id":"w1"}}}` and exits 0. Expect
    `No worker was due for a check.` MAIN today: the same output, so this step is a guard only.
12. **V12. Ledger baseline. `RUN`.** Query B on 2026-11-22 gives `already_gone|7|24|584|1816`.
13. **V13. Ledger after 2 weeks. `NOT RUN`.** Run query B with `AND c.at > '<release date>'` in the
    inner query. For group `already_gone`, the maximum delay must be at most 90 s: 30 s for the
    last beat, 30 s for the tick, and 30 s for a slow pass. MAIN today: the maximum is 584 s.

## Size and risks

Estimate: 0.75 worker-days. Size S.

Value: the fix removes at most 1816 s of blocked time in the 23 days measured. Rank this fix below
the fixes that remove hours of blocked time.

Risks:

1. **Risk 1. The real herdr uses another error code.** The early check never runs. V10 detects it.
2. **Risk 2. A zombie supervisor looks alive.** `kill(pid, 0)` succeeds for a zombie (`NOT VERIFIED`
   on each OS). The worker then keeps the 600 s schedule, so the result is the behavior of MAIN.
3. **Risk 3. A hung pane host delays a pass.** The call has a 2 s timeout.
4. **Risk 4. A herdr restart.** Panes can be absent for a short time and come back with a resumed
   agent. While the server is down, the pane state is `Unknown`, so no early check runs. If the
   server answers "not found" before it restores a pane, the worker is reaped early. This order is
   `NOT VERIFIED`.

Files that other specs of this run can also change:

1. `TL/watchdog.rs`: spec 03 changes `proof` at line 333. Spec 04 changes `needs_restart` at line
   692. This spec changes `check_worker` at line 194 and the `Observer` trait.
2. `CMD/taskledger_watch.rs`: spec 04 can change the restart path. This spec adds a method to `Live`.

Open questions for the operator:

1. **Q1.** MAIN reaps a worker whose recorded PIDs are gone, even when its pane still runs an agent.
   This closed 4 panes, and 3 of them showed `working`. Should the reap keep such a pane? I
   recommend a separate ticket with FM-13, which turns off the herdr resume for fleet panes.
2. **Q2.** 5 requeued tasks have no later claim and are in runs that the ledger shows as `active`.
   Should a separate ticket find out why? I recommend yes: this wait exceeds the delay fixed here.
