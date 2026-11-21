# fleetd: a supervised daemon that runs the fleet

This pull request replaces the LLM-driven control of a horch fleet with `horch fleetd`, a
supervised daemon that owns the task ledger. fleetd binds each task to a worker before the agent
starts (push dispatch), holds the task with a lease that renews only on real activity, records
each worktree and branch in the ledger before it exists, moves a task to done only when its checks
pass on disk, and lands task branches on a feature branch through a landing state machine. It
replaces the lazily started watchdog (`horch ledger watch`), the claim path (`horch ledger claim`
as the first step of a worker), the orchestrator conversation that created tasks, and the finish
steps that a worker started itself. The design is `docs/specs/fleetd/design.md` (Summary and
Migration plan).

## Status

- Draft. Do not merge before the operator decides the items of "Decisions for the operator".
- This text describes `fleet/fleetd-finish` at `99bf2a9f2c6e66e1fa0ef206a12c04e709ea6cdd`
  (`git rev-parse fleet/fleetd-finish`, 2026-11-21).
- Size against `origin/main` (`5863c698`, Release v0.5.2): 1242 commits
  (`git rev-list --count origin/main..fleet/fleetd-finish`), 1063 of them on the first-parent
  line. `git diff --stat origin/main...fleet/fleetd-finish`: 1920 files changed, 608439
  insertions, 23445 deletions. In `crates/`: 865 files, 636 added, 223 modified, 2 deleted, 4
  renamed.
- `origin/main` is not merged yet. The merge base is `cb9655c6`. `origin/main` has 13 commits
  after it. The trial merge (`merge-main-plan.md`, `merge-main-proof.md`) covers 10 of them, to
  Release v0.5.1 (`e5170752`). The 3 newer commits are not in the trial: `c28e6dd6` (#364,
  headless Chrome code-sign clones on macOS), `50e63b2a` (#363, herdr-core `hc` and the Herdr
  desktop app as a second pane host, with new fakes `fake-hc` and `fake-herdr-app` in
  `crates/horch-e2e`) and `5863c698` (Release v0.5.2).
- The run PR #362 is `fleet/fleetd-finish` -> `fleet/fleetd-build`. The pull request to `main`
  does not exist yet (task T-226, E5).

Open tasks of run `fleetd-finish` (`horch ledger status --run fleetd-finish`, 2026-11-21: 329
tasks, 275 done, 2 running, 1 landing, 13 queued, 38 cancelled):

1. T-94 (queued): verify the expected-failures rows and audit `expected_failures_list_gates_the_run`.
2. T-221 (queued): E1, merge `origin/main` and pass the full workspace check.
3. T-222 (queued): R8, the end review of the change of the run.
4. T-223 (queued): E2, the Phase 4 gate file `fleetd-p4.md`.
5. T-224 (queued): E3, the Phase 5 gate file `fleetd-p5.md`.
6. T-225 (queued): E4, the Phase 6 gate file `fleetd-p6.md`.
7. T-226 (queued): E5, open the draft pull request to `main` and record the CI of each OS.
8. T-227 (queued): F6, the Phase 7 gate and the final gate.
9. T-228 (queued): F7, the release handoff for the operator.
10. T-286 (landing): the worktree states `landing` and `landed`, so the tree of landed work is removed at the land.
11. T-289 (queued): fleetd never removes a released gate tree.
12. T-290 (queued): CI FIX-24, the 4 restore tests of `p3_worktrees_cli` wait for a rescue that does not come.
13. T-310 (running): CI run 4 (run 44861461538, in progress), report `ci-run-4.md`.
14. T-319 (queued): Linux, each stop of a job ends its sandbox (worktree jobs, and a KILL of the job group with a gate).
15. T-327 (queued): `arch_scan` is red: 14 SQL lines of 3 landed tasks read a table of another context.
16. T-329 (running): this file.

## What changed

Paths below are relative to `crates/horch-core/src/fleetd/` unless they start with `crates/`.
fleetd has 6 bounded contexts, each with `domain/`, `application/` and `adapters/`:
`fleet_runtime`, `task_execution`, `worktree_custody`, `integration`, `ideas` and
`worker_profiles`. `git diff --stat origin/main...fleet/fleetd-finish -- crates/horch-core/src/fleetd`:
226 files, 82612 insertions. The requirement ids (FR, RR, SR, OR, PL) and failure modes (FM) are
those of `design.md`, section Requirements and Failure modes and effects.

### The daemon and its pass loop

- Old: `taskledger_watch.rs` was started lazily by any horch command, had no service unit, and
  sent one-shot wakes to the orchestrator as typed mail (design, "Where multi-herdr stands today").
- New: `horch fleetd` runs under the service manager of the OS (launchd, a systemd user unit, a
  Windows service). Each pass compares the ledger with processes, herdr panes and files, writes
  its changes in 1 transaction, and runs its effects after the commit. 1 file lock and the
  generation number of the `fleetd` row allow 1 fleetd for each ledger. `horch fleetd check`
  restarts a hung or older fleetd. Long work runs as a job (`jobs` table), so a pass never waits
  on git or a check. Each pass has a row budget (`pass_cursors`) and a time budget, and each fact
  scan has a cursor. 1 fleet clock drives each timer. Health guards (disk, load, memory,
  processes, open files) and pacing pause new dispatch. They are not a cap. fleetd stops each
  recorded old watchdog by PID and start time.
