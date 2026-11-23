# 08: Refuse a spawn below a free-disk floor

Status: proposed. Target: `main` at `06930d95`. Source of the idea: fleetd at `9bc3bf70`.
Report id: FM-28. Size: S.

Labels: `CODE` is read at the pinned commit. `MEASURED` is a read-only command run on 2026-11-22.
`REPORT` is a report claim that this spec did not check. `NOT VERIFIED` has no evidence.
1 GB is 2^30 bytes, as in `human_bytes` (`CODE crates/horch-core/src/taskledger/triage/mod.rs:427`).

## Problem

The problem is real on MAIN. Each task gets a worktree, and each worktree gets its own build
folder. Many tasks fill the disk. A build then stops with a disk error, and the error looks like a
code fault. The operator must find the cause and remove build folders by hand. MAIN starts new
workers during this time, so the disk fills again.

The exact cause has 3 parts:

1. MAIN measures free disk in `MachineSnapshot.disk_free_bytes`
   (`CODE crates/horch-core/src/runtime/machine.rs:53`), but no production code reads the field
   outside `machine.rs` (`MEASURED`, step M1).
2. `spawn` in `crates/horch/src/cmd/spawn.rs:109` has 1 refusal for the worker plan
   (`refuse_over_plan`, `CODE spawn.rs:155`) and none for disk. `fleet_warnings` probes the
   machine and reads only load (`CODE spawn.rs:844-851`).
3. `worktree_ops::create` (`CODE crates/horch-core/src/taskledger/worktree_ops.rs:25`) and
   `finish::create_worktree` (`CODE crates/horch-core/src/taskledger/finish/mod.rs:380`) run
   `git worktree add` with no disk condition, so MAIN continues when free disk is low.

## Evidence on main

No count of disk faults exists in the ledger of MAIN. The evidence is in the fleetd run records.

Query, on `~/.horch/ledger.sqlite3`, run on 2026-11-22T23:17Z (`MEASURED`):

```sh
sqlite3 -readonly ~/.horch/ledger.sqlite3 "select count(*) from events where
  lower(text) like '%no space left%' or lower(text) like '%errno 28%' or
  lower(text) like '%enospc%' or lower(text) like '%disk full%' or lower(text) like '%storagefull%';"
```

1. The query returns 2 of 33,620 events. Both are false matches: review summaries of
   `tools/pixelize.py` in run `mech-miner-v2`, at 2026-10-31T07:14:41Z and 07:17:07Z (`MEASURED`).
2. The real count is 0, and 0 of 59 `launch_failed` workers name the disk (`MEASURED`). This does
   not show that the fault is rare. The ledger of `fleetd-build` is lost (`REPORT` R6).

Real examples from `ai_docs/plans/fleetd-build/decisions.md` at FINISH (`CODE`):

1. Line 305, 02:22Z, undated, after the 2026-11-18 entries: 0 GB free. The finish of T-880 failed
   with errno 28. The entry names no worker. Removal of the T-588 `target/` gave 11 GB free.
2. Line 164, 2026-11-18T02:43:47Z: disk 12 GB. The orchestrator removed 6 GB of build output and
   held the spawns of T-290 and T-295 until the disk was above 20 GB.

The same file has 6 entries of build output removed by hand (`MEASURED`, lines 157, 164, 287, 290,
305, 309). Line 154 records 20 GB free with spawns held by hand on 2026-11-18 (`CODE`). A cleanup
took `.worktrees` from 158 GB to 168 KB at 2026-11-19T17:10Z
(`CODE docs/tickets/fleetd-finish/_decisions-snapshot-1004Z.md:38`). The claim that workers lose
attempts to this fault has no count (`NOT VERIFIED`).

Size of 1 worktree with its build folder, from `worktrees.bytes_on_disk` (`MEASURED`):

```text
scope          worktrees  median  p90    p95    p99    max
this repo      169        2.84    9.22   10.24  12.49  14.16 GB
all projects   474        0.04    6.84   8.40   10.97  14.16 GB
```

MAIN writes this column at cleanup with `dir_bytes` (`CODE finish/land.rs:923`). `dir_bytes` adds
file lengths and counts a hard link each time (`CODE taskledger/git.rs:182`), so these sizes can be
too high.

