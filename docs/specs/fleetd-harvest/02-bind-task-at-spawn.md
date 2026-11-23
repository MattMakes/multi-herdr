# 02: Bind the task to the worker at spawn

Status: proposed. Target: `main` at `06930d95`. Source of the idea: fleetd at `9bc3bf70`.
Report id: FM-2. Size: S.

Labels: `CODE` is read at the pinned commit (MAIN unless named). `MEASURED` is a read-only read on
2026-11-22. `REPORT` is an unchecked report statement. `NOT VERIFIED` is an unexecuted inference.

## Problem

**MAIN already removes most of this problem.** Commit `306bfe72` (2026-11-06) added a spawn gate.
Since that commit, 0 of 515 workers ended `no_task`. Before it, 85 of 3757 workers ended `no_task`
[MEASURED]. This spec covers the 1 path that is left. That path has 0 recorded occurrences.

What goes wrong for the operator: the orchestrator spawns 2 workers of 2 personas, because
`horch ledger plan-workers` asks for both. The 2 tasks cannot run together, because they own the
same file. Both workers pass the spawn gate and start. The worker that claims second in the board
order finds no task and exits. The tokens of its start are lost, and its task waits.

The exact cause: `Board::plan` lets a queued task hold a slot only when a live worker of its persona
exists (`is_staffed`) [CODE `crates/horch-core/src/taskledger/tasks.rs:686`, `:768`]. The worker row holds no task until `claim`, because
only code-sweep reviewers set `target_task_id` [CODE `crates/horch/src/cmd/taskledger_spawn.rs:206`,
`:311`]. The fault needs task A (persona `p`, first in queue order) and task B (persona `q`) with 1
same `Own` path, and no live worker of `p` or `q`:

1. `horch spawn q` inserts its row. A holds no slot, so B is startable. The gate passes
   [CODE `crates/horch/src/cmd/spawn.rs:133`, `crates/horch-core/src/taskledger/tasks.rs:1495`].
2. `horch spawn p` inserts its row. A now holds a slot, so A is startable. The gate passes.
3. The `q` worker claims. `Board::plan` gives A the slot and marks B `FileHeld`. `pick_task` finds
   no task for `q`, and `no_task` ends the worker [CODE `crates/horch-core/src/taskledger/tasks.rs:886`, `:937`].

`plan-workers` sorts personas by name, so `q` spawns first whenever its name sorts first
[CODE `crates/horch-core/src/taskledger/tasks.rs:1455`].

A smaller fault has the same cause. `prepare` commits the row, and the gate reads it in a later
transaction. 2 concurrent spawns for 1 task can each count the other and both be refused
[CODE `crates/horch-core/src/taskledger/tasks.rs:1507`]. The task waits for the 120 s re-wake, and no tokens are lost.

## Evidence on main

The ledger is `~/.horch/ledger.sqlite3`. Measured 2026-11-22 23:15 UTC with `sqlite3 -readonly`.

```sql
SELECT CASE WHEN spawned_at < '2026-11-06T19:46:00Z' THEN 'before' ELSE 'after' END era,
       count(*) workers, sum(status='no_task') no_task,
       sum(end_reason LIKE 'horch spawn refused: persona%') gate_refusals
FROM workers WHERE kind='worker' GROUP BY 1;
```

| Period | Workers | `no_task` | Gate refusals |
|---|---|---|---|
| Before `306bfe72` | 3757 | 85 (2.26%) | 0 |
| After `306bfe72` | 515 | 0 | 7 |

[MEASURED] R6 gives 85 of 4239 [REPORT]. The total is now 4272 because live fleets add workers.

**Cause of the group of 71 ("no task for persona codex-reviewer").** The ticket asks if horch
spawned these reviewers itself. It did not:

1. `spawn_reviewers` first exists in `306bfe72`, after all 71 rows [MEASURED `git log -S`].
2. 0 of the 85 rows have a `target_task_id`. At the `no_task` event of 70 of the 71 rows, no
   unclaimed task of that persona existed [MEASURED].