- Modules: `fleet_runtime/application/pass.rs`, `fleet_runtime/domain/{clock,budget,heartbeat,generation,health,jobs}.rs`,
  `fleet_runtime/adapters/{old_watchdog,scans,cursors_store,jobs_store}.rs`, `crates/horch/src/cmd/fleetd.rs`,
  `crates/horch/src/cmd/jobcmd.rs`.
- Rules: RR-4, RR-7, OR-2, OR-4, OR-5, OR-6, PL-1, PL-3, FM-4, FM-7, FM-11, FM-12, FM-22, FM-34,
  FM-44 (Phase 1), A23, A25, A32, A33.

### Push dispatch, leases and attempts

- Old: the first prompt of a worker was "Run horch ledger claim". `take_task` was a permanent
  claim with no expiry. A wait counted as progress for 24 h to 7 days.
- New: fleetd binds a task to an attempt and writes `task.yaml` and `START.md` before the agent
  starts. A lease renews only on process heartbeat and real activity. The stall ladder ends a
  hung attempt. A usage limit is not a stall: the task restarts on its fallback persona or waits
  in `waiting_quota`, with no attempt burned. `horch task requeue` and `horch task cancel` exist.
  A cancel stops a live attempt before the task is `cancelled`. Exec mode is the default for
  workers.
- Modules: `task_execution/domain/{dispatch,lease,stall,quota,retry,cancel,task_files}.rs`,
  `task_execution/application/{dispatch,launch,lease,stall,quota,retry,cancel}.rs`,
  `crates/horch/src/cmd/taskcmd.rs`.
- Rules: RR-1, RR-2, RR-6, RR-11, FM-3, FM-5, FM-14 (Phase 2); FR-5, FM-2 (Phase 4); FR-6, FR-8,
  FR-9 (Phase 7); D2, D7.

### `result.json`, the verifier, and the check and gate jobs

- Old: a worker reported done with `horch ledger done`, and no hook enforced it. The worker started
  the finish steps itself.
- New: a Stop hook (Claude, Codex, Antigravity) refuses the end of a turn without `result.json`.
  `horch result` writes it, with a closed list of statuses (`completed`, `blocked`, `failed`).
  fleetd ingests worker files as untrusted input (SR-6: size, symlink and attempt id checks).
  Only the verifier moves a task to done: the check runs as a job in the check sandbox (SR-4),
  then today's review scripts for code, or a paired judge task for a judgment kind (FR-4, RR-3).
  Each project declares a fast gate and a full gate.
- Modules: `task_execution/application/{ingest,verifier,review,judge_pair}.rs`,
  `task_execution/domain/{result,verification,structural}.rs`, `worker_profiles/domain/stop_gate.rs`,
  `worker_profiles/adapters/check_sandbox.rs`, `crates/horch/src/cmd/resultcmd.rs`.
- Rules: SR-2, SR-3, SR-4, FM-6, FM-9 (Phases 4 and 6), D4, D6, D15.

### Worktree custody

- Old: workers created their own worktrees. A row was written only after the creation finished,
  and nothing compared the table with `git worktree list`.