## How fleetd solves it

fleetd has 3 parts at FINISH:

1. Free-disk floor: `free_disk` in `crates/horch-core/src/fleetd/fleet_runtime/domain/health.rs:201`.
   When free disk is known and below `health_min_free_disk_gb`, new dispatch holds. The default is
   10 GB (`CODE fleet_runtime/domain/config.rs:303`). An unknown reading does not hold
   (`CODE health.rs:204`).
2. Disk budget: `disk_budget` in `fleetd/worktree_custody/application/disk.rs:89`. Above a total of
   20 GB, it queues the removal of finished trees (`CODE`).
3. Dispatch pause: `holds` in `fleet_runtime/application/holds.rs` gives the holds to each pass.

The idea to port is part 1: do not start new work below a floor. Parts 2 and 3 exist because
fleetd is a daemon with a pass loop, a job queue and a needs-you list. MAIN has none of these.

## Fix

### Requirements

1. `H08-R1`: The floor is `HORCH_MIN_FREE_DISK_GB`, a whole number of GB. The default is 20. The
   value `0` turns the floor off. A value that does not parse gives the default.
2. `H08-R2`: `horch spawn` refuses when free disk is below the floor. It exits 1 and opens no pane.
   A ledger spawn ends its worker row as `launch_failed`, with the message in `end_reason`.
3. `H08-R3`: `horch ledger worktree new` and `horch ledger worktree scratch` refuse when they must
   create a worktree and free disk is below the floor. They exit 1. They create no folder, branch
   or ledger row.
4. `H08-R4`: The refusal message names the free space, the floor, the setting and the triage command. It scans no folder. Format:
   `free disk 7.5 GB is below the floor of 20 GB (HORCH_MIN_FREE_DISK_GB). The operator can free space with: horch worktree triage`
5. `H08-R5`: An unknown free-disk reading does not refuse. The command prints 1 stderr line:
   `horch: NOTE: free disk is unknown; the disk floor is not checked`.
6. `H08-R6`: The floor does not stop work that frees disk or continues held work. `horch spawn`
   does not check the floor for `--resume`, for `--replaces`, for the persona `worktree-steward`,
   or for a ledger spawn whose `target_task_id` names a queued `review` task.
7. `H08-R7`: `horch ledger worktree new` does not check the floor when the held task already has a
   worktree, or when the worker persona is `worktree-steward`.
8. `H08-R8`: When the operator sets `HORCH_MIN_FREE_DISK_GB`, `horch spawn` passes the value to
   the worker, so the worker's `horch ledger worktree new` uses the same floor.
9. `H08-R9`: `horch spawn` applies its refusals in 1 fixed order. The plan refusal comes first, and the floor applies only to a spawn that can proceed. A spawn with no task and low disk keeps the no-task reason.

The reason for `H08-R6`: finish needs reviewers and the comment-remover before land and cleanup
free the worktree. If the floor refused them, a full disk could not drain. Review tasks create no
worktree (`MEASURED`: 3,688 of 3,688 have `worktree_mode = 'shared'`). Horch spawns these reviewers with an exact target (`CODE crates/horch/src/cmd/taskledger_spawn.rs:311`). A persona match is not enough, because claim takes the first startable task of the persona (`CODE TL tasks.rs:886-907`). `H08-R3` still stops a worker whose target ended and that claims work.

### Files of MAIN to change

1. New `crates/horch-core/src/runtime/disk_floor.rs`. It holds
   `DEFAULT_MIN_FREE_DISK_GB = 20`, a pure function from `Known<u64>` and the floor to clear,
   unknown or below, and `check(ctx)`. `check` calls `machine::probe` with
   `ctx.settings.machine_file` and builds the message. Format sizes with `triage::human_bytes`.
2. `crates/horch-core/src/runtime/mod.rs`: add `pub mod disk_floor;`.
3. `crates/horch-core/src/runtime/context.rs`: add `min_free_disk_gb: Option<u64>` to `Settings`
   (line 128). Parse it next to `machine_file` (line 324).