3. The orchestrator ran shell loops with a fixed count, and no gate existed. Example from the
   `mining-overhaul` transcript at 2026-11-01T02:46:34Z [MEASURED]:
   `i=0; while [ $i -lt 9 ]; do ...; horch spawn codex-reviewer ...; done`.

The count 9 came from an older `plan-workers` line. The gate of `306bfe72` now refuses each such
spawn before a pane opens, so this group needs no more work.

Real examples [MEASURED]:

1. `codex-reviewer-746` (`b3ad2c14-62cf-44ad-a39a-22f3933e8744`), run `mining-overhaul`: spawned
   2026-11-01T02:46:21Z, `no_task` at 02:46:33Z.
2. `opus-hardening-17` (`f7805c62-2629-4159-8c65-9ec8b8835578`), run `trace-fixes`: the last
   `no_task` row, spawned 2026-11-06T10:08:32Z.

**How often the remaining path can occur.** No count of the path itself exists. The precondition
is common: the ledger holds 418 pairs of work tasks of 2 personas in 1 run with 1 same `Own` path,
and 302 of the pairs have no dependency path between them [MEASURED]. This is an upper bound,
because the 2 tasks of a pair must also be startable at the same time.

## How fleetd solves it

The rule: **1 transaction moves the task out of the ready set and writes the worker row with the
task id, before any process starts.** Paths are under
`9bc3bf70:crates/horch-core/src/fleetd/task_execution/application/` [CODE].

1. `bind` (`dispatch.rs:177`) checks that the task is `ready`, moves it to `leased`, and inserts
   the worker with `target_task_id`, `task_id` and the attempt.
2. `dispatch_due` (`dispatch.rs:697`) runs each bind in a savepoint of 1 transaction.
3. `choices` (`dispatch.rs:636`) reads the ready and running tasks in 1 snapshot.
4. `crash` (`launch.rs:250`) ends a failed launch and schedules a retry.

The rule is the idea to port. These parts exist only because fleetd is a daemon, and this spec does
not port them: the `leased` state, the `attempts` table, the fleet clock, the push launch, the
worktree reservation at bind, the dispatch budget and the retry rungs.

## Fix

The fix keeps pull dispatch. Decision D1 of `docs/specs/trace-fixes.md` stays true: the worker gets
no task text, it still runs `horch ledger claim`, and the ledger still decides. The fix only makes
the existing reservation (`workers.target_task_id`) apply to every spawn without task text.

### Requirements

1. **H02-R1.** `horch spawn` without task text inserts the worker row and reserves 1 startable task
   of its persona in 1 `IMMEDIATE` transaction. Of N concurrent spawns for M startable tasks,
   exactly `min(N, M)` get a reservation.
2. **H02-R2.** When no task can be reserved, the same transaction ends the row as `launch_failed`
   with the reason `horch spawn refused: <why>`. No pane opens and no agent starts.
3. **H02-R3.** A queued task with a reservation of a live, unclaimed worker counts as active on the
   board. A task that conflicts with it (`Own` overlap, exclusive, shared checkout) is not
   startable. The reserved task stays startable for its holder.
4. **H02-R4.** `claim` gives a worker its reserved task. If that task is no longer startable, for
   example because it was cancelled, the existing fallback stays: the worker takes the next task of
   its persona, or ends `no_task`.
5. **H02-R5.** A reservation ends when its worker ends. No new timer exists. The task is then
   startable again, and the existing re-wake tells the orchestrator.
6. **H02-R6.** A spawn with task text (a role of a controller) reserves nothing. A spawn with
   `HORCH_CLAIM_TARGET` keeps its target when that task is startable.
7. **H02-R7.** The first prompt `CLAIM_TASK` does not change.

### When the reserved worker dies before its claim

