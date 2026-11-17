# Map: fleetd build, Phase 3 (worktree leases and rescue)

Spec: `docs/specs/fleetd/design.md`

| # | Title | Blocked by | Wave |
|---|---|---|---|
| 01 | [The module files of worktree_custody, its tables in the fitness rules, and its row in the architecture doc](01-worktree-custody-modules.md) | T-273 | 1 |
| 02 | [The next schema version: the state, owner and rescue columns of worktrees, and their backfill](02-schema-phase3.md) | T-273 | 1 |
| 03 | [The worktree is an entity of the 1 door: its state table in transition(), and the rows store of worktree_custody](03-worktree-entity-in-the-door.md) | 01, 02 | 2 |
| 04 | [The Phase 3 keys of fleet.toml: the period of the worktree_list job, the worktree disk budget and the time limit of a rescue job](04-fleet-toml-phase3-keys.md) | T-273 | 1 |
| 05 | [The worktree_add and worktree_list jobs, and the start and wait of a job from a command](05-worktree-add-and-list-jobs.md) | 01, 03, 04 | 3 |
| 06 | [The rescue job: a rescue ref under refs/horch/rescue/, read back and verified, never overwritten](06-verified-rescue-job.md) | 05 | 4 |
| 07 | [The worktree_remove job and its guard: only a landed tree or a tree on a verified rescue ref is removed](07-worktree-remove-job-and-guard.md) | 06 | 5 |
| 08 | [horch ledger worktree new and scratch write the reserved row first and run the worktree_add job](08-create-worktree-row-first.md) | 03, 05, 06 | 5 |
| 09 | [The worktree reconciler in report-only mode: adopt, mark orphaned, observe the jobs, and check I6](09-worktree-reconciler-report-only.md) | 03, 06, 07, 08 | 6 |
| 10 | [The pass of Phase 3: the worktree facts in part 1, the reconciler in part 2, the worktree_list job each period, and 1 git writer for each repository](10-pass-wiring-phase3.md) | 09 | 7 |
| 11 | [The claim and the handoff give the tree of the task to the next attempt, and the rescue ref is named](11-claim-reuses-the-tree.md) | 08 | 6 |
| 12 | [Drop, cleanup, dismiss and the run gate keep their refusals and remove a tree only through the rescue and remove jobs](12-drop-cleanup-dismiss-and-gate-through-custody.md) | 07, 08 | 6 |
| 13 | [Triage keeps its commits, refusals and forward-only refs, and removes and restores a tree only through the custody jobs](13-triage-through-custody.md) | 07, 08 | 6 |
| 14 | [A task that cannot land parks with its tree, a requeue resumes it in the same tree, and unlanded work stays on record](14-unlanded-park-requeue-and-record.md) | 08, 11, 12 | 7 |
| 15 | [horch worktrees lists each tree, horch worktree restore turns a rescue ref into a branch and a tree, and the status shows the total size](15-horch-worktrees-and-restore.md) | 05, 08, 09 | 7 |
| 16 | [The worktree disk budget: over it, fleetd removes landed and rescued trees of finished runs, oldest first, and holds new dispatch](16-worktree-disk-budget.md) | 04, 07, 10 | 8 |
| 17 | [Only worktree custody runs git worktree add or remove: the arch_scan rule, and the profiles deny git worktree](17-only-custody-runs-git-worktree.md) | 03, 08, 12, 13, 15 | 8 |
| 18 | [The reconciler acts: it queues the rescue of each orphan and the removal of each landed tree, after the tests of RR-13 to RR-18 are green](18-reconciler-acts.md) | 09, 12, 13, 14, 16 | 9 |
| 19 | [Make the Phase 0 red tests of rescue green: a killed worker loses nothing, and after a herdr restart each tree is rescued and reused](19-p0-rescue-green.md) | 11, 18 | 10 |
| 20 | [The e2e tests of Phase 3: a hand-made tree is adopted, a vanished tree is escalated, the disk budget holds, and a slow worktree add keeps the heartbeat](20-p3-worktrees-e2e.md) | 10, 16, 17, 18 | 10 |
| 21 | [The release test of Phase 3: a live tree stays active, a dirty orphan is listed, and nothing is removed after the update](21-p3-release.md) | 15, 18 | 10 |
| 22 | [Gate of Phase 3: each rule of the gate green, and the gate file](22-gate.md) | 01, 02, 03, 04, 05, 06, 07, 08, 09, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21 | 11 |

## The check of the run

```
cargo test --workspace --locked
```

As in Phases 1 and 2 (`docs/tickets/fleetd-p0/MAP.md`): this command passes when each test that is
not on the list of expected failures passes, and each test on the list fails for its listed reason.
The list is the files in `crates/horch-e2e/expected-failures/`. Ticket 19 makes the 2 red tests of
Phase 3 green (FM-25 and the full FM-13), removes their lines and owns
`crates/horch-e2e/tests/p0_rescue.rs` and `crates/horch-e2e/expected-failures/p0_rescue.txt`.