4. `crates/horch-core/src/messaging/brief.rs`: add `min_free_disk_gb: Option<u64>` with
   `#[serde(default, skip_serializing_if = "Option::is_none")]`, as `ledger_db` has (line 51).
   Add it to `transport_env` (line 96) when it is set. Add it to the literal at line 199.
5. `crates/horch-core/src/execution/service.rs`: set the brief field next to `ledger_db` (line 209).
   Add `None` to the literals in `execution/lifecycle.rs` (lines 1278, 1373, 1769).
6. `crates/horch-core/src/taskledger/tasks.rs`: add `is_review_target(conn, worker_id)`. It reads
   the worker's `target_task_id` and is true when that task has `status = 'queued'` and `kind = 'review'`.
7. `crates/horch/src/cmd/spawn.rs`: add `refuse_before_launch(ctx, &args, ledger.as_ref(), brief)` in `spawn` after `prepare` (line 131), in place of the 2 branches, and before `spawn_worker`. It runs the plan refusal for a ledger spawn with no task text (`refuse_over_plan`, or `l.refused` after spec 02), then `refuse_below_floor`. A refusal after `prepare` lets the existing `l.launch_failed` record it. Use `bail!("horch spawn refused: {why}")`.
8. `crates/horch/src/cmd/taskledger.rs`: in `worktree_cmd` (line 1051), check the floor before
   `worktree_ops::create` for `New` and `Scratch`. Use the `project_root` of the worker's run.
   Skip the check for `New` when `finish::worktree_of` finds a worktree for the held task.
9. `crates/horch/src/cmd/recipes.rs`: make `WORKTREE_STEWARD` (line 417) `pub(super)`.
10. `crates/horch-e2e/src/harness.rs`: add `HORCH_MIN_FREE_DISK_GB=0` to the default env (line 282).
    Without this line, each e2e spawn test depends on the free disk of the test machine.
11. `README.md`: add 1 row for `HORCH_MIN_FREE_DISK_GB` to the environment table (line 1363).

### New setting

`HORCH_MIN_FREE_DISK_GB`, default `20`. The reasons:

1. The largest measured worktree is 14.16 GB. A floor of 20 GB leaves 5.84 GB after a build of
   that size, for git, the ledger and logs (`MEASURED`).
2. The fleetd default of 10 GB is below the p95 of this repo, 10.24 GB. A build of that size can
   fill the disk after it passes a 10 GB floor (`MEASURED`).
3. The operator held spawns at 20 GB by hand (`CODE decisions.md:135`, `decisions.md:154`).

### Not ported

1. The disk budget and automatic removal. Removal can lose work, and `horch worktree triage`
   already gives the operator this decision.
2. The dispatch pause, the needs-you item, `fleet.toml` and the other health floors. The first 3
   are parts of the daemon, and the floors for load, memory and files are not part of FM-28.

### Effect at upgrade

The ledger has no schema change, so existing runs and tasks continue. A brief from an older version
still parses (`serde(default)`). A live worker keeps its held worktree (`H08-R7`). A live worker
with no worktree is refused at `horch ledger worktree new` only below the floor. Its task stays
held, and the worker reports `BLOCKED`.

## Reproduce

### Automated

`NOT RUN`, because the run rules forbid `cargo`.

- Crate: `horch-e2e`. New file: `crates/horch-e2e/tests/h08_free_disk_floor.rs`.
- Copy `ledger_world`, `as_worker` and `worker_ok` from `tests/fleet18_brief_worktree.rs:85-119`.
- Copy `err`, `fleet` and `herdr` from `tests/fleet18_observability.rs:8` and `:272-295`.
- Setup: the seam is `HORCH_MACHINE_FILE` (`CODE runtime/context.rs:324`). `probe` reads this JSON
  file in place of the disk (`CODE machine.rs:80`). No full disk is needed.