1. The agent exits and the supervisor lives: the supervisor ends the row at once
   [CODE `crates/horch-core/src/taskledger/supervisor.rs:230`].
2. The supervisor is gone, or the row has no check-in for 120 s: the next watchdog pass ends the
   row. A pass runs each 30 s by default [CODE `crates/horch/src/cmd/taskledger_watch.rs:383`,
   `crates/horch-core/src/taskledger/workers.rs:436`, `:468`].
3. The agent is alive but waits at a prompt: the row keeps the reservation. The watchdog flags it
   after 180 s, and the orchestrator decides [CODE `crates/horch-core/src/taskledger/watchdog.rs:782`].

`reservations` ignores a row that is not live [CODE `crates/horch-core/src/taskledger/tasks.rs:924`].
After the row ends, `startable_rewake` wakes the orchestrator within 120 s plus 1 pass
[CODE `crates/horch-core/src/taskledger/watchdog.rs:707`].

### When no startable task exists at spawn time

`horch spawn` exits non-zero with the text of today: `persona <p> has 0 startable task(s), ...`.

### Files to change

1. `crates/horch-core/src/taskledger/tasks.rs`
   - Add the field `reserved: BTreeSet<String>` to `Board`. Fill it in `Board::load` from
     `reservations(conn, run_id, None)`.
   - In `Board::plan`, start `active` with the running and landing tasks and the queued, not held
     tasks in `reserved`. For each task, call `wait` with `active` minus the task itself. Do not
     push a reserved task a second time.
   - Add `pub enum SpawnBind { Bound(String), Refused(String) }`.
   - Add `pub fn bind_spawn(conn, run_id, worker_id) -> Result<SpawnBind>`. It calls `spawn_refusal`
     and returns `Refused(why)` on `Some`. It then calls `pick_task`. On `None` it returns
     `Refused`. On a task it writes `workers.target_task_id` and returns `Bound(task_id)`.
2. `crates/horch/src/cmd/taskledger_spawn.rs`: `insert_row` returns `Result<(String, Option<String>)>`: the worker id and the refusal. When `args.task.trim().is_empty()`, it calls `tasks::bind_spawn` in the `store.immediate` closure of `workers::insert`. On `Refused(why)`, it calls `workers::launch_failed` there with `horch spawn refused: {why}` and returns `Some(why)`. `prepare` destructures the pair and sets the new field `pub refused: Option<String>` of `LedgerSpawn`.
3. `crates/horch/src/cmd/spawn.rs`: in `refuse_before_launch` (spec 08), the first test bails with `l.refused`, in place of `refuse_over_plan`. A disk check runs only after it. The later `l.launch_failed` is a no-op [CODE `crates/horch-core/src/taskledger/workers.rs:252`].
4. `crates/horch-e2e/tests/taskledger.rs` and the `tests` module of `tasks.rs`: add the tests.

### Settings

This fix adds no setting. Each time limit that it uses already exists.

### Not ported

1. The `leased` state. A reserved task stays `queued`, so status and cancel code stay correct.
2. A claim that takes only the reserved task. `trf_24_auto_spawned_reviewer_claims_its_review_not_an_older_task`
   pins the fallback, and the fallback saves a start when the reserved task was cancelled.
3. A change to the `plan-workers` count for 2 conflicting tasks.
   `trf_31_a_task_with_no_worker_of_its_persona_holds_no_slot` pins it. The second spawn is refused
   before it costs tokens.

### Effect at upgrade

1. The ledger needs no migration [CODE `crates/horch-core/src/taskledger/schema.rs:444`].
2. A live worker without a reservation claims normally, and `spawn_refusal` counts it as on its
   way. Only spawns after the upgrade get a reservation.
3. After a spawn, a reserved task leaves the `plan-workers` count. The line
   `opus: spawn 0 (1 can start, 1 on the way).` becomes `No task can start now. Spawn no worker.`
   when that task was the only one.

## Reproduce