- New: fleetd reserves each worktree row before `git worktree add` runs, and runs
  `worktree_add`, `worktree_remove` and `worktree_list` as jobs. A reconciler adopts a tree on disk
  with no row (RR-10). Before a removal, a rescue job commits the tree to a rescue ref and reads it
  back (RR-9). The refusals of today's drop, cleanup and triage are kept (RR-13 to RR-17). `horch
  worktrees` lists each tree with state, task, owner, age and size. `horch worktree restore`
  recreates a tree from its ref. A disk budget counts the space of each tree.
- Modules: `worktree_custody/domain/{worktree,removal,rescue,rescue_class,disk}.rs`,
  `worktree_custody/application/{create,reconcile,remove,rescue,rescue_refs,disk,gate_tree}.rs`,
  `crates/horch/src/cmd/rescuecmd.rs`.
- Rules: FR-11, RR-9, RR-10, RR-13 to RR-18, FM-13, FM-25 to FM-28 (Phase 3), D10, A32.
- Not done: the states `landing` and `landed` (T-286, landing) and the removal of a released gate
  tree (T-289).

### Landing, bundles and the feature branch

- Old: a worker landed its own worktree with `horch worktree land`.
- New: each branch is a `branches` row before it exists, with a landing state, 1 owner and a time
  limit. fleetd lands ready task branches in bundles behind the fast gate, gives a failed branch a
  resolver or fix attempt (`repair_attempts`), runs `git fetch` and `git push` as jobs, and checks
  the no-loss invariants I8 to I13 against git on each pass. Trial merges find conflicts between
  features early, and a conflict stacks the younger feature on the older one. Each feature
  reaches the primary branch through 1 pull request. `horch feature abandon`, `horch branch
  abandon` and `horch rescue drop` exist.
- Modules: `integration/domain/{bundle,feature,task_branch,trial_merge,stack,invariants,bisector}.rs`,
  `integration/application/{landing,bundles,branches,remote,trial_merges,stacks,features}.rs`,
  `integration/adapters/exec_{bundle,fetch,gate,push,rebase,trial_merge}.rs`,
  `crates/horch/src/cmd/{featurecmd,branchcmd}.rs`.
- Rules: SR-3 (no agent push), FM-36, FM-37, FM-38, FM-40, FM-43, I8 to I12 (Phase 4); I13, FM-39,
  FM-41, FM-42 (Phase 6); D5, D10, D11, A27.

### Ideas, sub-orchestrators, questions and answers

- Old: 1 orchestrator conversation received ideas, worker mail and watchdog wakes, and created
  tasks with `horch ledger add-map`.
- New: `horch idea add` records each idea as 1 file and 1 `captured` row (FR-1). A deterministic
  dispatcher moves each idea through the idea state machine and keeps 1 sub-orchestrator
  (`idea-lead`) for each feature (FR-2). A plan commits only after schema, DAG, coverage, gate,
  judge, profile and model-guide checks (FR-3). `--direct` is the fast lane for a ticket you
  wrote (D3). A worker question goes to the sub-orchestrator first, and only product direction,
  scope or a trade-off goes to the operator (RR-12, D13). An answer resumes the CLI session of
  the asked attempt when that attempt has a session id, else a fresh attempt gets the question,
  the answer and the committed work in `START.md` (T-300). The needs-you list holds each item
  that waits on the operator.
- Modules: `ideas/domain/{idea,plan_validator,plan_rules_*,questions,answer,approval,needs_you}.rs`,
  `ideas/application/{dispatcher,sub_orchestrators,intake,direct,approval,question_flow,answers,replan,rollup}.rs`,
  `crates/horch/src/cmd/idea/`, `crates/horch/src/cmd/askcmd.rs`.
- Rules: FR-1, FR-2, FR-3, FR-4, FR-10, FR-12, RR-3, RR-12, OR-3, FM-1, FM-8, FM-10, FM-16, FM-21,
  FM-29 to FM-33, FM-35 (Phase 6), D3, D11, D12, D13, A28.

### The operator boundary

- Old: an agent in the orchestrator seat had operator rights. Any horch command wrote the ledger.
- New: from Phase 5, only fleetd writes the ledger (RR-5). Each other command opens it read-only
  and sends a request on the command channel (a Unix socket, a named pipe on Windows) or as a
  request file. A request has an idempotency key (RR-8) and a class. A request of class operator
  needs the operator token, an owner-only 0600 file under an owner-only 0700 run folder. The
  request catalog is closed: 15 retired kinds print the command that replaces them
  (`fleet_runtime/domain/requests.rs`, `RETIRED`). The worker sandbox denies the run folder and
  the ledger files. `fleet_process` decides who is a fleet process from OS process identity,
  ancestry, group and session, so no process of a fleet attempt can approve its own idea (FR-10).
  The worker wrapper sends no operator request and does not read the token (T-5). The wrapper
  waits until fleetd recorded the process of its attempt before the agent starts, and fails
  closed at a time limit (SR-8, T-172). Profile validation refuses a profile on an OS that cannot
  sandbox it.
- Modules: `fleet_runtime/domain/{requests,authorize,operator}.rs`,
  `fleet_runtime/adapters/{channel_server,channel_client,request_spool,request_wire,token_file,peer_process,protected_files}.rs`,
  `fleet_runtime/application/handlers_*.rs`, `worker_profiles/domain/operator_boundary.rs`,
  `crates/horch-core/src/execution/lifecycle.rs` (the wrapper), `crates/horch/src/cmd/{requestcmd,operator_request}.rs`.
- Rules: FR-7, RR-5, RR-8, SR-5, SR-6, SR-8, FM-15, FM-17, FM-23, FM-24 (Phase 5); FR-10 (Phase 6);
  SR-7 and the check "Operator boundary" (Phase 7); A24, A29, A35, A37.

### Sandboxes and the platform crate `horch-platform`

- Old: OS-specific code sat in the production code of each crate.
- New: the crate `crates/horch-platform` (99 files, 11913 insertions) holds each OS concern with
  1 module per OS (`mac/`, `linux/`, `windows/`, shared `unix/`): process facts, process groups,
  signals, the service manager, the channel, locks, file watch, the machine probe, paths and the
  sandbox. Core code has no `cfg(target_os)` (PL-2, guarded by `crates/horch-core/tests/arch_scan.rs`).
  Phase P moved the existing sites with no change of behavior (PL-6, `scripts/pl6/`).
  - macOS: Seatbelt (`sandbox-exec`). The check sandbox denies remote IP and allows `localhost:*`.
    Seatbelt treats the LAN address of the host as local (R8-F2, open).
  - Linux: bubblewrap. A job gets a PID namespace (`--unshare-pid`, `--proc /proc`) with `setsid`
    as the first program inside, and no `--new-session`, so a KILL of the group of `bwrap` ends
    each sandbox process (T-309). A check with no network gets `--unshare-net`. Claude Code on
    Linux also needs `socat` (`crates/horch-core/src/harness/claude.rs:1639`).
  - Windows: the sandbox concern returns `NotSupported` (`crates/horch-platform/src/windows/sandbox.rs`).
    The channel is a named pipe. Several concerns give `NotSupported` (for example process group
    members), and the design rules that need them rest on unit tests there.
- Rules: PL-1 to PL-6, D1, D16.

### The ledger schema

The schema goes from version 7 (main before the merge base) to the fleetd steps. On the run
branch now they are 8 to 16 (`crates/horch-core/src/taskledger/schema.rs`). After the merge of
main they are 9 to 17, because main's version 8 is the `comment-sweep` step (`merge-main-plan.md`
row 8, `merge-main-proof.md` "For the operator" item 1):

| After the merge | Name in code | What it adds |
| --- | --- | --- |
| 8 | `COMMENT_SWEEP_VERSION` | main: `comment-sweep` in the `CHECK` of `task_steps` |
| 9 | `PHASE_1_VERSION` | `fleetd`, `attempts`, `jobs`, `pass_cursors`, `needs_you`, the Phase 1 event kinds, and an attempt row for each live worker |
| 10 | `PHASE_2_VERSION` | `leases`, `questions`, the new task states and task columns, and a lease for each held task |
| 11 | `PHASE_3_VERSION` | the custody columns of `worktrees` (`state`, `owner_attempt`, `rescue_ref`, `head_sha`, `dirty`, `seen_at`), and their backfill |
| 12 | `EVENT_KINDS_VERSION` | `events` built again with the event kinds of this binary |
| 13 | `WORKTREE_PATHS_VERSION` | `worktrees.path` unique only among rows that are not `removed` |
| 14 | `PHASE_4_VERSION` | `features`, `branches`, `bundles`, `bundle_members`, `gate_runs`, `remote_tips`, `ingested`, `attempts.via`, and the backfill |
| 15 | `PHASE_5_VERSION` | `requests`, `polite_stops`, and `jobs` built again with the `triage_apply` kind |
| 16 | `PHASE_6_VERSION` | `ideas`, `requirements`, `task_covers`, `trial_merges`, the rule that each task has an idea, and the idea of each run in flight |
| 17 | `PHASE_7_VERSION` | `events` built again with the event words of the health of a profile |

Each table name in this table comes from the migration steps in `migrate_unchecked` of `schema.rs`. A newer schema is refused by an older binary (`newer_schema_9_is_refused`, `fleetd-p1.md`).

### The CLI

New top-level commands (`crates/horch/src/main.rs`): `horch fleetd` (with `check`), `horch idea`
(`add`, `--direct`, `--to`, `show`, `note`, `approve`, `revise`, `amend`, `park`, `cancel`),
`horch ideas`, `horch task` (`requeue`, `cancel`), `horch result` (`--check` tests the Stop
hooks), `horch ask`, `horch feature abandon`, `horch branch abandon`, `horch rescue drop`,
`horch request`, `horch job` (`requeue`, `run`), `horch docs sync`, `horch worktrees`.

Retired commands. Each prints its replacement (`RETIRED` in `fleet_runtime/domain/requests.rs`):

| Retired | Replacement |
| --- | --- |
| `horch ledger run new` | `horch idea add "<idea>"` |
| `horch ledger run close` | merge the pull request of the feature; to drop it, `horch feature abandon` |
| `horch ledger run gate` | fleetd runs the full gate when the task branches of a feature are landed |
| `horch ledger add`, `horch ledger task` | `horch idea add --direct <ticket file>` |
| `horch ledger add-map` | `horch idea add`; `horch ledger add-map --dry-run` still checks a map |
| `horch ledger plan-workers` | fleetd dispatches each ready task |
| `horch spawn` | fleetd launches each attempt; plan the task with an interactive persona to pair |
| `horch ledger worker new`, `horch ledger worker pid`, the spawn adoption | fleetd records each attempt at its launch |
| `horch ledger reap` | fleetd stops an orphan worker; `horch ledger stop <worker> "<reason>"` stops 1 |
| `horch ledger unblock` | `horch task requeue <task>` |
| the orchestrator claim | `horch fleet start` starts fleetd and herdr with no orchestrator |
| `horch fleet stop` | no orchestrator runs; `horch ledger stop` stops 1 worker |

`horch ledger watch` only points at fleetd (design, Migration plan, Phase 1 row). `horch ledger
finish` only writes `result.json` (`merge-main-plan.md` row 18).

### The test harness and the e2e crate

- `crates/horch-e2e` (118 files changed, 42629 insertions): 87 test files against 37 on main.
  The suites `p0_*` to `p7_*` hold the gate tests of each phase. New harness modules:
  `fleetd.rs`, `bind.rs`, `exec.rs`, `expected.rs`, `fr10_agent.rs`, `idea_agent.rs`,
  `ledger_seed.rs`, `probe.rs`, `agent.rs`. The fakes (`fake-herdr`, `fake-claude`, `fake-codex`,
  `fake-antigravity` and others in `src/bin/`) stand in for herdr and each agent CLI.
- `crates/horch-e2e/expected-failures/`: 1 file for each test binary. Each entry has a test name,
  the phase that makes it green, and the reason. Phase 0 wrote each red test there.
  `expected_failures` fails on a stale entry.
- `scripts/dev-sandbox.sh` runs a command with its own `HOME`, state, ledger, Claude and Codex
  homes and `FLEET_ROOT`, so a development build never touches the live ledger of the operator.
- `scripts/pl6/` compares the result of each test before and after Phase P. `scripts/live/p7-real-conformance.sh`
  runs the conformance of a real CLI in a logged-in shell.
- The crate `herdr-docs-sync` is renamed `agent-docs-sync` and covers each agent CLI (Phase 7).

### CI changes

`.github/workflows/ci.yml`: on Linux the test job installs `bubblewrap` and `socat` and sets
`kernel.apparmor_restrict_unprivileged_userns=0`, because Ubuntu 24.04 lets no unconfined program
make a user namespace. `cargo test` runs with `--no-fail-fast`. A new job `test-windows` runs
`cargo test --workspace --locked --no-fail-fast` with `continue-on-error: true`.

## Behavior changes that a user sees

| Before | After | Why |
| --- | --- | --- |
| A lazily started watchdog with no service unit | `horch fleetd` under launchd, systemd or a Windows service | A missed event is caught on the next pass, and a hung daemon is restarted (FM-7, FM-12) |
| A worker runs `horch ledger claim` | fleetd binds the task before the agent starts | Nothing to find, so no `no_task` worker (FR-5, FM-2) |
| A claim with no expiry | A lease that renews on real activity | An idle worker loses its task in minutes (RR-1, FM-3) |
| `horch ledger done` reports done | `horch result` writes `result.json`; the verifier decides done | Done is a fact on disk (FM-6, FM-9) |
| A worker lands its own worktree | fleetd lands branches in bundles on the feature branch | No branch is lost (I8 to I13) |
| `horch ledger run new`, `add`, `add-map`, `task` | `horch idea add`, `horch idea add --direct` | Each task belongs to an approved idea (FR-12) |
| `horch spawn`, `horch ledger worker new`, `worker pid`, `reap`, `unblock`, `plan-workers` | fleetd launches, records and stops attempts; `horch task requeue` | Push dispatch (FR-5) |
| `horch fleet` starts an orchestrator pane | `horch fleet start` starts fleetd and herdr, and fleetd starts 1 sub-orchestrator for each feature | No agent has operator rights (FR-10, SR-8) |
| `horch fleet stop` | Retired | No orchestrator runs |
| Any horch command writes the ledger | Only fleetd writes; others send a request | 1 writer (RR-5) |
| Operator commands work from any pane | They need the operator token; a worker sandbox denies it | SR-8 |
| A task check has open network | Loopback and Unix sockets only (`Network::Local`) | SR-3, SR-4 (T-17). A check that downloads a dependency fails |
| The `horch fleet` worktree steward starts | It does not start (3 `i308_fleet_steward` expected failures) | Phase 6 retired `horch ledger task` (decisions log, T-87) |
| Linux: no extra host tools | `bubblewrap`, `socat` and util-linux `setsid` are required | Job and worker sandboxes (T-267, T-309) |
| Context window from the seat | 256000 tokens for each seat | FR-9 |
| Exec or interactive by seat | Exec is the default; a persona can be interactive | D7 |

## Faults found and fixed in the finish run

Product faults (not test faults) that run `fleetd-finish` found. Source: the decisions log
(`ai_docs/plans/fleetd-finish/decisions.md`, entries "PRODUCT" and "SECURITY") and the board.

1. T-163 (done): FR-10, an agent could approve an idea while its attempt was launching with no recorded pid.
2. T-172 (done): SR-8, agent code could run before fleetd recorded the process of its attempt. The wrapper now waits, and fails closed. The same task fixed a git writer that starved behind a fetch (FIFO for each repository).
3. T-156 (done): the question text of an idea-lead accepted line separators and bidi characters, and the status line did not escape it.
4. T-195 (done): an attempt that timed out in `launching` left its task `running` with no lease and no attempt (lost work).
5. T-246 (done): `write_idea_effects` wrote `plan.yaml` with no judge ticket, so a `--direct` judgment idea stayed in `ticketing` and fleetd relaunched its sub-orchestrator.
6. T-267 (done): on Linux fleetd found no `bwrap` for the check sandbox and held each persona that is not codex (`no_check_sandbox`), also in production.
7. T-281 (done): the pass called herdr with no time limit, so a herdr that did not answer held the pass.
8. T-276 (done): `kill_group_on_termination` kept 1 global pointer, so a SIGTERM could leave a check group alive.
9. T-287 (done): RR-13, a drop of a worktree whose HEAD moved during the checks was accepted. It also fixed the runaway job host chain of a unit test.
10. T-288 (done): RR-14, `horch ledger done --unlanded` did not park the task. Fix: the optional key `unlanded` of `result.json`.
11. T-292 (done): the heartbeat gap of a scan of 10000 rows was 34 to 52 s against a limit of 30 s. Each per-row step is now paged by the pass budget.
12. T-294 (done): `lost_tasks` opened I1 for a task whose cancel waits. A cancel of a running task now completes.
13. T-254 (done): a stop of fleetd during a launch left an attempt that nothing relaunched before the start timeout. `worker_launch_failed` now ends the attempt.
14. T-307 (done): a launch whose host call failed waited for the start timeout before the retry.
15. T-300 (done): RR-12, an answered question now resumes the CLI session of the worker (R8-F1).
16. T-302 (done): fleetd accepted the dead request kind `spawn_adopt` (R8-F3).
17. T-309 (done): on Linux `bwrap --new-session` put the sandboxed command in a new process group, so each check, gate and worktree job leaked its processes.
18. T-320 (done): the operator-boundary probe counted "file not found" as "refused", so on Linux it gave a false Pass for a rule that allows the token read.
19. T-313 (done): the dispatch step ran SQL for each ready task above the pass budget.
20. T-209, T-236 (done): `horch ledger status` wrote on its read-only connection. T-236 found the cause in `workers::waiting`. T-209 is labeled "not verified fixed" in the decisions log.
21. T-126 (done, ticket 147): a worker that fleetd launched in exec mode got no skills briefing.

## How it was proved

"verified" names the evidence file. "not verified" means no evidence file shows a pass.

| Item | Result | Label |
| --- | --- | --- |
| Gate of Phase 0 | `cargo test --workspace --locked` at `f8c0b7a6`: 2866 pass, 0 fail, 11 ignored; each red test fails with its listed reason | verified, `gates/fleetd-p0.md` |
| Gate of Phase P | PL-2 green on macOS (18 of 18). PL-6 on Linux passes in Docker (2938 tests, no difference) with no CI run id. PL-6 on Windows pending CI | macOS verified, `gates/fleetd-pp.md`; Linux and Windows not verified |
| Gate of Phase 1 | Each row passes on macOS at `97236dda` (3206 ok, 0 failed). Linux in Docker: 11 failed, 10 by the container, 1 fixed in `3709bb4d` | macOS verified, `gates/fleetd-p1.md`; CI not verified |
| Gate of Phase 2 | Green on macOS at `9a2bf61d`. Each row passes on Linux in Docker | macOS verified, `gates/fleetd-p2.md`; CI not verified |
| Gate of Phase 3 | Green on macOS at `d84c9ddd`. Linux and Windows: not run | macOS verified, `gates/fleetd-p3.md`; Linux and Windows not verified |
| Gates of Phases 4, 5, 6, 7 and the final gate | Not written (T-223 to T-225, T-227) | not verified |
| Pre-gate of Phase 4, tip `895e3848` | Each rule row passes on macOS. Red: `arch_scan` `fitness_no_context_touches_foreign_tables` (14 lines, T-327) and 2 Phase 4 expected failures in `taskledger.txt` (T-286) | verified, `gates/pre-gate-p4.md` |
| Pre-gate of Phase 5, tip `895e3848` | Each rule row passes on macOS except the `arch_scan` guard. The Windows pipe test (REQUIRED 1) runs only in CI | verified, `gates/pre-gate-p5.md` |
| Pre-gate of Phase 6, tip `895e3848` | 306 passed, 1 failed (the `arch_scan` guard), 2 ignored (`nfr` measurements) | verified, `gates/pre-gate-p6.md` |
| End reviews of phase groups | R1 to R6 done (T-25 to T-30): `reviews/end-review-p0-pp-p1.md`, `-p2-p3.md`, `-p4.md`, `-p5.md`, `-p6.md`, `-p7.md` | files verified. The state of each finding is in `docs/tickets/fleetd-finish/FIX-INDEX-*.md` (not tracked in git) and is not checked in this file |
| End review A of the run, tip `08c6d4ff` | 0 Critical, 3 Important, 2 Minor. R8-F1 fixed (T-300). R8-F2 open: T-301 measured that no Seatbelt form limits a check to loopback, see `gates/macos-check-sandbox-lan.md`. R8-F3 fixed (T-302). R8-F4 and R8-F5 fixed (T-303) | verified, `reviews/end-review-a.md` and the decisions log |
| End review R8 of the final change | Queued (T-222) | not verified |
| CI0, run 44680906908, head `085427fb` | 1 of 6 jobs passed. macOS 4411 passed, 666 failed | verified, `gates/ci-baseline-2026-11.md` |
| CI1, run 44701297167, head `7fbdc0d3` | 1 of 6 jobs passed. macOS 4796 / 270, Linux 4691 / 375 (passed / failed). Windows: 0 tests ran | verified, `gates/ci-run-2.md` |
| CI2, run 44784709135, head `001e73c2` | 2 of 6 jobs passed (`docs`, `build-windows`). macOS 5002 / 188, Linux 4821 / 368. `release-lint` 2 errors. Windows: 1 compile error | verified, `gates/ci-run-3.md` |
| CI3, run 44861461538 | In progress (T-310) | not verified |
| Trial merge of `origin/main` to v0.5.1 | `1e782060` on local branch `keep/fleetd-finish-trial-merge-main`: 26 conflicts resolved. `horch-core` 3699 / 0, `horch` 614 / 0, `ui-prop-tester` 241 / 0, both clippy forms and `cargo fmt --all --check` exit 0 | verified, `gates/merge-main-proof.md` |
| Trial merge: rest | `cargo test --workspace --locked` as 1 command, the other crates, `horch-e2e` (only `p4_release`, `p7_release` ran), the 17 later run-branch commits, the CI of the trial | not verified, `gates/merge-main-proof.md` item 5 |

## Not proved, and known gaps

1. Windows. The sandbox concern is `NotSupported`, and `test-windows` has `continue-on-error`. No CI run shows Windows tests passing (CI1: 0 tests ran; CI2: 1 compile error). T-243 (the Windows test targets do not compile) is cancelled.
2. Real CLIs. Each e2e test uses a fake CLI. No run of `scripts/live/p7-real-conformance.sh` for real Claude and Codex is recorded (`end-review-a.md`, Operator decision 2; `end-review-p5.md` OD-3).
3. Antigravity. Its adapter is proved with a fake only. Real `agy` stays unhealthy by design and gets no dispatch (`end-review-a.md`, Operator decision 5).
4. The macOS check sandbox reaches the LAN address of the host (R8-F2, `gates/macos-check-sandbox-lan.md`). On Linux `--unshare-net` blocks it.
5. FR-7 and SR-8 as the literal design text: the tests prove a closed list of 9 typed lines, not 1 doorbell line, and an OS sandbox refusal leaves no `requests` row (`pre-gate-p5.md` Q1, Q2).
6. Linux: a KILL of the job group does not reach the gate group (T-319, queued). A fleet root under `/tmp` on Linux gives "Operator boundary" fail and an unhealthy profile (decisions log 05:41Z).
7. `arch_scan` `fitness_no_context_touches_foreign_tables` is red on the run branch (T-327).
8. Main's change S5 (a quota refusal is held no longer than its window) is not tested through a fleetd hold (`merge-main-proof.md`).
9. The 3 commits of `origin/main` after v0.5.1 (#363, #364, v0.5.2) are not in the trial merge. #363 adds a second pane host backend that fleetd code does not know.

Expected-failure entries that remain (`crates/horch-e2e/expected-failures/*.txt`, 8 entries):

| File | Test | Phase | Owner |
| --- | --- | --- | --- |
| `i308_fleet_steward.txt` | `a_fleet_with_an_abandoned_worktree_spawns_the_worktree_steward` | 7 | operator decision (T-87) |
| `i308_fleet_steward.txt` | `an_exhausted_claude_pool_starts_no_codex_steward` | 7 | operator decision (T-87) |
| `i308_fleet_steward.txt` | `the_no_change_steward_task_finishes_without_a_worktree` | 7 | operator decision (T-87) |
| `p3_refusals.txt` | `run_gate_folder_is_added_and_removed_through_the_jobs` | 7 | T-289 (the entry names T-199, cancelled) |
| `p3_unlanded.txt` | `rr15_landed_work_of_a_done_task_is_not_listed` | 3 | T-286 (the entry names T-207, cancelled) |
| `p3_worktrees.txt` | `fm27_only_landed_or_rescued_trees_are_removed` | 3 | T-286 (the entry names T-207, cancelled) |
| `taskledger.txt` | `the_whole_flow_on_the_fake_herdr` | 4 | T-286, then T-289 (decisions log 03:03Z) |
| `taskledger.txt` | `a_task_finishes_through_review_land_pr_and_cleanup` | 4 | T-286 |

Ignore marks (`git grep -n '#\[ignore' fleet/fleetd-finish -- crates`): 29 on the run branch, 14
on `origin/main`. 21 are child helpers that a named parent test runs. The others:

- `crates/horch-e2e/tests/p3_refusals.rs:425`, `:554`, `:677`, `:769`: "flaky until T-199". T-199 is cancelled. The work moved to T-286 and T-289.
- `crates/horch-core/tests/nfr.rs:433` (`nfr_02_tick_budget`), `:668` (`nfr_02_live_store_rss`), `crates/horch-core/tests/telemetry_collect.rs:834` (`project_rollups_timing`): measurements that need `HORCH_PERF_*`.
- `crates/horch-core/tests/context_prime.rs:1085`: live, needs the real prime agent in a herdr pane.

## Decisions for the operator

1. **comment-sweep under fleetd** (`merge-main-plan.md` Q1, `merge-main-proof.md` item 4). Main runs comment-sweep in each profile through the retired `horch ledger finish`. Options: (a) fleetd does not run it, and the docs say so; (b) port it as a fleetd verifier step that starts 1 `comment-remover`. Recommendation: (a) for the merge, and a ticket for (b). The trial fixes `05cdf1e9` and `14833c51` follow (a).
2. **Schema numbers** (`merge-main-plan.md` Q2). The fleetd steps move to 9 to 17 after main's version 8. Does a ledger file exist that a run-branch build migrated past version 7, other than a scratch `HORCH_LEDGER_DB` file? Recommendation: renumber. The decisions log (03:52Z) says no run-branch build was installed.
3. **Release and the CHANGELOG line** (`merge-main-plan.md` Q3, S9). The run adds no line under `## Unreleased`, so a merge to `main` publishes no release. Do you want a line for the fleetd release? Recommendation of the run: none given; the goal prompt says "Do not release".
4. **macOS check sandbox and the LAN address** (R8-F2, `gates/macos-check-sandbox-lan.md`). Options: (a) no IP on macOS checks, which makes 24 tests of this repo fail; (b) keep the behavior and change the contract and design text to "this host only"; (c) mark the macOS check sandbox unsupported. The review recommends the strict loopback contract.
5. **Network in a task check** (decisions log 12:55Z, T-17). A check has no off-host network, so a dependency download in a check fails. Recommendation: keep it (SR-3, SR-4).
6. **RR-12 attempt burn at a failed resume** (T-300 commit message, design lines 1003, 1236, 1237). A resumed CLI that exits with no result ends `no_result` and burns 1 attempt. The next attempt starts fresh after the backoff. Recommendation: keep.
7. **The 3 `i308_fleet_steward` expected failures** (decisions log 07:55Z, T-87). Restore the worktree-steward start under fleetd (new ticket), or retire it (design change). Recommendation: restore.
8. **Windows fleetd as a release target** (`end-review-p5.md` OD-2). Recommendation: implement ACL-based protected-file reads before Windows support is claimed, else document that the request-file fallback and sandboxed workers are not available there.
9. **FR-7 and SR-8 as literal design text** (`pre-gate-p5.md` Q1, Q2). Change the design to "a closed list of 9 typed lines" and "an OS refusal leaves no row", or require new code. Recommendation: change the design text (the run cannot edit `design.md`).
10. **The `unlanded` key of `result.json`** (decisions log 02:24Z, T-288). The design (lines 541 to 557) does not show it. Recommendation: add it to the design text.
11. **`ui-prop-tester` and the platform rules** (`merge-main-proof.md` item 3). `arch_scan` skips the crate for PL-2 (17 OS `cfg` sites) and SR-7 (5 `Command::new`). The guard `no_crate_of_horch_depends_on_ui_prop_tester` keeps the skip safe. Decide whether to port the crate to `horch-platform`.
12. **Unhealthy review profiles** (`end-review-a.md` Operator decision 1). A task whose primary profile is unhealthy is held, and its fallback is not used (`task_execution/domain/dispatch.rs:16`). Recommendation: a review task runs on a healthy cross-provider fallback at once.
13. **Real CLI release evidence** (`end-review-a.md` Operator decision 2, `end-review-p5.md` OD-3). Recommendation: run `scripts/live/p7-real-conformance.sh` for Claude and Codex in a logged-in shell before the release, and record the "Operator boundary" result.
14. **Antigravity release scope** (`end-review-a.md` Operator decision 5). Recommendation: keep `agy` out of the release claim until its real conformance is green.
15. **Design lines with old schema numbers** (`merge-main-proof.md` item 1). `design.md:1611` says "Schema v8 adds a `leases` table" (code: 10). `design.md:1615` says "Schema v9" for ideas (code: 16). `preservation-05-contracts-and-values.md:8`, `:9`, `:98` say "v8 leases, v9 ideas". Recommendation: change the text after the merge.
16. **Unlisted request kinds** (`end-review-p5.md` OD-1). `worker_progress`, `worker_done`, `run_adopt` and `question_answer` are not in the Phase 6 map. `spawn_adopt` is retired (T-302). Recommendation: add the 4 kinds to the map.
17. **When a gate tree is removed** (decisions log 22:40Z). The design has no rule for the end of a gate tree. T-289 implements a removal.
18. **The main checkout branch** (decisions log line 44). The main checkout is on a fleetd branch, so a new solo run starts from it. Recommendation: switch it to `main`.

## Release steps (not done by this pull request)

1. Decide each item of "Decisions for the operator" before the merge.
2. Run the real CLI conformance (item 13).
3. Add a line under `## Unreleased` in `CHANGELOG.md` only if the merge must publish a release. This pull request adds no CHANGELOG line, because the goal prompt forbids a release from the run, and a merge to `main` with a line publishes one at once (`merge-main-plan.md` S9).
4. After the release, `horch self-update` restarts a running fleetd before it replaces itself (OR-6, D9). A run in flight follows "What a release does to a run in flight" in the design: the release backfills the rows of each phase.
5. On each Linux host, install `bubblewrap`, `socat` and util-linux `setsid`.
6. Remove the Docker images that the run left: `t250-linux:local` (2.08 GB) and `ubuntu:24.04` (117 MB, owner not known) (decisions log 05:34Z).

## How to review

1. Read `docs/specs/fleetd/design.md`: Summary, Amendments, Requirements, Migration plan.
2. Read `crates/horch-platform/src/` (the OS layer), then `crates/horch-core/src/fleetd/` in this order: `fleet_runtime`, `task_execution`, `worktree_custody`, `integration`, `ideas`, `worker_profiles`.
3. Read `crates/horch-core/src/taskledger/schema.rs` (the migrations) and `crates/horch-core/src/execution/lifecycle.rs` (the wrapper).
4. Read `crates/horch/src/main.rs` and `crates/horch/src/cmd/` (the new and retired commands).
5. Read `crates/horch-e2e/tests/p0_*` to `p7_*` and `crates/horch-e2e/expected-failures/`.
6. Read the gate files in `docs/specs/fleetd/gates/` and the reviews in `docs/specs/fleetd/reviews/`.

The full check, each command through `scripts/dev-sandbox.sh`:

```sh
scripts/dev-sandbox.sh cargo fmt --all --check
scripts/dev-sandbox.sh cargo clippy --workspace --all-targets --locked
scripts/dev-sandbox.sh cargo build --workspace --bins --locked
scripts/dev-sandbox.sh cargo test --workspace --locked
```

Build the binaries before `cargo test`, or each `horch-e2e` test fails with "horch is not built".
Under a load average above 25, e2e tests can report a leaked process at teardown.
