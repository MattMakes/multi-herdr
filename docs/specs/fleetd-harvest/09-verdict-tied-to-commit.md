# 09: Tie a check verdict to the commit it judged

Status: proposed. Target: `main` at `06930d95`. Source of the idea: fleetd at `9bc3bf70`.
Report id: R4 P7 and W5. Size: S.

Labels: `CODE` means read at the pinned commit. `MEASURED` means a read-only query that I ran.
Paths under `finish/` are in `crates/horch-core/src/taskledger/finish/`.

## Problem

**The problem is only partly real on MAIN.** The report says that the land step lands a tip that
no check judged. For the normal case, this is wrong. 3 narrow gaps remain.

What MAIN already does:

1. Each step stores the git tree that it judged in `task_steps.judged_rev`.
   `CODE finish/mod.rs:798-818`, `CODE taskledger/git.rs:132`
2. Each finish runs a passed step again when its tree is not the current tree. A comment-sweep
   commit changes the tree, so `check` runs again. `CODE finish/mod.rs:656-666`
3. The land step rebases, runs the check again, then fast-forwards. `CODE finish/land.rs:583-603`
4. A commit after the land reopens the land and the earlier steps. `CODE finish/mod.rs:635-654`
5. The cleanup step refuses a branch with unlanded commits. `CODE finish/land.rs:852-864`

What goes wrong for the operator: a worker or a check command can add a commit while finish runs.
In 2 cases that commit reaches the run branch without the verdict that the operator expects. In
1 case the commit leaves the branch. Each case needs a commit inside a short time window.

The exact causes:

1. **F1, a commit during the land check lands unjudged.** `rebased_and_checked` runs the check at
   `finish/land.rs:592` and reads the commit to land at `:595`, after the check. The condition:
   `HEAD` moves while `check_in` runs. `CODE`
2. **F2, a red land check can drop a commit.** `land_stages` saves `tip` at `finish/land.rs:490`.
   A red check calls `undo_rebase`, and `reset_to` runs `git reset --hard <tip>` (`:496`, `:503`,
   `:521`). The condition: `HEAD` moves during a check that fails. The commit stays only in the
   reflog. `CODE`
3. **F3, a commit after a sweep step skips that step.** The loop tests each step 1 time
   (`finish/mod.rs:753-758`). The land does not compare earlier verdicts with the tip
   (`finish/land.rs:472-497`). After the land passes, `frozen` keeps them valid
   (`finish/mod.rs:668-671`). The condition: a commit arrives after a sweep passes and before the
   land starts. The land check judges the commit, but the sweep does not. `CODE`

## Evidence on main

The measured count of a real fault is **0 of 383** landed tasks with a check. The count is a lower
bound. Query date: 2026-11-22 23:15 UTC. Ledger window: 2026-10-31 to 2026-11-22, all projects.
`MEASURED`

Method, on `~/.horch/ledger.sqlite3` with `mode=ro`:

```sql
SELECT w.task_id, w.base_commit, w.landed_commit, w.landed_at, w.repo_root, t.run_id, s.worker_id
FROM task_steps s
JOIN tasks t ON t.task_id = s.task_id
JOIN worktrees w ON w.task_id = s.task_id AND w.landed_commit = s.verdict
WHERE s.name = 'land' AND s.status = 'passed';
```

For each row:

1. Read the latest `ended_at` of the passed `check` or `recheck` step with a `judged_rev`.
2. List the commits from the previous land of the run to `landed_commit`, newest first, with
   `git log --no-merges --format='%H %at %s'`.
3. Count a hit when the newest commit has an author time after that `ended_at`.

Results: `MEASURED`

1. 455 tasks landed. 72 have no judged check. 383 were measured. 0 are hits.
2. 18 later commits sit below a task commit. They are run-branch commits that the task rebased
   onto, and they are not faults. Example: `trace-fixes` T-22, landed 2026-11-06T02:37:11Z,
   commit "trace-fixes: add ticket 41 (outbox lock release)".