No step below was executed, because this ticket forbids `cargo` and `horch spawn`.

### Automated (NOT RUN)

Crate `horch-e2e`, file `crates/horch-e2e/tests/taskledger.rs`. Copy the setup of
`a_spawned_worker_records_its_pids_session_and_checkins` (`:398`). The scenario `exec,stay` keeps
the fake agent alive and it does not claim, so the test controls the order.

```rust
#[test]
fn h02_a_spawned_worker_keeps_its_task_when_another_persona_spawns_next() {
    let h = fleet("h02-bind", "exec,stay");
    ok(&h, &["ledger", "run", "new", "a"]);
    queue(&h, "01-a", "codex-terra", &["--own", "src/shared.rs"]);
    queue(&h, "02-b", "codex-sol", &["--own", "src/shared.rs"]);
    ok(&h, &["spawn", "codex-sol", "--from-pane", "w1:p1", "--no-tile", "--force"]);
    let second = h.run(&["spawn", "codex-terra", "--from-pane", "w1:p1", "--no-tile", "--force"]);
    let st = json_of(&h, &["ledger", "status", "--json"]);
    let rows = st["workers"].as_array().unwrap();
    let row = rows.iter().find(|w| w["worker"]["persona"] == "codex-sol").unwrap();
    let sol = row["worker"]["worker_id"].as_str().unwrap();
    let claim: Value = serde_json::from_str(&ok_as(&h, sol, &["ledger", "claim", "--json"])).unwrap();
    assert_eq!(claim["outcome"], "claimed", "{claim}");
    assert!(!second.status.success(), "{}", text(&second));
}
```

Expected on MAIN [NOT VERIFIED]: the test fails at the first assertion with `left: "no_task"`,
`right: "claimed"`.

### Manual (NOT RUN)

These steps start no agent. Use a scratch repository and a scratch `HORCH_LEDGER_DB`, because a
development build can migrate the shared ledger.

1. NOT RUN. Build MAIN, then set `H=<checkout>/target/debug/horch`.
2. NOT RUN. In a scratch git repository, run `export HORCH_LEDGER_DB=/tmp/h02/ledger.sqlite3`.
3. NOT RUN. Run `$H ledger run new h02`.
4. NOT RUN. Write `tickets/01-a.md` and `tickets/02-b.md`, each with 1 heading.
5. NOT RUN. Run `$H ledger add --file tickets/01-a.md --primary opus --own src/shared.rs`.
6. NOT RUN. Run `$H ledger add --file tickets/02-b.md --primary codex-sol --own src/shared.rs`.
7. NOT RUN. Run `$H ledger plan-workers`. MAIN prints:
   `codex-sol: spawn 1 (1 can start, 0 on the way).` and `opus: spawn 1 (1 can start, 0 on the way).`
8. NOT RUN. Run `A=$($H ledger worker new --persona codex-sol)`, then
   `B=$($H ledger worker new --persona opus)`. No agent runs, so neither row claims early.
9. NOT RUN. Run `HORCH_WORKER_ID=$A $H ledger claim`. MAIN ends `A` as `no_task` with the reason
   `no task for persona codex-sol`, because task 01 holds the slot [NOT VERIFIED].

## Validate

No test below was executed. Each item says what MAIN gives today.

### Tests (all NOT RUN)

Add the core tests to the `tests` module of `tasks.rs`, with its helpers `queue`, `hire`, `spawn_for`.

1. NOT RUN. **H02-R1:** the test of `## Reproduce` passes. MAIN fails it.
2. NOT RUN. **H02-R1:** `h02_r1_racing_spawns_reserve_exactly_the_startable_tasks`. Copy
   `eight_racing_claims_take_exactly_three_tasks` (`crates/horch-core/tests/taskledger_queue.rs:284`).
   8 threads insert and bind, with 3 tasks. Assert 3 `Bound` and 5 `Refused`. MAIN has no
   `bind_spawn`. To show the fault on MAIN, insert 2 rows for 1 task: `spawn_refusal` refuses both.