```rust
fn low_disk(h: &mut Harness) {
    let file = h.root.join("machine.json");
    let snapshot = serde_json::json!({"os":"linux","arch":"x86_64","cpus":2,
        "mem_total_bytes":null,"mem_available_bytes":null,"disk_free_bytes":1073741824u64,
        "disk_total_bytes":null,"gpu":"unknown","max_open_files":null,"max_processes":null});
    std::fs::write(&file, snapshot.to_string()).unwrap();
    h.set("HORCH_MACHINE_FILE", file.to_string_lossy());
    h.set("HORCH_MIN_FREE_DISK_GB", "20");
}

#[test]
fn h08_r2_spawn_is_refused_below_the_floor() {
    let mut h = fleet("h08-spawn", "");
    low_disk(&mut h);
    let o = h.run(&["spawn", "sonnet", "--from-pane", "w1:p1", "--no-tile", "--force"]);
    assert!(!o.status.success(), "{}", err(&o));
    assert!(err(&o).contains("below the floor of 20 GB"), "{}", err(&o));
}
```

Write `h08_r3_worktree_new_is_refused_below_the_floor` in the same way. Use `ledger_world` and
`low_disk`, then run `as_worker(&h, &w, &["ledger", "worktree", "new", "floor"])`. Assert that the
command fails and that `h.project.join(".worktrees/T1-floor")` does not exist.

Result on MAIN (`NOT RUN`, from `CODE`): both tests fail at the first `assert!`, because both
commands succeed. `g50_legacy_orchestrator_and_host_load_are_advisory` runs the same spawn command
with a machine file and expects success (`CODE fleet18_observability.rs:689-699`).

### Manual

Run steps M3 to M6 in a plain terminal, not in a fleet pane.

1. M1, `RUN`. `git grep -n 'disk_free_bytes' 06930d95 -- 'crates/*/src/*'
   ':!crates/horch-core/src/runtime/machine.rs'` prints nothing and exits 1.
2. M2, `RUN`. With `horch` 0.5.3, `horch spawn --help | grep -ci disk` prints `0`, and
   `horch ledger worktree new --help | grep -ci disk` prints `0`.
3. M3, `NOT RUN`. Make a scratch project and ledger, then queue and claim 1 task:
   ```sh
   mkdir -p /tmp/h08/p && cd /tmp/h08/p && git init && git commit --allow-empty -m base
   export HORCH_STATE_DIR=/tmp/h08/state HORCH_LEDGER_DB=/tmp/h08/ledger.sqlite3
   export HORCH_NO_WATCHDOG=1 HORCH_NO_WAKE=1
   horch ledger run new h08
   printf '# Floor\n\n**Check:** none\n' > ticket.md
   horch ledger add --file ticket.md --primary sonnet
   export HORCH_WORKER_ID=$(horch ledger worker new --persona sonnet)
   horch ledger claim
   ```
4. M4, `NOT RUN`. Write the JSON of `low_disk` to `/tmp/h08/machine.json`.
5. M5, `NOT RUN`. Set the seam: `export HORCH_MACHINE_FILE=/tmp/h08/machine.json`.
6. M6, `NOT RUN`. Run `horch ledger worktree new floor`. Expect on MAIN: exit 0 and the lines
   `path:`, `branch:`, `base:`. The fault is that 1 GB of free disk did not stop the command.

## Validate

Each test goes in `crates/horch-e2e/tests/h08_free_disk_floor.rs`, unless the step names another
file. All steps are `NOT RUN`, except V13.

1. V1, `H08-R1`, `NOT RUN`. Unit test `floor_reads_default_zero_and_bad_value` in `disk_floor.rs`:
   unset gives 20, `0` gives off, `abc` gives 20. MAIN: the setting does not exist.
2. V2, `H08-R2`, `NOT RUN`. `h08_r2_spawn_is_refused_below_the_floor` passes. Also assert that the
   fake host logs no `pane split`, and a free reading of exactly 20 GB spawns. MAIN: fails.
3. V3, `H08-R2`, `NOT RUN`. `h08_r2_ledger_spawn_records_launch_failed`: the worker row has status
   `launch_failed` and `end_reason` contains `below the floor`. MAIN: the spawn succeeds.
4. V4, `H08-R3`, `NOT RUN`. `h08_r3_worktree_new_is_refused_below_the_floor` passes. Also assert no
   branch and no `worktrees` row. Repeat for `scratch`. MAIN: fails, the worktree is created.
5. V5, `H08-R4`, `NOT RUN`. `h08_r4_message_names_space_floor_and_triage`: assert `1.0 GB`, `20 GB`
   and `horch worktree triage`. MAIN: no message.
