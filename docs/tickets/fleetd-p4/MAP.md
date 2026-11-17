# Map: fleetd build, Phase 4 (push dispatch and the result gate)

Spec: `docs/specs/fleetd/design.md`

| # | Title | Blocked by | Wave |
|---|---|---|---|
| 01 | [The module files of Phase 4: the integration context, the new files of task_execution, worker_profiles and worktree_custody, and their rows in the fitness rules and the architecture doc](01-phase4-modules.md) | T-302 | 1 |
| 02 | [The next schema version: features, branches, bundles, bundle_members, gate_runs, remote_tips and ingested, the push columns of attempts, and the release backfill](02-schema-phase4.md) | T-302 | 1 |
| 03 | [The Phase 4 keys of fleet.toml, the time limits of the 5 job kinds that have none, and the route of the integration job kinds](03-fleet-toml-phase4-keys.md) | 01 | 2 |
| 04 | [The project declares its gates: [gates] with fast and full in the project fleet.toml, the pr and remote settings, the hold "missing fast gate", and the gates of this repository](04-project-gates.md) | 01 | 2 |
| 05 | [The 4 new commands in the CLI: horch result, horch feature abandon, horch branch abandon and horch rescue drop, each in its own file](05-cli-commands-phase4.md) | T-302 | 1 |
| 06 | [The task branch, the bundle and the feature are entities of the 1 door: their state tables in transition(), and the rows store of integration](06-integration-entities-in-the-door.md) | 01, 02 | 2 |
| 07 | [task.yaml and START.md: fleetd writes them once for each attempt, atomically, with no secret in them](07-task-yaml-and-start-md.md) | 01 | 2 |
| 08 | [Push dispatch: 1 function binds a ready task to a new attempt before its pane exists, and fleetd decides which ready tasks to dispatch](08-push-dispatch-bind.md) | 02, 04, 06, 07 | 3 |
| 09 | [horch spawn binds its task before the pane, CLAIM_TASK goes away, and the claim and the finish stay only for an attempt that began with a claim](09-horch-spawn-binds.md) | 08 | 4 |
| 10 | [fleetd launches each attempt that it bound: the spawn after the commit, through the launcher, with stops before launches](10-fleetd-launches-dispatched-attempts.md) | 08, 09 | 5 |
| 11 | [The worker briefing of a push attempt: read START.md, work in the named tree, commit, run horch result](11-worker-briefing-push.md) | 09 | 5 |
| 12 | [horch result writes result.json of the attempt, with its schema, and horch result --check tells a Stop hook whether the turn may end](12-horch-result.md) | 05, 07 | 3 |
| 13 | [fleetd ingests result.json and question files of each attempt with the checks of SR-6 and the ingested table](13-ingest-result-and-questions.md) | 06, 12 | 4 |
| 14 | [The Stop hooks of Claude Code, Codex and Antigravity: a push attempt cannot end its turn without result.json or a question file](14-stop-hooks.md) | 12 | 4 |
| 15 | [The repair pass: an agent that exits with commits and no result gets 1 short resume of its session to write the result](15-repair-pass.md) | 12, 13 | 5 |
| 16 | [A QUESTION of a push attempt also writes its question file in outbox/questions/](16-question-file.md) | 07, 13 | 5 |
| 17 | [The check sandbox of the platform layer: run 1 command so that it can write only a list of paths, on macOS and Linux, and NotSupported elsewhere](17-platform-check-sandbox.md) | T-302 | 1 |
| 18 | [The check of a task runs only in the sandbox profile of its worker; a persona with no check sandbox gets no dispatch, 1 needs-you item and 1 status line](18-check-runs-in-the-worker-sandbox.md) | 03, 08, 17 | 4 |
| 19 | [The verifier: a task in verifying gets its check as a task_check job, verdict.json is written, and only the verifier moves a task to done](19-verifier.md) | 13, 18 | 5 |
| 20 | [The review steps carry over unchanged: code-sweep rounds, findings, the answers file, waivers, complexity-sweep, recheck and the verification profiles, run by the verifier](20-review-steps-carry-over.md) | 19 | 6 |
| 21 | [The structural check of a judgment kind: the task passes on its structural check and today's finish steps until the judge pair of Phase 6](21-structural-check.md) | 20 | 7 |
| 22 | [SR-3: no agent pushes, deploys or messages outside the fleet: the deny rules of the profiles and a pre-push hook in the environment of each agent](22-agents-never-push.md) | 11, 14 | 6 |
| 23 | [The git gateway of integration: compare-and-set ref moves, the merge-tree candidate, fleet branch and stash reads, and the by-content check of I8](23-integration-git-gateway.md) | 01 | 2 |
| 24 | [Rescue refs of branches and the 2 classes of rescue refs: unique is kept until you decide, superseded expires, and a budget opens 1 needs-you item](24-rescue-ref-classes.md) | 06, 23 | 3 |
| 25 | [The landing state machine of a task branch: it follows its task, each state has a time limit, a cancelled task's branch is rescued and abandoned, and a detached old finish blocks landing](25-landing-state-machine.md) | 06, 19, 24 | 6 |
| 26 | [I2: a dependency is met when its task is done and its task branch is landed](26-i2-dependency-met-when-landed.md) | 06 | 3 |
| 27 | [The bundle_build job: it builds the candidate ref refs/horch/bundle/<id> from the base tip and each member, and names a member that conflicts](27-bundle-build-job.md) | 03, 23 | 3 |
| 28 | [The fast_gate and full_gate jobs: the declared gate of the project runs on a tip in the gate tree of the feature](28-gate-jobs.md) | 03, 04, 23 | 3 |
| 29 | [Bundles: fleetd builds 1 candidate of the ready branches of 1 feature, gates it 1 time, and continues each bundle from its row after a restart](29-bundles.md) | 25, 27, 28 | 7 |
| 30 | [A failed bundle is bisected: each half is a child bundle, the failing branch goes to gate_failed, and the others go on](30-bisect.md) | 29 | 8 |
| 31 | [A passed bundle moves the feature ref by compare-and-set; the move is observed before landed; a failed compare-and-set drops the bundle](31-ref-move-by-compare-and-set.md) | 29 | 8 |
| 32 | [fleetd deletes the candidate ref of each ended bundle and verifies the deletion](32-candidate-ref-deletion.md) | 30, 31 | 9 |
| 33 | [Resolver and fix attempts: fleetd starts an attempt of a task branch in conflicted or gate_failed, in its worktree, with the conflict or the gate output](33-resolver-and-fix-attempts.md) | 10, 25, 30 | 9 |
| 34 | [A failed resolver or fix attempt adds 1 to repair_attempts and starts the next after the backoff; at max_attempts 1 needs-you item; a requeue of a done task resets it](34-repair-attempt-failures.md) | 15, 33 | 10 |
| 35 | [horch branch abandon: fleetd writes a rescue ref of the branch, then moves it to abandoned](35-horch-branch-abandon.md) | 05, 24, 25 | 7 |
| 36 | [The feature state machine of the 1 feature of a run: open, ready after the full gate, merged by content at the fetched primary tip](36-feature-state-machine.md) | 25, 28, 37 | 7 |
| 37 | [The fetch job: each period is a new run, remote_tips records each fetched tip, and a failed fetch shows since when the tip is stale](37-fetch-job.md) | 03, 23 | 3 |
| 38 | [The push job: the feature branch is pushed with --force-with-lease on seen_remote_tip, a failure backs off, and 3 failures open 1 needs-you item](38-push-job.md) | 37 | 4 |
| 39 | [The pull request follows the feature state: gh opens it when ready and marks it draft when open again; manual shows the compare URL](39-pull-request-follows-the-state.md) | 36, 38 | 8 |
| 40 | [horch feature abandon: a rescue ref for each branch that is not landed, each such branch abandoned, each task that is not terminal cancelled](40-horch-feature-abandon.md) | 35, 36 | 8 |
| 41 | [The rebase job: when the fetched primary tip moves, the feature branch is rebased on it after a rescue ref of its old tip, never while a bundle is gating or passed](41-rebase-job.md) | 24, 29, 37, 38 | 8 |
| 42 | [A rebase that conflicts opens 1 resolver task in the run, with the conflict in zdiff3 form](42-rebase-conflict-resolver-task.md) | 33, 41 | 10 |
| 43 | [horch rescue drop: you delete 1 unique rescue ref, verified](43-horch-rescue-drop.md) | 05, 24 | 4 |
| 44 | [I11 and I12: a fleet branch with no row is adopted as an orphan and never deleted; a branch is deleted only when landed, merged or rescued and no stash names it](44-orphan-branches-and-deletion-guard.md) | 23, 24, 42 | 11 |
| 45 | [horch status shows the Phase 4 facts: the hold missing fast gate, a persona with no check sandbox, the bundle ref count, the rescue classes, the push state and a stale remote](45-status-lines-phase4.md) | 04, 18, 24, 32, 37, 38 | 10 |
| 46 | [The pass of Phase 4, part 1: dispatch, launch, the outbox scan and ingest, the verifier, the review steps and the repair attempts in the reconcile pass, in the order of the design](46-pass-wiring-dispatch-and-verifier.md) | 10, 13, 15, 16, 20, 21, 26, 34 | 11 |
| 47 | [The pass of Phase 4, part 2: landing, bundles, the ref move, candidate refs, the feature, fetch, push, rebase, rescue classes and the checks of I8 to I12 in the reconcile pass](47-pass-wiring-integration.md) | 24, 30, 31, 32, 36, 39, 40, 41, 43, 44, 46 | 12 |
| 48 | [The fake agents of Phase 4: scenario keys that record the binding at process entry, commit a file, run horch result, write a question, or exit with no result](48-fake-agents-phase4.md) | 12, 16 | 6 |
| 49 | [Make the Phase 0 red tests of dispatch green: FM-2 checks the binding at process entry, and FM-6 leaves the work to the next attempt](49-p0-dispatch-green.md) | 11, 14, 46, 48 | 12 |
| 50 | [Make the Phase 0 red test of the bundle green: 3 ready branches land with 1 fast gate run](50-p0-landing-bundle-green.md) | 47, 48 | 13 |
| 51 | [The e2e tests of the verifier: FM-9 for a code kind and for the structural check, and SR-4](51-p4-verifier-e2e.md) | 18, 46, 48 | 12 |
| 52 | [The e2e tests of the no-loss invariants I8 to I12, 1 for each](52-p4-no-loss-e2e.md) | 47 | 13 |
| 53 | [The e2e tests of bundles: FM-36 and FM-37, the dropped bundle, and the deletion of candidate refs](53-p4-landing-e2e.md) | 47 | 13 |
| 54 | [The e2e tests of FM-38: a resolver attempt after a conflict, a failed resolver attempt starts the next, and max_attempts opens 1 needs-you item](54-p4-repair-e2e.md) | 47, 48 | 13 |
| 55 | [The e2e tests of the remote: a transient push failure is followed by a new run, and 2 fetch periods make 2 fetch runs](55-p4-remote-e2e.md) | 47 | 13 |
| 56 | [The e2e tests of FM-40: a unique rescue ref stays until horch rescue drop, a superseded ref expires, and the budget opens 1 item](56-p4-rescue-refs-e2e.md) | 47 | 13 |
| 57 | [The e2e tests of PL-4 and PL-5 for Phase 4: a sleep past a branch deadline fires nothing, and a fast gate and a full gate of 5 min each keep fleetd healthy](57-p4-clock-and-heartbeat-e2e.md) | 47 | 13 |
| 58 | [The e2e tests of SR-2 and SR-3 and FR-5 through the binary](58-p4-safety-e2e.md) | 22, 46, 48 | 12 |
| 59 | [The release test of Phase 4: a claimed attempt with a detached finish ends by the old path and lands 1 time, and a run of a project with no gates loses no task](59-p4-release.md) | 09, 47 | 13 |
| 60 | [Gate of Phase 4: each rule of the gate green, and the gate file](60-gate.md) | 01, 02, 03, 04, 05, 06, 07, 08, 09, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 61 | 14 |
| 61 | [The health guards compare numbers of the same scope, and a fact that the platform does not support gives no hold](61-health-scope.md) | 04 | 3 |
## The check of the run

```
cargo test --workspace --locked
```

As in Phases 1 to 3 (`docs/tickets/fleetd-p0/MAP.md`): this command passes when each test that is
not on the list of expected failures passes, and each test on the list fails for its listed reason.
The list is the files in `crates/horch-e2e/expected-failures/`. Ticket 49 makes the red tests of
FM-2 and FM-6 green and owns `crates/horch-e2e/tests/p0_dispatch.rs` and
`crates/horch-e2e/expected-failures/p0_dispatch.txt`. Ticket 50 makes the bundle test of Phase 4
green and owns `crates/horch-e2e/tests/p0_landing.rs` and
`crates/horch-e2e/expected-failures/p0_landing.txt`.

Each ticket's Check also runs the guard tests:
`cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`.

## Waves

- Wave 1: 01, 02, 05, 17
- Wave 2: 03, 04, 06, 07, 23
- Wave 3: 08, 12, 24, 26, 27, 28, 37, 61
- Wave 4: 09, 13, 14, 18, 38, 43
- Wave 5: 10, 11, 15, 16, 19
- Wave 6: 20, 22, 25, 48
- Wave 7: 21, 29, 35, 36
- Wave 8: 30, 31, 39, 40, 41
- Wave 9: 32, 33
- Wave 10: 34, 42, 45
- Wave 11: 44, 46
- Wave 12: 47, 49, 51, 58
- Wave 13: 50, 52, 53, 54, 55, 56, 57, 59
- Wave 14: 60

The tickets of 1 wave run at the same time. Only ticket 60 is `Parallel: no`. No 2 tickets that
can run at the same time own the same file.

The groups of the phase:

- Foundation: 01 (module files), 02 (schema), 03 (keys of `fleet.toml`), 04 (project gates), 05
  (CLI commands), 06 (the task branch, the bundle and the feature in the 1 door), 61 (health guards of the same scope).
- Dispatch and result: 07 to 16 (push dispatch, `task.yaml` and `START.md`, the end of
  `CLAIM_TASK`, the Stop hooks, `horch result`, the ingest, the repair pass and the question file).
- Verifier: 17 to 22 (the check sandbox, the `task_check` job, the review scripts, the structural
  check, and the launch environment of SR-3).
- Landing and integration: 23 to 45 (git gateway, rescue refs, landing state machine, bundles and
  bisect, resolver and fix attempts, feature state machine, fetch, push and rebase, the operator
  commands and the status).
- Tests of the gate: 46 to 59 (pass wiring, fake agents, and the e2e tests of each gate row).
- Gate: 60.

Tickets that change the same file run one after the other through their blockers:

- Ticket 01 makes each new module file of `task_execution/`, `worker_profiles/` and
  `worktree_custody/`, and the later ticket fills it: for example
  `task_execution/application/verifier.rs`: 01 -> 19 -> 20 -> 21, and
  `task_execution/domain/repair.rs`: 01 -> 15 -> 34. `crates/horch-core/tests/arch_scan.rs` is
  owned by 01 only.
- `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`: 03 -> 18.
- `crates/horch-core/src/fleetd/fleet_runtime/application/holds.rs`: 04 -> 61.
- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` and `adapters/effects.rs`:
  46 -> 47.
- `crates/horch-core/src/harness/claude.rs` and `codex.rs`: 14 -> 22. `harness/launch.rs`:
  11 -> 22.
- `crates/horch-core/src/fleetd/integration/application/bundles.rs`: 29 -> 30. `commands.rs`:
  35 -> 40. `remote.rs`: 37 -> 38. `branches.rs`: 42 -> 44. `adapters/exec_push.rs`: 38 -> 39.
- `crates/horch-core/src/taskledger/tasks.rs`: 09 -> 42.
- `crates/horch/src/cmd/resultcmd.rs`: 05 -> 12. `branchcmd.rs`: 05 -> 35. `featurecmd.rs`:
  05 -> 40. `rescuecmd.rs`: 05 -> 43.

Each ticket with no blocker in this map (01, 02, 05, 17) is blocked by `T-302`, the gate of
Phase 3.

## Critical path

01 -> 07 -> 12 -> 13 -> 19 -> 25 -> 29 -> 30 -> 33 -> 42 -> 44 -> 47 -> 50 -> 60

## Open questions

Q-A, Q-B and Q-C are answers of the orchestrator. Q1 to Q22 are choices of the planner where the
design and the code of today do not agree, or where the design leaves a point open. Each ticket
follows them, and the orchestrator can change each one. Q2 and Q23 have no question.

- **Q1. Module files.** Ticket 01 makes each new file of the 3 contexts (1 file for each concern
  under `domain/`, `application/` and `adapters/`), so that tickets of 1 wave own different files.
  Ticket 01.
- **Q2.** None.
- **Q3. Where `task.yaml` and `START.md` are.** 1 copy for each attempt under
  `<fleet root>/tasks/<task id>/attempts/<attempt n>/`. Ticket 07.
- **Q4. How an attempt began.** `attempts.via` is `claim` or `push`. A backfilled attempt is
  `claim`. Tickets 02, 09.
- **Q5. The feature of a run.** The feature id is `F-<run seq>`, its name is the run slug, its
  branch is the run branch, and its primary is the base branch. A run with no run branch gets no
  feature row and keeps today's land path. Tickets 02, 36.
- **Q6. The time keys of the phase.** The design names them with no value. Choice:
  `fetch_period_s` = 60, `push_failure_limit` = 3, `landing_ready_limit_s` = 1800,
  `landing_repair_limit_s` = 3600, `landing_job_margin_s` = 600, `bundle_passed_limit_s` = 600.
  Ticket 03.
- **Q7. The size keys of the phase.** `result_max_bytes` = 65536, `resolver_max_lines` = 100,
  `resolver_max_files` = 2. Ticket 03.
- **Q8. The first line in the pane.** `Read <path>/inbox/START.md and continue.` (the doorbell has
  the path). Ticket 09.
- **Q9. Questions of a worker.** No new question command. The QUESTION path writes the question
  file. Ticket 16.
- **Q10. The reader of the question file.** It uses the SR-6 reader of `result.json`. Ticket 13.
- **Q11. Findings of the complexity sweep.** The outcome is `rejected`, and the list is in
  `verdict.json` and in `START.md` of the next attempt. Ticket 20.
- **Q12. The outputs of a judgment kind.** The files of `Own` that the commits changed. A `.md`
  file needs a `## ` heading, and a `.json` file must parse. Ticket 21.
- **Q13. SR-3.** No agent pushes. A pre-push hook comes through the `GIT_CONFIG_*` environment and
  the deny rules of the profile. The `-c core.hooksPath=/dev/null` of horch's own git calls wins.
  Ticket 22.
- **Q14. I8 for a squash with a conflict resolution.** The rule compares by content. Ticket 23.
- **Q15. The candidate of a bundle.** 1 merge commit for each member through
  `git merge-tree --write-tree` and `git commit-tree` (no worktree). The commits of each task are
  kept, and the history is not linear. Tickets 23, 27.
- **Q16. Branch rescue refs.** `refs/horch/rescue/branch/<name>/<n>`. Ticket 24.
- **Q17. Where the rescue class is read.** The rescue classes, the list of fleet branches and the
  stash names are read in the `worktree_list` job, not in the pass. Tickets 24, 44.
- **Q18. The gate tree.** 1 tree for each feature at `<project>/.worktrees/gate-<feature id>`, a
  custody row with no task and owner fleetd. The rebase also runs there. Tickets 28, 41.
- **Q19. How the gates run.** Through the launcher, with no worker sandbox. Ticket 28.
- **Q20. Ideas before Phase 6.** The stand-in for an idea: each task of the run is done or
  cancelled. Ticket 36.
- **Q21. When fleetd pushes.** When the feature tip is not `seen_remote_tip` and the feature is
  ready or its PR is open, and after a rebase. Ticket 38.
- **Q22. `gh` calls.** They run in the push job. Ticket 39.
- **Q23.** None.
- **Q-A. The check sandbox (answer of the orchestrator, option 1).** The `task_check` job runs only
  in the sandbox of the worker (SR-4). Reuse the sandbox concern of Phase P and do not build a
  second one: extend it with a list of writable paths. The sandbox host is in
  `crates/horch-core/src/harness/claude.rs` (`sandbox_host_problem`, Phase P Q5). Ticket 17 reuses
  it, or moves the host facts that a check needs into the platform concern. `NotSupported` means no dispatch of that persona, 1 needs-you item
  for each persona, and a clear line in `horch status`, never a silent run with the rights of
  fleetd. The Linux CI job gets `bubblewrap`, else the Linux tests assert `NotSupported` and the
  gate file names them as pending. Tickets 17, 18, 45, 51, 60.
- **Q-B. The new time limits (answer of the orchestrator).** `job_limit_s.fast_gate` = 1200,
  `full_gate` = 3600, `trial_merge` = 180, `bundle_build` = 600, `rebase` = 600, and
  `rescue_unique_budget_mb` = 5120. Each is a starting value, listed for the operator. Ticket 03.
- **Q-C. A project with no gates (answer of the orchestrator).** A project with no `[gates]` gets
  the dispatch hold and 1 needs-you item, and no task is lost. The gates of this repository are
  `fast = "cargo test --workspace --locked --lib --bins"` and
  `full = "cargo test --workspace --locked"`, a choice that the operator can change. Tickets 04,
  59.

Notes: fleetd's pass dispatches each ready task (the state machine of the design, Phase 1 Q16).
Continuous rebase is in Phase 4, because I10 needs a forced update (ticket 41).

## Coverage

Each mechanism of the row Phase 4 in "What each gate needs" and in "New work for each phase" of the
design, each mechanism of item 6 of the Migration plan, each rule of the gate line, and each
required item of the phase, with the ticket that gives it.

| Mechanism or rule | Ticket |
|---|---|
| The module files of the phase | 01 |
| The features table, the branches to remote_tips tables, and the release backfill | 02 |
| The task branch, the bundle and the feature as entities of the 1 door | 06 |
| The keys of `fleet.toml`; each job kind without a limit gets its key in `job_limit_s` (required item 5) | 03 |
| The 2 gates: `[gates]` with `fast` and `full` in the project `fleet.toml` (required item 2) | 04, 28 |
| The 4 new commands in the CLI | 05 |
| `task.yaml` and `START.md`; SR-2 | 07 |
| `horch spawn` binds the task before the pane exists; dispatch by fleetd | 08, 09, 10 |
| `CLAIM_TASK` goes; FR-5 | 09 |
| The briefing | 11 |
| `horch result` | 12 |
| The ingest of `result.json`, the `ingested` table and the SR-6 checks | 13 |
| An asked question or a quota stop is not an attempt | 13, 15 |
| The Stop hooks of Claude Code, Codex and Antigravity | 14 |
| The repair pass | 15 |
| The question file | 13, 16 |
| The check sandbox of the worker; SR-4 (required item 3) | 17, 18, 51 |
| The verifier runs the check as a `task_check` job | 19 |
| The review scripts carry over unchanged (V-14); land is not a verifier step | 20, 25 |
| The structural check of a judgment kind | 21 |
| SR-3: no agent pushes | 22, 58 |
| The git gateway; I8 and I9 | 23, 31 |
| The 2 rescue ref classes; I10 | 24, 41 |
| I2 | 26 |
| The bundle build job | 27 |
| The gate tree and the gate jobs | 28 |
| The landing state machine | 25 |
| Bundles | 29 |
| The bisect of a bundle | 30 |
| A bundle lands (compare-and-set of the ref) | 31 |
| The deletion of candidate refs | 32 |
| Resolver and fix attempts with their lease | 33 |
| `repair_attempts` | 34 |
| `horch branch abandon` | 35 |
| The feature state machine, 1 feature for each run | 36 |
| The fetch job | 37 |
| The push job | 38 |
| The `gh` calls of the pull request | 39 |
| `horch feature abandon` | 40 |
| The rebase job (continuous rebase) | 41 |
| A rebase that conflicts opens 1 resolver task | 42 |
| `horch rescue drop` | 43 |
| I11 and I12: orphan branches and the guard of branch deletion | 44 |
| `horch status` of the phase | 45 |
| The wiring of the pass | 46, 47 |
| The fake agents; the FM-2 test records at process entry that the task is bound and `task.yaml` exists (required item 1) | 48, 49 |
| Gate tests: FM-2, FM-6 | 49 |
| Gate test: the bundle test of Phase 0 | 50 |
| Gate test: the part test of FM-9 | 51 |
| Gate tests: I8, I9, I10, I11, I12 | 52 |
| Gate tests: FM-36, FM-37 | 29, 31, 53 |
| Gate test: FM-38 | 33, 34, 54 |
| Gate test: FM-43 | 38, 55 |
| Gate test: FM-40 | 24, 56 |
| Gate tests: PL-4, PL-5 | 57 |
| Gate tests: SR-2, SR-3, FR-5 (e2e) | 58 |
| The release row of Phase 4 (a run in flight) | 59 |
| The gate file | 60 |
| The health guards compare numbers of the same scope, and a "not supported" fact gives no hold (required item 4) | 61 |
| The jobs, the launcher, the fleet clock and the `needs_you` table | Phase 1 (no ticket) |
| Leases and questions | Phase 2 (no ticket) |
| Worktree custody and the verified rescue job | Phase 3 (no ticket) |