3. All 400 passed `check` rows and all 94 passed `recheck` rows have a `judged_rev`.

Limits: an amended commit keeps its old author time. A commit made during the last check step has
an author time before `ended_at`. So the query can miss F1. It cannot see F2, because a dropped
commit is not in the landed history.

## How fleetd solves it

The rule: **a verdict stands only for the tip that it judged.**

1. `Verified::stands` compares the task, verdict number, attempt, decision and tip.
   `CODE 9bc3bf70:crates/horch-core/src/fleetd/task_execution/domain/verification.rs:254-261`
2. `verifier::stands` reads the newest verdict and attempt, and `verifier::done` refuses a verdict
   that no longer stands.
   `CODE 9bc3bf70:crates/horch-core/src/fleetd/task_execution/application/verifier.rs:472`, `:528`

The idea is 1 comparison: the id that the verdict judged against the id that the next step acts
on. MAIN has the same rule for a run: `run_gate` records `tip_sha`, and `run close` refuses a tip
with no passing gate. `CODE taskledger/runs.rs:803-812`, `:837-852`

Check jobs, attempts, verdict files, custody and the generation guard exist only for the daemon.

## Fix

The fix changes 1 file. It adds no column and no setting.

Requirements:

1. **H09-R1.** Before the land rebases, it compares the tree of `HEAD` with `judged_rev` of each
   earlier step that passed or is not applicable. If 1 differs, the land fails and no ref moves.
   The note names each stale step.
2. **H09-R2.** The land reads the `HEAD` commit before and after its check. If the 2 ids differ,
   the land fails and does not fast-forward. This applies to a green and to a red check.
3. **H09-R3.** A land that fails for H09-R1 or H09-R2 does not run `git reset`. The branch keeps
   each commit.
4. **H09-R4.** The next `horch ledger finish` runs the stale steps again and then lands, with no
   manual repair.
5. **H09-R5.** The land does not refuse a normal case. A rebase on a moved run branch still lands.
   A step with a `NULL` `judged_rev`, such as `check` of a `**Check:** none` task, is not compared.

Changes in `crates/horch-core/src/taskledger/finish/land.rs`:

1. Add the variant `Moved(String)` to `enum Stop` (`:452`).
2. Add this function. Call it in `land_stages` after `still_mine` (`:481`) and before
   `set_worktree` (`:482`):

   ```rust
   fn require_judged_tip(store: &Store, cx: &Ctx<'_>, dir: &Path) -> Result<(), Stop> {
       let tree = cx.tools.git.tree(dir)?;
       let rows = super::steps_of(store.conn(), &cx.task.task_id, &cx.worker.worker_id)?;
       let stale: Vec<&str> = rows
           .iter()
           .filter(|r| r.seq < cx.seq && super::through(&r.status))
           .filter(|r| r.judged_rev.as_deref().is_some_and(|rev| rev != tree))
           .map(|r| r.name.as_str())
           .collect();
       if stale.is_empty() {
           return Ok(());
       }
       Err(Stop::Moved(format!(
           "the tip moved since {} judged it; nothing landed; run finish again",
           stale.join(", ")
       )))
   }
   ```

3. Replace `:592-595` in `rebased_and_checked`:

   ```rust
   let judged = git.rev(dir, "HEAD")?;
   let failed = check_in(cx, dir, log);
   let new = git.rev(dir, "HEAD")?;
   if new != judged {
       return Err(Stop::Moved(format!(
           "the tip moved during the land check, from {} to {}; nothing landed; run finish again",
           &judged[..judged.len().min(12)],
           &new[..new.len().min(12)]
       )));
   }
   if let Some((_, why)) = failed? {
       return Err(Stop::Red(format!("after the rebase: {why}")));
   }
   ```