6. V6, `H08-R5`, `NOT RUN`. `h08_r5_unknown_reading_does_not_refuse`: `disk_free_bytes: null`. The
   spawn succeeds and stderr has the NOTE. MAIN: the spawn succeeds with no NOTE, so the test fails.
7. V7, `H08-R6`, `NOT RUN`. `h08_r6_exempt_spawns_start_below_the_floor`: 1 case each for
   `--resume`, `--replaces`, `worktree-steward` and an exact review target. A second test queues 1 review and 1 higher-priority work task for 1 persona, with no target, and asserts a refusal. MAIN: each spawn succeeds, so the second test fails.
8. V8, `H08-R7`, `NOT RUN`. `h08_r7_existing_worktree_is_returned_below_the_floor`: create the
   worktree at a high reading, then call `new` at 1 GB. The same path returns. MAIN: passes, so
   this test only guards the fix against a regression.
9. V9, `H08-R8`, `NOT RUN`. Unit test in `messaging/brief.rs`, next to
   `transport_env_carries_identity_paths_and_every_override` (line 272): a brief with the field
   gives the env pair. MAIN: the field does not exist.
10. V10, `H08-R9`, `NOT RUN`. `h08_r9_no_task_and_low_disk_keeps_the_no_task_reason`: 0 startable
    tasks and 1 GB free. Assert that `end_reason` names `startable` and not `floor`. MAIN: passes, so this test only guards the order.
11. V11, manual, `NOT RUN`. Repeat M3 to M6 on the fix. Expect exit 1, and stderr:
    `horch: horch ledger worktree new refused: free disk 1.0 GB is below the floor of 20 GB (HORCH_MIN_FREE_DISK_GB). ...`
    Then `git -C /tmp/h08/p worktree list` shows 1 worktree. MAIN: exit 0 and 2 worktrees.
12. V12, manual, `NOT RUN`. Run `HORCH_MIN_FREE_DISK_GB=0 horch ledger worktree new floor`. Expect
    exit 0. MAIN: exit 0, because MAIN ignores the variable. This step proves only the off switch.
13. V13, ledger baseline, `RUN` on 2026-11-22T23:17Z. This query printed `0`:
    `sqlite3 -readonly ~/.horch/ledger.sqlite3 "select count(*) from workers where
    status='launch_failed' and end_reason like '%below the floor%';"`
14. V14, ledger count after 2 weeks of use, `NOT RUN`. Run the V13 query and the query of
    `## Evidence on main`. Expect no real disk-full event above the 2 false matches.

## Size and risks

Estimate: 1 worker-day. Size S.

Risks:

1. R1: The floor stops only new work. 8 builds that start at 21 GB can still fill the disk.
2. R2: `f_bavail` on APFS can exclude space that macOS can purge (`NOT VERIFIED`). A refusal can
   then occur while Finder shows more free space. `HORCH_MIN_FREE_DISK_GB=0` is the way out.
3. R3: GitHub runners can have less than 20 GB free (`NOT VERIFIED`). File change 10 removes this
   risk for e2e tests. A new test that does not use `Harness` must set the variable.
4. R4: A sandbox can deny `statvfs` (`NOT VERIFIED`). `H08-R5` then lets the command run.

Files that another spec also changes: `crates/horch/src/cmd/spawn.rs` (spec 02), `tasks.rs` (specs 02 and 06), `cmd/taskledger.rs` (specs 01, 06, 07 and 10), and `runtime/mod.rs` (spec 01). This spec calls `prepare` and does not change it.

Open questions for the operator:

1. Q1: Is 20 GB the right default? Recommendation: yes, for the 3 reasons under `### New setting`.
2. Q2: Should a follow-up add ranked sizes, with `<project>/target`, to `horch worktree triage`? Recommendation: yes, as a separate ticket. On 2026-11-22 the `target` of the main checkout was 111.2 GB and `.worktrees` was 13.6 GB (`MEASURED`). The refusal stays constant-time.
3. Q3: Should `horch worktree new` (`CODE crates/horch/src/cmd/worktree.rs:49`), the path outside
   the ledger, also check the floor? Recommendation: no. The ticket names only the ledger command.