3. NOT RUN. **H02-R2:** `h02_r2_a_spawn_with_no_task_opens_no_pane`. Assert a non-zero exit, 0 fake
   agent calls, `launch_failed`, and `LedgerSpawn.refused` is `Some` here and `None` on a bound spawn. MAIN has no field.
4. NOT RUN. **H02-R3:** `h02_r3_a_reservation_blocks_a_conflicting_task`. Queue A and B with 1 same
   `Own` path, and bind a `q` worker to B. Assert `startable` is `[B]`. MAIN gives `[A, B]`.
5. NOT RUN. **H02-R3:** `h02_r3_an_exclusive_reserved_task_stays_startable_for_its_holder`. Bind to
   1 exclusive task and claim it. MAIN passes, so this test only guards the new self-exclusion.
6. NOT RUN. **H02-R4:** `h02_r4_a_cancelled_reservation_falls_back`. Bind to A, cancel A, and claim.
   Assert the next task of the persona. MAIN passes, so this test only guards behavior.
7. NOT RUN. **H02-R5:** `h02_r5_a_dead_worker_frees_its_reservation`. Bind to B, end the row, and
   assert A is startable. MAIN has no reservation, so A is startable before and after.
8. NOT RUN. **H02-R6:** `h02_r6_a_brief_spawn_reserves_nothing`. Assert a null `target_task_id`.
   MAIN gives null, so this test only guards behavior.
9. NOT RUN. **H02-R7:** `crates/horch-core/tests/golden_prompts.rs` passes unchanged. MAIN passes.

### Manual (NOT RUN)

Repeat steps 1 to 7 of the manual reproduction with the fixed build. Warning: step 2 starts 1 agent.

1. NOT RUN. Step 7 prints the same 2 lines.
2. NOT RUN. Run `$H spawn codex-sol --run h02; $H spawn opus --run h02`. The second spawn exits
   non-zero and prints `horch spawn refused: persona opus has 0 startable task(s), ...`. MAIN starts both.
3. NOT RUN. `$H ledger status` shows `codex-sol-1` as `working` on T-2. MAIN shows `no_task`.
4. NOT RUN. After T-2 finishes, `$H ledger plan-workers` prints `opus: spawn 1`. MAIN does too.

### Ledger count after 2 weeks of use (NOT RUN)

NOT RUN. Replace `<landed>` with the UTC time when the fix landed:

```sql
SELECT count(*) workers, sum(end_reason LIKE 'no task for persona%') found_no_task
FROM workers WHERE kind='worker' AND spawned_at >= '<landed>';
```

The target is 0. MAIN gives 0 of 515 today, so only the automated test proves the fix.

## Size and risks

Size: S, 0.5 to 1 worker-day. The change is about 60 lines in 3 files, plus tests.

1. **R1: priority inversion.** If the `q` worker spawns first, B runs before the higher-priority A.
   A runs after B lands. No work is lost.
2. **R2: a stuck worker holds its reservation.** A live agent at a login prompt also blocks tasks
   that conflict with its reserved task, until the orchestrator acts on the 180 s flag.
3. **R3: a test can pin the old `plan-workers` line after a real spawn.** A search of the test
   folders found none [MEASURED]. Inline `src` tests were not searched. A full test run must confirm.
4. **R4: the value is small.** The remaining path has 0 recorded occurrences in 515 spawns.

Spec 08 also changes `crates/horch/src/cmd/spawn.rs`, and specs 06 and 08 change `tasks.rs`.

Open questions for the operator:

1. **Q1.** Should this fix land, given 0 occurrences since 2026-11-06? I recommend that it lands
   after the fixes with measured failures, because it is small and the code reaches this path.
2. **Q2.** Is the priority inversion of R1 acceptable? I recommend yes. Holding back the
   lower-priority persona can stall a run when the higher-priority persona is out of quota.