4. In `undo_rebase` (`:500`), use `matches!(stop, Stop::NotMine(_) | Stop::Moved(_))`.
5. In `land` (`:422`), use `Err(Stop::Red(why) | Stop::Moved(why)) => red(store, cx, &wt, log, why),`.

H09-R2 compares a commit id, because that id is the exact commit that lands. H09-R1 compares a
tree id, because `judged_rev` holds a tree id.

Not ported, and why:

1. Jobs, attempts, verdict files and custody. The orchestrator workflow does not need them.
2. The check hash in the key. The land always runs the current check command.
3. A compare in `land_shared` (`finish/land.rs:792`). Other workers commit to the shared branch,
   so a refusal would repeat. 0 shared tasks have a land row in the ledger. `MEASURED`
4. A compare in `horch ledger done`. The cleanup step already refuses unlanded commits.
5. Detection of uncommitted edits during a check. See Q1.

Effect at the upgrade: a ledger needs no migration, and a run continues with no change. A worker
in a finish keeps the old code until its process ends. Its next finish uses the fix.

## Reproduce

Each step in this section is **NOT RUN**, because the run rules forbid `cargo` and a process start.
The expected output comes from the code at `06930d95`.

### Automated

Crate: `horch-core`. File: the `tests` module of `crates/horch-core/src/taskledger/finish/land.rs`.
Copy the `World` harness and the style of `trf_16_a_task_with_no_check_runs_the_run_default_check`
(`:1319`). `World` starts with 1 feature commit in a worktree and the steps `[Land]`.

Add these constants. Each check command adds a commit the first time that it runs:

```rust
const LATE: &str =
    "test -f late.txt || { echo late > late.txt; git add late.txt; git commit -q -m late; }";
const LATE_RED: &str =
    "test -f late.txt && exit 0; echo late > late.txt; git add late.txt; git commit -q -m late; exit 1";
```

First failing test, for F1. **NOT RUN**

```rust
#[test]
fn h09_r2_a_commit_during_the_land_check_does_not_land() {
    let mut w = World::new(LATE);
    let before = git(&w.root, &["rev-parse", RUN_BRANCH]);
    let r = w.finish();
    assert!(!r.complete, "{r:?}");
    assert_eq!(git(&w.root, &["rev-parse", RUN_BRANCH]), before);
}
```

MAIN today: `r.complete` is true, so the first assertion fails. Run it with
`cargo test -p horch-core --lib taskledger::finish::land::tests::h09`.

### Manual

**NOT RUN.** Use a new terminal that is not a fleet pane. The scratch ledger protects the live one.

1. Make the scratch project, the run and the task:

   ```sh
   T=$(mktemp -d); export HOME=$T/home HORCH_LEDGER_DB=$T/ledger.sqlite3
   export HORCH_NO_WAKE=1 HORCH_NO_WATCHDOG=1
   mkdir -p "$HOME" "$T/repo/tickets"; cd "$T/repo"
   git init -q -b main; git config user.name t; git config user.email t@example.com
   git config commit.gpgsign false
   printf '# 01: Feature\n' > tickets/01-a.md; git add .; git commit -qm init
   horch ledger run new alpha --steps check,land
   horch ledger add --file "$PWD/tickets/01-a.md" --primary opus \
     --check 'test -f late.txt || { echo late > late.txt; git add late.txt; git commit -q -m late; }'
   export HORCH_WORKER_ID=$(horch ledger worker new --persona opus)
   horch ledger claim
   ```

2. Run `horch ledger worktree new feat`. Then `cd` to the path that it prints.
3. Commit the work: `echo f > f.txt; git add f.txt; git commit -qm feature`
4. Run `horch ledger finish --json`. MAIN prints `"complete": true` and exits 0.
5. Run `git -C "$T/repo" log --format=%s fleet/alpha -2`. MAIN prints `late`, then `feature`. The
   `check` step judged the tree of `feature` only.

## Validate

Each step in this section is **NOT RUN**, for the reason in `## Reproduce`.