Each ticket's Check also runs the guard tests:
`cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`.

## Waves

- Wave 1: 01, 02, 04
- Wave 2: 03
- Wave 3: 05
- Wave 4: 06
- Wave 5: 07, 08
- Wave 6: 09, 11, 12, 13
- Wave 7: 10, 14, 15
- Wave 8: 16, 17
- Wave 9: 18
- Wave 10: 19, 20, 21
- Wave 11: 22

The tickets of 1 wave run at the same time: 01, 02 and 04; then 07 and 08; then 09, 11, 12 and 13;
then 10, 14 and 15; then 16 and 17; then 19, 20 and 21. Only ticket 22 is `Parallel: no`. No 2
tickets that can run at the same time own the same file. Tickets that change the same file run one
after the other through their blockers:

- `crates/horch-core/tests/arch_scan.rs`: 01 -> 03 -> 17. `docs/architecture.md`: 01 -> 17.
- `crates/horch-core/src/fleetd/worktree_custody/adapters/executors.rs`: 05 -> 06 -> 07.
- `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`: 05 -> 12.
- `crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs`,
  `domain/worktree.rs` and `crates/horch-core/tests/worktree_reconcile.rs`: 09 -> 18.
  `application/disk.rs`: 16 -> 18.
- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`: 10 -> 16.
- `crates/horch-core/src/taskledger/worktree_ops.rs`: 08 -> 12. `taskledger/finish/mod.rs`: 08 -> 14.
  `taskledger/tasks.rs`: 11 -> 14. `taskledger/import.rs`: 12 -> 14.
- Ticket 01 makes each file of `crates/horch-core/src/fleetd/worktree_custody/`, so that the
  tickets of 1 wave own different files of the context.

Each ticket with no blocker in this map (01, 02, 04) is blocked by `T-273`, the gate of Phase 2.

## Critical path

01 -> 03 -> 05 -> 06 -> 07 -> 09 -> 10 -> 16 -> 18 -> 20 -> 22

## Open questions

Q25 is an answer of the orchestrator. Q1 to Q24 are choices of the planner where the design and the
code of today do not agree, or where the design leaves a point open; each ticket follows them, and
the orchestrator can change each one. The orchestrator has a NOTE of Q1, Q8 and Q9.

- **Q1. Who runs the worktree_add job before Phase 4.** The design: "until Phase 4, `horch spawn`
  queues the `worktree_add` job and waits for it outside the pass". The code: `horch spawn` does not
  know the task (the claim comes after the spawn), and the worker makes its tree after the claim
  with `horch ledger worktree new` or `scratch` (`finish::worktree_new`, `worktree_ops::create`).
  Choice: those commands write the `reserved` row and queue the job in 1 transaction, start the job
  through the launcher when it is still `queued` (a compare-and-set, so the pass and the command
  never both start it), wait with no transaction open, and observe it with the function of the
  pass. The code that runs `git worktree add` is the job executor of worktree custody. Tickets 05,
  08.
- **Q2. The columns.** Today's table has `worktree_id` (key), `path` (unique), `base_commit`,
  `bytes_on_disk`, and a nullable `task_id`. The design has `path` as key, `base_sha`,
  `disk_bytes` and `idea_id`. Choice: keep today's key and columns (`base_commit` is `base_sha`,
  `bytes_on_disk` is `disk_bytes`), `task_id` stays nullable (scratch trees, row W4, and adopted
  orphans with no task). Add `state`, `owner_attempt`, `head_sha`, `dirty`, `rescue_ref` and
  `seen_at`. No `idea_id` before Phase 6. Ticket 02.
- **Q3. 2 state columns.** `status` keeps today's words (`open`, `landing`, `landed`,
  `cleanup_failed`, `cleaned`, `unlanded`, `abandoned`) and what you read today (`horch worktree
  list`, triage, `unfinished`). `state` holds the 8 words of the design and is written only through
  `transition()`. `status` is retired in Phase 4. Tickets 02, 03.
- **Q4. Paths and branch names.** The design: `/fleet/work/T-0007-03` on `fleet/I-0007/T-0007-03`.
  Today: `<project>/.worktrees/T<seq>-<name>` on `<run branch>-T<seq>-<name>`. Choice: keep today's
  paths and names (features and ideas come in Phases 4 and 6). The work directory of the
  reconciler is `<project>/.worktrees/` of each run that is not `done` or `abandoned`. Tickets 08,
  09, 10.
- **Q5. The name of a rescue ref.** The design: `refs/horch/rescue/T-0007-03/1`. Task ids today
  are UUIDs, and a tree can have no task. Choice: `refs/horch/rescue/<worktree id>/<n>`, `n` = 1 +
  the highest `n` of that tree, written create-only. The manifest is
  `<fleet root>/rescue/<worktree id>-<n>.json`. `horch worktree restore T<seq>` finds the ref by
  the task. Tickets 06, 15.
- **Q6. Rescue ref classes and ignored files.** The classes `unique` and `superseded`, their
  retention and `horch rescue drop` are Phase 4 (FM-40, DDD Slice 4). Phase 3 deletes no rescue
  ref. `rescue_include` of a profile is not in the Phase 3 row: Phase 3 saves no ignored file.
  Ticket 06.
- **Q7. What a removal needs (RR-9).** The guard of the `worktree_remove` job: the tree is `landed`
  with its HEAD at `landed_commit` and no modified or untracked file, or it is `rescued` and its
  read-back rescue ref holds exactly the folder now; no stash entry names its branch; the HEAD did
  not move during the checks; each fact was read. Drop, cleanup, dismiss and triage reach the second
  case through a rescue job first. Archive and trash refs of triage stay as today, beside it.
  Tickets 07, 12, 13.
- **Q8. When fleetd removes a tree in Phase 3.** Phase 3 has no ideas. Choice: fleetd removes a
  `landed` tree (as today's cleanup step), and over the disk budget the `landed` and `rescued` trees
  whose run is `done` or `abandoned` (the stand-in for an idea `merged` or `cancelled`), oldest
  first. "Ideas finish clean" (the removal of rescued trees when an idea merges) is Phase 6. A
  rescued tree with no task is never removed by fleetd. Tickets 09, 16, 18.
- **Q9. Report-only, then acting.** Report-only is a stage of the build, not a key of `fleet.toml`:
  tickets 09 and 16 write the rows and report, and queue no rescue and no remove job. Ticket 18
  turns the queues on after the tests of RR-13 to RR-18 (tickets 12, 13, 14) pass, and it checks
  them first. The release test (21) checks that the orphan is listed and that nothing is removed.
- **Q10. The tree states in today's flow.** Claim or handoff -> `active` with the live attempt;
  land step starts -> `landing`; land passed -> `landed`; a red land and new commits keep
  `landing`; `done --unlanded` -> `created` with no owner; an attempt that ended `crashed`,
  `killed`, `stalled`, `timed_out` or `no_result`, or a cancelled task -> `orphaned`; `rescued` ->
  `active` on the next claim; a failed `worktree_add` drops the `reserved` row. A `landing` tree
  whose owner dies stays `landing`, and the next attempt takes it. Tickets 03, 09, 11, 12, 14.
- **Q11. The git scan.** The `worktree_list` job of each period reads `git worktree list
  --porcelain` and, for each tree of a row, the HEAD, the dirty flag and the size in bytes. The
  pass reads its result. Tickets 05, 09, 10.
- **Q12. 1 git writer for each repository.** The design rule (in "Between features") applies from
  the first jobs that change git state, and Phase 3 has 3 (`worktree_add`, `worktree_remove`,
  `rescue`). Today the SQLite writer lock serializes the creations (row W1); without the rule,
  parallel `git worktree add` calls can fail on git locks. Choice: the pass and a command start at
  most 1 of these jobs for 1 repository at a time. Tickets 05, 10.
- **Q13. "`horch status` shows the total size".** `horch status` is `horch ledger status` today.
  Ticket 15.
- **Q14. `horch worktree restore`.** Today it restores an archive or trash ref by name. Choice: a
  task (`T<seq>`) or a worktree id with a rescue ref gives the branch `rescue/T<seq>-<n>` and the
  tree `<project>/.worktrees/rescue-T<seq>-<n>` through a `worktree_add` job; each other name keeps
  today's restore. `horch worktrees` is a new command in `crates/horch/src/cmd/worktree.rs`.
  Ticket 15.
- **Q15. The claims of `horch worktree new` and `land`.** `crates/horch-core/src/vcs/landing.rs`
  makes trees that belong to no ledger run. Choice: they keep their git calls in Phase 3, as the 1
  exemption of the `arch_scan` rule, with this reason. The reconciler adopts such a tree in the
  project of a run as `orphaned` with no task, rescues it, and never removes it. The run gate folder
  (`runs.rs`, row W11) moves behind the custody jobs. `git worktree list` is a read and is not in
  the rule. Tickets 12, 17.
- **Q16. The deny rules of the profiles.** Claude Code: `permissions.deny` gets `Bash(git worktree
  add:*)`, `remove` and `prune`. Codex: `forbidden` `prefix_rule`s for the same 3. On Windows,
  codex has 1 shared rules file with no forbidden rule, so there the reconciler alone catches a tree
  that a worker makes. Other CLIs: no rule; the reconciler. Ticket 17.
- **Q17. The attempt waits for its tree.** Phase 3 attempts start before the tree exists (the
  worker makes it after the claim). The gate test "the attempt launches only after its
  `worktree_add` job passed" is, in Phase 3: `horch ledger worktree new` returns the path only
  after the job passed. `START.md` with the rescue commit is Phase 4; Phase 3 prints the rescue ref
  in the claim output. Tickets 08, 11, 20.
- **Q18. The drop of a `reserved` row.** The design: "A job that failed drops the row". Choice: the
  door deletes a `reserved` row with an event and its cause; it is the only delete of the door.
  Ticket 03.
- **Q19. The release backfill.** `cleaned` -> `removed`; a row whose task has a `live` attempt of
  the worker of the row -> `active` with that attempt; a scratch row of a worker with a `live`
  attempt -> `active`; each other row -> `orphaned`. The migration reads no process and no folder;
  the first pass checks again. Tickets 02, 21.
- **Q20. The disk budget key.** `worktree_disk_budget_gb` = 20 (the design's `worktree_disk_budget`
  with its unit in the name, as each key of Phases 1 and 2). Over the budget with nothing left to
  remove: a global hold `WorktreeDisk` on dispatch and 1 needs-you item. Tickets 04, 16.
- **Q21. The Phase 0 tests.** Ticket 26 of fleetd-p1 made the first FM-13 test green. Ticket 19
  makes the 2 tests of phase 3 in `p0_rescue.txt` green.
- **Q22. The dependency direction of the executors.** `run_job` of fleet runtime calls
  `worktree_custody::adapters::executors::run` for the 4 worktree kinds. No fitness rule forbids it
  (only `domain/` files have the import rule). Ticket 05.
- **Q23. A tree with no live attempt in a test.** Many e2e tests play the agent with no attempt
  row. A tree of such a worker is `active` with a null owner while its worker is live, and
  `orphaned` when the worker is not live. Tickets 08, 09.
- **Q24. Module files.** 1 file for each concern under `domain/`, `application/` and `adapters/`
  (more files than `ddd/08-code` lists), so that tickets of 1 wave own different files. Ticket 01.
- **Q25. The time limit of a rescue job (answer of the orchestrator).** `job_limit_s.rescue` = 600 s,
  a starting value with its key. A rescue job that reaches its limit is never read as `rescued`. The
  tree stays, the job runs again by the rule of `abandoned` runs, and after `abandoned_run_limit` a
  needs-you item opens. No tree is removed without a verified rescue. Tickets 04, 05, 06.

## Coverage

Each mechanism of the row Phase 3 in "What each gate needs" and in "New work for each phase" of the
design, each mechanism of item 5 of the Migration plan, and each rule of the gate line, with the
ticket that gives it.

| Mechanism or rule | Ticket |
|---|---|
| The module files of `worktree_custody` | 01 |
| The state, owner, base and rescue columns of `worktrees`, and the release backfill | 02 |
| The worktree state table in `transition()` (the 1 door) | 03 |
| The keys `worktree_list_period_s`, `worktree_disk_budget_gb`, `job_limit_s.rescue` | 04 |
| The `worktree_add` and `worktree_list` jobs; 1 git writer for each repository (start from a command) | 05 |
| The verified rescue to `refs/horch/rescue/` (the rescue job and its verification) | 06 |
| The `worktree_remove` job and the guard of RR-9 | 07 |
| fleetd (through the worker commands until Phase 4) as the only code that runs `git worktree add`, the row first (FR-11) | 08 |
| The worktree reconciler in report-only mode; I6 | 09 |
| The reconciler in the pass; the `worktree_list` job each period; 1 git writer for each repository (the pass) | 10 |
| Reuse by the next attempt | 11 |
| The refusals of today's drop and cleanup, and `git worktree remove` only through the jobs (RR-13) | 12 |
| The refusals of today's triage; forward-only refs (RR-16, RR-17) | 13 |
| The record of unlanded work; park and requeue in the same tree (RR-14, RR-15, RR-18) | 14 |
| `horch worktrees` and `horch worktree restore`; the total size in the status | 15 |
| The worktree disk budget (FM-28) | 16 |
| The `arch_scan` rule of FR-11; the deny rules of the profiles (FM-26) | 17 |
| The worktree reconciler acting, after the tests of RR-13 to RR-18 | 18 |
| Gate tests: FM-25 and the full FM-13 (the Phase 0 red tests) | 19 |
| Gate tests: FR-11, RR-10, FM-26, FM-27, FM-28, the slow `worktree_add` | 20 |
| The release row of Phase 3 (a run in flight) | 21 |
| The gate file | 22 |
| The jobs and the launcher | Phase 1 (no ticket) |
| The `needs_you` table | Phase 1 (no ticket) |
| `horch task requeue` | Phase 2 (no ticket) |