Automated, 1 test for each requirement, in the same `tests` module:

1. **H09-R1**, `h09_r1_a_commit_after_the_check_step_stops_the_land`. **NOT RUN**
   Setup: `World::new(LATE)`, then `set_steps(&[StepName::Check, StepName::Land])`.
   Assert: the first finish is not complete, its land note contains `check judged it`, and the
   run branch did not move. MAIN today: FAILS, because the finish is complete.
2. **H09-R2**, `h09_r2_a_commit_during_the_land_check_does_not_land`, from `## Reproduce`.
   **NOT RUN**. MAIN today: FAILS, because the finish is complete.
3. **H09-R3**, `h09_r3_a_red_land_check_keeps_a_commit_made_during_it`. **NOT RUN**
   Setup: `World::new(LATE_RED)`, then 1 finish. Assert: the finish is not complete, `late.txt`
   exists in `w.wt`, and `HEAD` has the subject `late`.
   MAIN today: FAILS, because `git reset --hard` removes the commit and the file.
4. **H09-R4**, `h09_r4_the_next_finish_judges_the_new_tip_and_lands`. **NOT RUN**
   Setup: as for H09-R1, then 2 finishes. Assert: the first is not complete. The second is
   complete, `check` has `rounds == 2`, and the run branch holds `late.txt`.
   MAIN today: FAILS at the first assertion.
5. **H09-R5**, `h09_r5_a_rebase_and_a_check_none_task_are_not_refused`. **NOT RUN**
   Case 1: `World::new("true")`, steps `[Check, Land]`, `w.move_run_branch()`, 1 finish.
   Case 2: `World::shaped(|t| t.check_none = true)`, steps `[Check, Land]`, 1 finish.
   Assert: each finish is complete. MAIN today: PASSES. This test does not prove the fix. It
   guards against a false refusal.

Existing tests that must still pass. **NOT RUN**. MAIN today: not measured in this run. Run
`cargo test -p horch-core --lib taskledger::finish::land` and
`cargo test -p horch-e2e --test taskledger a_commit_after_the_land_is_checked_and_reviewed_again`.

Manual, with the fix. **NOT RUN**

1. Do steps 1 to 3 of the manual steps in `## Reproduce`, with the fixed binary.
2. Run `horch ledger finish --json`. New output: exit 1, `"stopped_at": "land"`, and a message
   with `the tip moved since check judged it`. MAIN today: exit 0 and `"complete": true`.
3. Run `git -C "$T/repo" log --format=%s fleet/alpha -1`. New output: `init`. MAIN today: `late`.
4. Run `horch ledger finish --json` again. New output: exit 0, `"complete": true`, a new `check`
   round, and `late` on `fleet/alpha`. MAIN today: exit 0, with no new `check` round.

Ledger count, 2 weeks after the fix lands. **NOT RUN**

```sql
SELECT COUNT(*) FROM events WHERE kind = 'land_red' AND text LIKE '%tip moved%';
```

MAIN today: 0 of 33 `land_red` events. `MEASURED`. Each count above 0 is a land that the fix
stopped. A count of 0 means that the guard did not fire, and it is not a defect.

## Size and risks

Estimate: 0.5 worker-day. The change is about 40 lines of code and 5 tests in 1 file.

Risks:

1. A worker that commits during finish now sees a failed land and must run finish again.
2. A check command that adds a new commit on each run fails each land. Such a check is a defect.
3. A small window remains. A commit between `require_judged_tip` and the read of `judged` gets the
   land check but not the sweeps. H09-R2 still ensures that the check judged the landed commit.

Other specs: spec 01 (check sandbox) also changes `finish/land.rs`. Land the 2 fixes in sequence.

Open questions for the operator:

1. **Q1.** Must the land also refuse when tracked files have uncommitted edits during a check? I
   recommend no for this fix. A check can create files, so a compare after it needs its own design.
