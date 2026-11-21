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

## Read this first

- **What fleetd is.** `horch fleetd` is a daemon under the service manager of the OS. It is the only
  writer of the ledger. Each pass compares the ledger with processes, panes and files, and fixes
  the difference.
- **What changes for a user of `horch fleet`.**
  - `horch fleet start` starts fleetd and herdr. No orchestrator pane starts.
  - You give work with `horch idea add "<idea>"`, or `horch idea add --direct <ticket file>`.
  - fleetd binds each task to a worker before the agent starts. A worker ends with `horch result`.
    The verifier, not the worker, moves a task to done.
  - fleetd lands task branches on a feature branch. You merge 1 pull request for each feature.
  - A question that needs you shows in the needs-you list. An operator command needs the operator
    token.
  - A Linux host needs `bubblewrap`, `socat`, util-linux `setsid` and git 2.41 or newer.
- **What is removed.** The lazily started watchdog, the claim path of a worker, the orchestrator
  claim, `horch fleet stop`, and 15 request kinds. Each retired command prints its replacement. The
  table is in "The CLI".
- **What the operator must decide.** The file `docs/specs/fleetd/gates/operator-decisions.md`
  holds each open decision with its facts, its options and its recommendation. Task T-390 writes it
  now. It is not in tip `72b48f3e` (verified: the file is absent). Until it lands, use "Decisions
  for the operator" in this file.
- **How to test.** Build the binaries, then run the 4 commands of "How to review".

## Status

- Draft. Do not merge before the operator decides the items of "Decisions for the operator".
- This text describes `fleet/fleetd-finish` at `72b48f3eeccef7d501a41d38e8ced33728d8ce61`
  (`git rev-parse fleet/fleetd-finish`, 2026-11-21). The first draft described `99bf2a9f`. 92
  commits are between them (`git rev-list --count 99bf2a9f..72b48f3e`). A commit after `72b48f3e`
  is not described here.
- Size against `origin/main` (`5863c698`, Release v0.5.2): 1334 commits
  (`git rev-list --count origin/main..fleet/fleetd-finish`), 1155 of them on the first-parent
  line. `git diff --shortstat origin/main...fleet/fleetd-finish`: 1930 files changed, 614678
  insertions, 23504 deletions. In `crates/`: 867 files, 636 added, 225 modified, 2 deleted, 4
  renamed (`git diff --name-status --find-renames`).
- `origin/main` is not merged yet. The merge base is `cb9655c6`. `origin/main` has 13 commits after
  it. The trial merge covers all 13, to Release v0.5.2 (`5863c698`): `merge-main-proof.md` (to
  v0.5.1) and `merge-main-proof-2.md` (the 3 newer commits: #364, #363, v0.5.2). The trial is the
  local branch `keep/fleetd-finish-trial-merge-main` at `3fd534f7`. It is not pushed and has no CI
  run.
- The run PR #362 is `fleet/fleetd-finish` -> `fleet/fleetd-build`. The pull request to `main`
  does not exist yet (task T-226, E5).
- To fill at the end: the final tip, the CI links of run 5 and of the final gates, and the gate
  results. See "How it was proved".

Board of run `fleetd-finish` (`horch ledger status --run fleetd-finish`, 2026-11-21): 392 tasks.
331 are done, 41 cancelled, 10 queued, 9 running and 1 landing. The tasks that are not settled:

1. T-94 (queued): verify the expected-failures rows and audit `expected_failures_list_gates_the_run`.
2. T-221 (queued): E1, merge `origin/main` and pass the full workspace check. The trial merge exists, the real merge does not.
3. T-222 (queued): R8, the end review of the change of the run.
4. T-223, T-224, T-225 (queued): E2, E3 and E4, the gate files `fleetd-p4.md`, `fleetd-p5.md` and `fleetd-p6.md`.
5. T-226 (queued): E5, open the draft pull request to `main` and record the CI of each OS.
6. T-227 (queued): F6, the Phase 7 gate and the final gate.
7. T-228 (queued): F7, the release handoff for the operator.
8. T-289 (running): fleetd never removes a released gate tree.
9. T-369 (landing): 2 stop gaps of T-319 (the window before the handler of a check group, and a git in the fleetd process with no stop path).
10. T-376 (running): CI run 5 of the run branch.
11. T-378 (running): the early full gate on macOS.
12. T-384 (running): D244-F1, a profile whose first conformance is "not proved" gets dispatch.
13. T-385 (queued): D244-F2, the reconciler reads the owner of each active tree with SQL for each row.
14. T-386 (running): D244-F3, a resume of a session that is gone burns an attempt.
15. T-389 (running): check the git version in `horch doctor` and at the start of fleetd.
16. T-390 (running): the file of the open decisions of the operator.
17. T-391 (running): this file.
18. T-392 (running): the comment sweep of T-369.

## What changed

Paths below are relative to `crates/horch-core/src/fleetd/` unless they start with `crates/`.
fleetd has 6 bounded contexts, each with `domain/`, `application/` and `adapters/`:
`fleet_runtime`, `task_execution`, `worktree_custody`, `integration`, `ideas` and
`worker_profiles`. `git diff --shortstat origin/main...fleet/fleetd-finish -- crates/horch-core/src/fleetd`:
226 files, 84074 insertions. The requirement ids (FR, RR, SR, OR, PL) and failure modes (FM) are
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
- The states `landing` and `landed` exist (T-286, `497d9cf7`). A tree of landed work is removed at
  the land of its branch. See "Changes since the first draft".
- Not done: the removal of a released gate tree (T-289, running, not in the tip).

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
    each sandbox process (T-309). The init of the PID namespace ignores a TERM from outside, so a
    stop of a job sends TERM first and a KILL later (T-319). A check with no network gets
    `--unshare-net`. A check of `Network::Local` runs in a new network namespace with its own
    loopback, so a listener of the host is not reachable from it (T-337, `b29ac09b`). Claude
    Code on Linux also needs `socat`
    (`crates/horch-core/src/harness/claude.rs:1639`).
  - Windows: the sandbox concern returns `NotSupported` (`crates/horch-platform/src/windows/sandbox.rs`).
    The channel is a named pipe. Several concerns give `NotSupported` (for example process group
    members), and the design rules that need them rest on unit tests there. A path that
    `canonicalize()` gives has the prefix `\\?\`. `start::Command` gives a child the plain form
    (T-340). A detached child inherits no standard handle of its parent (T-342).
- Rules: PL-1 to PL-6, D1, D16.

### The ledger schema

The schema goes from version 7 (main before the merge base) to the fleetd steps. On the run
branch now they are 8 to 16 (`crates/horch-core/src/taskledger/schema.rs`). After the merge of
main they are 10 to 18. Main adds 2 versions: version 8 is the `comment-sweep` step, and version 9
is `workers.backend` of the second pane host (#363, `BACKEND_VERSION`). The first draft counted
only the first one (9 to 17). The numbers below are those of the trial merge
(`merge-main-proof-2.md`, "For the operator" item 4; verified there by the tests
`trf_48_every_old_version_migrates_to_current` and `hcb_03_migrate_from_version_7_adds_backend_and_keeps_rows`):

| After the merge | Name in code | What it adds |
| --- | --- | --- |
| 8 | `COMMENT_SWEEP_VERSION` | main: `comment-sweep` in the `CHECK` of `task_steps` |
| 9 | `BACKEND_VERSION` | main: `workers.backend` |
| 10 | `PHASE_1_VERSION` | `fleetd`, `attempts`, `jobs`, `pass_cursors`, `needs_you`, the Phase 1 event kinds, and an attempt row for each live worker |
| 11 | `PHASE_2_VERSION` | `leases`, `questions`, the new task states and task columns, and a lease for each held task |
| 12 | `PHASE_3_VERSION` | the custody columns of `worktrees` (`state`, `owner_attempt`, `rescue_ref`, `head_sha`, `dirty`, `seen_at`), and their backfill |
| 13 | `EVENT_KINDS_VERSION` | `events` built again with the event kinds of this binary |
| 14 | `WORKTREE_PATHS_VERSION` | `worktrees.path` unique only among rows that are not `removed` |
| 15 | `PHASE_4_VERSION` | `features`, `branches`, `bundles`, `bundle_members`, `gate_runs`, `remote_tips`, `ingested`, `attempts.via`, and the backfill |
| 16 | `PHASE_5_VERSION` | `requests`, `polite_stops`, and `jobs` built again with the `triage_apply` kind |
| 17 | `PHASE_6_VERSION` | `ideas`, `requirements`, `task_covers`, `trial_merges`, the rule that each task has an idea, and the idea of each run in flight |
| 18 | `PHASE_7_VERSION` | `events` built again with the event words of the health of a profile |

No fix of the finish run since the first draft adds a schema step (T-286 uses the columns and the
states that version 11 and 12 already have). The installed `horch` 0.5.2 migrates a ledger to
version 9. A fleetd build that reads 9 as "Phase 1 done" would skip the fleetd tables, so the
renumber is needed (`merge-main-proof-2.md` H4).

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

- `crates/horch-e2e` (117 files changed, 43466 insertions, 6797 deletions): 86 test files in `tests/`
  against 36 on main (`git ls-tree -r --name-only <ref> crates/horch-e2e/tests`).
  The suites `p0_*` to `p7_*` hold the gate tests of each phase. New harness modules:
  `fleetd.rs`, `bind.rs`, `exec.rs`, `expected.rs`, `fr10_agent.rs`, `idea_agent.rs`,
  `ledger_seed.rs`, `probe.rs`, `agent.rs`. The fakes (`fake-herdr`, `fake-claude`, `fake-codex`,
  `fake-antigravity` and others in `src/bin/`) stand in for herdr and each agent CLI.
- `crates/horch-e2e/expected-failures/`: 1 file for each test binary. Each entry has a test name,
  the phase that makes it green, and the reason. Phase 0 wrote each red test there.
  `expected_failures` fails on a stale entry. 5 entries remain in 3 files (see "Not proved, and
  known gaps"). T-286 removed the file `p3_worktrees.txt` and 3 entries.
- The harness stops each job of its world at teardown by the group that the `jobs` row records
  (T-365), and each `AgentShell` has its own folder (T-363). A call of `Harness::run` has a time
  limit (T-342).
- `scripts/dev-sandbox.sh` runs a command with its own `HOME`, state, ledger, Claude and Codex
  homes and `FLEET_ROOT`, so a development build never touches the live ledger of the operator.
- `scripts/pl6/` compares the result of each test before and after Phase P. `scripts/live/p7-real-conformance.sh`
  runs the conformance of a real CLI in a logged-in shell.
- The crate `herdr-docs-sync` is renamed `agent-docs-sync` and covers each agent CLI (Phase 7).

### CI changes

`.github/workflows/ci.yml`: on Linux the test job installs `bubblewrap` and `socat` and sets
`kernel.apparmor_restrict_unprivileged_userns=0`, because Ubuntu 24.04 lets no unconfined program
make a user namespace. `cargo test` runs with `--no-fail-fast`. A new job `test-windows` runs
`cargo test --workspace --locked --no-fail-fast` with `continue-on-error: true`. The jobs `test`
and `test-windows` have `timeout-minutes: 120` (T-342, `b7c65960`). The jobs `docs`,
`release-lint` and `build-windows` have no time limit (`grep timeout-minutes .github/workflows/ci.yml`).

## Changes since the first draft

The first draft described `99bf2a9f`. This section lists the 92 commits up to `72b48f3e` by subject.
Each item gives what changes for a user or an operator, why (the fault), the task and the commit.
The label "verified" names the evidence. "not verified" means that no evidence file shows a pass.
Almost every fix of the fix wave has a proof on macOS or in a Linux container, written in its
commit message. No fix has a CI proof, because CI run 5 is not done (see "How it was proved").
A fix that has a comment sweep commit and a "restore" commit: both are in the same task. This text
names the fix commit only.

### Stop of a job

What changes for an operator: a job that passes its deadline gets a TERM first. A KILL comes after
`stop_grace_s` (20 s, design.md line 1700), on a later pass. On Linux each stop also ends the
sandbox of the job. A job whose process is gone but whose group still has a member gets a notice,
and no signal.

- Why (T-319, `7a8e2a27`). On Linux a sandboxed git or gate is `bwrap --unshare-pid` with no
  `--new-session`, and `setsid` starts the command inside it. The init of the PID namespace ignores
  a TERM from outside. Fault (a): a worktree job stopped by TERM left its git sandbox alive. Fault
  (b): a pass past the deadline and the grace sent KILL at once, so the job could not run its
  handler, and the gate sandbox lived on.
- TERM first. The pass records a `TERM_SENT` fact (a note event with the job, the PID and the PID
  start time) before the signal (A24). Then it sends SIGTERM to the job group. The handler of the
  job (`kill_group_on_termination`) sends SIGKILL to the group of the work that runs: `GIT_GROUP`
  (new, armed only during a sandboxed git call), `GATE_GROUP` or `CHECK_GROUP`. The grace counts
  from the recorded fact, so a restarted fleetd counts the same grace.
- Own groups. A job process registers at most 3 of the 8 handler slots. A second registration of
  the same static takes no new slot.
- The orphan group notice. When the job process is gone and its group id still has members, fleetd
  sends no signal. With the leader gone, no start time proves that a member belongs to the job. The
  pass records a `GROUP_LEFT` notice with each member and its start time.
- The git writer wait. While a recorded member runs, no second git writer starts for that
  repository. `git_job_holds` shows the hold with the group, each member PID and start time, and
  says that the operator must end them by hand. After `time_limit_s` plus `stop_grace_s` from the
  notice, the operator gets 1 needs-you item of kind `job_abandoned`. It comes 1 time (a
  `GROUP_TOLD` fact).
- Limits, documented in the comments of `jobs::stop` and `note_group_left`:
  - A job that cannot run its handler leaves its gate or check group alive until the command
    ends. Nothing waits for it.
  - The git writer wait has no bound from fleetd. A hung orphan git holds the writer of its
    repository until the operator ends it. This was a choice of the orchestrator (O1). A KILL by
    fleetd (O2) and a second writer (O3) are not done.
  - A stop of fleetd sends no signal to a job. A cancel of a task sends no signal to a job.
- Tests: `worktree_jobs_stop.rs` (new), `gate_jobs.rs`, `worktree_jobs.rs`, `worktree_rescue.rs`.
  Linux proof: the container `t250-linux:local`, `--privileged`, in the commit message.
  Not verified: a CI run.
- Not in the tip: T-369 (landing) closes 2 stop gaps that T-319 found.

### Signal safety

- Not in the tip. The review `reviews/delta-review-jobs-and-signals.md` (verdict "Not ready") has 2
  Critical findings. Each is "verified by reading" in that file, and neither is fixed in `72b48f3e`.
  - D242-F1: a stop can signal an unrelated process. `crates/horch-platform/src/unix/group.rs:74`
    treats a group as the group of the job when its leader is gone and any process has that group
    id. The fix that the review names: signal a group only when its leader exists with the recorded
    start time. No task of the board names this finding (not verified: the search of the board
    is by the text "D242").
  - D242-F2: a TERM can arrive after a gate or check starts and before its group is armed
    (`exec_gate.rs:478`, `jobs.rs:1714`). The fix that the review names: block the termination
    signals before the launch and restore them after the group is armed. T-369 (landing) covers
    the window before the handler of a check group. It is not in the tip.
- Fixed: D242-F3 (T-383, `c67ba24f`). The Windows detached spawn fails closed when it cannot read
  or clear the inherit flag of a standard handle. It restores every handle it changed. D242-F4
  (same commit): the RR-13 invariant comment above `refuse_moved_head` is back. Verified: the
  commit message. The Windows tests are not run on a Windows host here.

### The folder lock

What changes for an operator: no change that you see. The lock is `fsx.rs` `DirLock`. It guards the
land registry, the repository lock, the land lock, the ledger open lock, the watchdog, the updater,
the job file of a compaction and the recipes command.

- T-368 (`2dda73a9`). Fault (a real race): a breaker judged a lock stale and then renamed whatever
  lock was at the path. The release of the old holder did not take the breaker guard. The next
  holder could lose its lock, and a later taker then held it at the same time (2 holders). Fix: a
  release takes the breaker guard. `remove_lock` renames the lock to a tomb, reads the owner of the
  tomb, and deletes the tomb only when the owner is the one that was judged. Else it puts the lock
  back with `rename_noreplace`. Proof: the new test `dirlock_break_keeps_a_lock_taken_after_its_check`
  failed before the fix. `fsx::` passed 30 of 30 runs, and the full lib 5 of 5 (macOS).
- T-382 (`249faec1`, review finding D243-F1, Critical). Fault: after the 5 s wait for the guard, a
  release went on without the guard and removed its lock. A paused breaker could then rename the lock
  of the next holder while a third holder took the empty path: 2 holders. Fix: when the wait
  gives nothing, the release writes `<lock>/released-<nonce>` and removes nothing. The next taker
  reads that lock as stale and breaks it under the guard. The release still blocks at most 5 s.
  Proof: the test `dirlock_release_timeout_does_not_overlap_a_new_holder` failed before the fix with
  2 holders. `fsx::` passed 30 of 30 runs (macOS).
- Limits (the "Limits" doc at `remove_lock`): the put-back runs only on Windows and on a `flock`
  error. On Windows the guard holds nothing. The owner check after the rename is then the check,
  and it has a window between a wrong rename and its put-back. The commit message states that a rename of
  a directory to a new name is atomic on APFS, Linux and Windows.
- Not verified: Linux and Windows runs of the lock tests, and a CI run.

### The ledger reads, since the first draft

- Set readers read 1 snapshot (T-327, `a5987157`). Fault: `ReadyRows::read` read the ready tasks
  and the facts of their runs in separate statements. `horch ledger status --json` has no
  transaction, so it could get a ready task whose run was not in the second read. It then failed
  with `no run <id>`. `one_snapshot` runs a read in 1 savepoint: a deferred transaction (1 WAL
  snapshot) when the connection has none, and a nested savepoint in the transaction of the pass. The
  readers of dispatch (`ReadyRows`, `read_base_tips`, `running_tasks`, `choices`), the rows of the
  task execution store (attempts, live tasks, workers), the disk budget, and the reads of the
  reconciler use it. Proof: the new test `ready_rows_read_one_moment_while_another_connection_commits`
  failed before the fix with `no run r1`.
- Each context reads the tables of another context through the reader of the owner (T-327,
  `5efc8c11`). The guard `fitness_no_context_touches_foreign_tables` is green with no change of
  the test and no allow-list entry. The commit lists each of the 14 lines and its new reader. Each
  set read is 1 statement for any count of rows (an `IN` list).
- The read-only record of each connection (T-359, `8235f257`). Fault: the trace callback of a
  read-only connection wrote the last statement to 1 process-wide slot. With 2 read-only stores the
  failure text named the second ledger and its `SELECT`. Fix: each read-only `Store` owns its
  record through an `Arc`, and the CLI keeps a clone. The new test failed before the fix.
- Not done: the reconciler still reads the owner of each active tree with SQL for each row (D244-F2,
  Important, `reviews/delta-review-custody-and-conformance.md`). T-385 is queued.
- Not verified: Linux and Windows runs, and a CI run.

### Conformance

What changes for an operator: a conformance check that fleetd cannot prove is "not proved", not
"fail". A profile is not marked unhealthy for it. Antigravity (`agy`) makes no drift idea.

- Why (T-347, `f21c54e2`, `8f0e1802`, `96eba319`). In each e2e world that runs
  `horch ledger watch --once` with no fleetd, the conformance job gave "fail" for "Operator
  boundary" of claude and codex, because the runner could not connect to the command channel
  (`ENOENT`). Each of their profiles became unhealthy and got no dispatch. The ledger also got a
  drift feature for claude and for agy. The new test
  `passes_with_no_fleetd_mark_no_conformant_profile_and_add_no_drift_feature` was red before the
  fix (88 claude profiles unhealthy).
- The rule. "Operator boundary" is "not proved" only when the 1 act that the runner cannot do
  outside the sandbox is the connect to the command channel, and no channel server exists
  (`NotFound` or `ConnectionRefused`). Each other gap stays "fail". A run with no failed check and a
  check that is not proved writes no health event and makes no drift idea. A fleetd that serves the
  command channel queues 1 new conformance run for each CLI whose last run has a check that is not
  proved: 1 time for each start of fleetd.
- Antigravity is unsupported by design. Its "Operator boundary" is "not supported" on each OS, and
  profile validation refuses its profiles. A report with that result makes no drift idea and no
  needs-you item. Its profiles stay unhealthy.
- PL-2: the device and inode of the fleet root come from `horch_platform::fs::device_and_inode`.
  Windows gives `NotSupported`, which the verdict reads as "not visible".
- Not in the tip: the hold of a profile that was never proved. A profile with no health event is
  healthy. So a first conformance run that is "not proved" leaves the profile dispatchable (D244-F1,
  Critical, "verified by reading" in `reviews/delta-review-custody-and-conformance.md`). The
  design has no rule that a version with no passed conformance gets no dispatch (the commit
  message of `f21c54e2` says this). T-384 is running.
- Related: the real-CLI gate script `scripts/live/p7-real-conformance.sh` made its temp fleet in
  `/tmp`, and the Linux sandbox mounts a new tmpfs on `/tmp`. On Linux the gate gave "fail" for
  both checks, so the release gate could not pass. The script now uses `/var/tmp` on Linux (T-337,
  `215dc0c1`). macOS keeps `/tmp` because of the 104-byte limit of a socket path.

### Worktree custody, since the first draft

- The states `landing` and `landed` (T-286, `497d9cf7`). Fault: the states of the design did not
  exist, so the tree of landed work was never removed. The moves now are:
  - `active` -> `landing`: the attempt ends verified, and the task is done. No rescue job.
  - `active` -> `created`: only when the attempt ended clean (rejected, asked, quota,
    `critical_open`, `context_limit`) or the task is parked.
  - `active` -> `orphaned`: a crash, kill, timeout, stall or `no_result`, a cancelled task, or
    another end. A verifying task keeps its tree `active`.
  - `landing` -> `landed`: the reconciler sees the task branch landed on the feature branch. It
    moves the row only with a `worktree_list` read that is not older than the land (RR-15). The row
    records `landed_commit` and `landed_at`.
  - `landing` -> `rescued`: the branch is abandoned, and the rescue is verified.
  - `landed` -> `removed`: fleetd queues a `worktree_remove` job. The branch is deleted only if it
    is merged. The refusals of RR-13 apply (dirty tree, untracked file, stash, HEAD not the landed
    commit). A refusal keeps the tree and opens 1 needs-you item.
- No schema change: the `CHECK` and the table of legal moves already had both states.
- 3 entries left the expected-failures lists (`a_task_finishes_through_review_land_pr_and_cleanup`,
  `rr15_landed_work_of_a_done_task_is_not_listed`, `fm27_only_landed_or_rescued_trees_are_removed`).
  `the_whole_flow_on_the_fake_herdr` now asserts the designed effects (no retired wake, attempt 2
  on a new worker) and stays on the list for T-289.
- Not in the tip: removal of released and dropped trees (T-289, running). A gate tree such as
  `gate-F-0001` stays open until it lands. 1 test of `p3_refusals.txt` and
  `the_whole_flow_on_the_fake_herdr` wait for it.
- Also: the 4 restore tests of `p3_worktrees_cli` set `worktree_list_period_s = 5`, so the rescue of
  the tree of T1 no longer uses 60 s of the 120 s wait (T-290, `686940dd`). `p3_worktrees` now runs in 196 to 331 s, not 680 s (T-356, `f7c102c8`). The test
  world has no docs jobs, a list period of 10 s and a pass period of 1 s. No assert changed.
  Verified: the commit message (8 of 8 tests, 3 runs, macOS).

### The question resume

- In the first draft (T-300, R8-F1): an answer resumes the CLI session of the asked attempt.
- Not in the tip: D244-F3 (Important, "verified by reading"). The next worker passes the session id
  to the adapter without a proof that the session exists. When the agent deleted the session, the
  adapter refuses the resume, the attempt ends `no_result` and burns 1 attempt. At the last allowed
  attempt the task escalates, and no fresh worker gets the question, the answer and the committed
  work. The review's fix: a missing session is not a task failure; queue 1 fresh launch with no
  burn. T-386 is running. The operator decision 6 below stays for the case with attempts left.

### Windows

What changes for a user on Windows: the roster loads, a path from `canonicalize()` reaches git, and
`horch tell` ends when its caller reads its output. Windows is still not a supported release target
(see "Not proved, and known gaps"). The Windows target does not compile on the Mac that ran these
fixes (`libsqlite3-sys` needs a Windows C sysroot). The proof of each item below is "not verified"
until CI run 5.

- Roster and CRLF (T-339, `e120be16`). Fault: on a Windows checkout the compiled-in roster base files
  have CRLF line ends (the repository has no `.gitattributes`). `split_frontmatter` accepted only
  `---\n`, so each load of a base failed with "file does not start with a '---' frontmatter fence".
  Fix: the line end of the opening fence sets the line end of the closing fence. A CRLF file parses
  to the same teammate as its LF form. CI run 4 had 369 Windows tests of this fault
  (`ci-run-4.md`, FIX-35).
- Verbatim paths (T-340, `c95240f8`, landed through T-348). Fault: `canonicalize()` gives
  `\\?\C:\...` on Windows, and git refuses it ("could not create leading directories"). 163 Windows
  tests failed in CI run 4. Fix in `horch-platform`: `path::plain_of_verbatim` gives the plain Win32
  form, and `path::for_child` applies it. `start::Command::arg`, `args` and `current_dir` call it, so
  each product git call gets the plain form. The prefix stays for a path of 260 UTF-16 units or
  more, a UNC path with no server or share, an empty, `.` or `..` component, a reserved device name
  (CON, NUL, COM1, LPT1, ...), a trailing dot or space, a `/`, or a character that Win32 refuses.
  Test code that starts its own git got `for_child` (T-355, `f18eb76e`: `bundles.rs`,
  `w3_finish_steps.rs`, `worktree_create.rs`, `worktree_jobs.rs` and 3 lib tests). Proof here: macOS
  tests, and `cargo check` of `horch-platform` for `x86_64-pc-windows-msvc`.
- The detached child (T-342, `b7c65960`). Fault: `horch tell` starts the detached mail courier. On
  Windows, `std` starts a child with handle inheritance on, so the courier inherited the stdout and
  stderr pipes of `horch tell`. A caller that waits for the end of the output (a test, or the Bash
  tool of an agent) waited as long as the courier lived, and the courier redelivers for hours. 4
  tests of `fleet18_messaging.rs` hung, filled the 4 test threads, and the job ran 3 h until the
  orchestrator cancelled it. The cause is "probable": the log shows no line after the start of the 4
  tests. Fix: `horch_platform::group::spawn_detached` is the 1 function that starts a detached child
  that inherits no handle of the parent. On Windows it clears the inherit flag of the 3 standard
  handles for the spawn, sets it back, and holds a lock. The mail courier, the detached branch of the
  launcher of fleetd, the prime child, the compaction job and the grid tidy use it. The start of
  fleetd, the jobs of fleetd and the self-update helper do not.
- Fail closed (T-383, `c67ba24f`, review D242-F3). `spawn_detached` fails before the spawn when it
  cannot read or clear the inherit flag of a valid standard handle. It restores each handle that it
  changed. New tests with an injected flag API (`a_failed_query_spawns_nothing_and_restores_the_cleared_handles`,
  `a_failed_clear_...`, `a_failed_restore_...`, `a_handle_that_is_not_valid_is_not_a_failure`) run only on
  Windows. Not verified: no Windows run.
- CI time limits (T-342). `Harness::run` calls `output_within(cmd, DEADLINE)`: the child must exit
  and close its output inside the limit, else the harness kills the child and its group and panics
  with the command and its state. The jobs `test` and `test-windows` have `timeout-minutes: 120`.
- Windows test faults (T-341, `6c0fba97`). `tempdir_in("/tmp")` in 5 test files, a `C:\` path given
  to `node` as an ES module (`pi_compact.rs`), and CRLF in the output of Python (`godot_wave.rs`).
  Test-side only. About 50 tests.

### CI run 4 and its fixes

CI run 4 (`44861461538`, head `2c6ef0b4`) is described in "How it was proved". The column "Class"
says whether a failure group was a fault of the product, of a script or of a test. Each fix below
has a proof on macOS, or on Linux in a container, in its commit message. No fix has a CI proof.

| Fix | Class | What it fixes | Task, commit |
| --- | --- | --- | --- |
| FIX-26 | product doc | `docs` job: a doc comment of `ingest.rs` named the private `verifier::park_unlanded` as an intra-doc link (`rustdoc::private_intra_doc_links`). It is now in code font | T-331, `f82ac305` |
| FIX-27 | test setup | 10 Linux tests: the pass helpers of `p4_no_loss` and `p6_features` replaced the sealed `PATH`, so `bwrap` was gone and no git ran. `i10` waited for exactly 1 pass: it now waits for the rescue ref (a pass stops at its time budget, PL-5) | T-332, `e8eafe97`, `c52341e0` |
| FIX-28, FIX-31 | test | tests that name no run fail with "2 active runs ... Name one with --run", because a failed conformance adds a drift feature with a run. They name their run now. The polite-stop tests run no conformance job | T-333, `61874795` |
| FIX-29 | test | 4 `skills_exposure` tests counted a launch of the fleetd conformance job as the agent of the worker. Not a product fault: the launch refuses a skills drift before the agent command exists | T-334, `e15b0fcd` |
| FIX-30 | test | `g50`, `g52` and `rr10` waits: a launch took 29.7 s at load 50 against a wait of 30 s. The launch waits use the harness `DEADLINE` (120 s). The bound of RR-10 counts the fleet seconds that the steps of the test add. A scratch edit that delays the adoption still fails the test | T-335, `e76dc9b0`, `25ff0e00` |
| FIX-32 | test and fake | the fake `codex sandbox` confined nothing on Linux. It now runs the command in `sandboxed_job`. The check-timeout proof looks for its processes on the host. A stale test expected "unknown teammate" and now expects the whole end reason "its spawn failed: ..." (T-254). 3 other tests do not reproduce | T-336 (cancelled), T-364, `a8d376a1`, `8734f885` |
| FIX-33 | test, and a product script | 4 Linux tests: on Linux a check has its own loopback, so a host listener refuses by design (SR-3 holds). The check now runs its own webhook on its loopback. The script `scripts/live/p7-real-conformance.sh` is a product fault (see "Conformance"). A fourth test was a race with the boot conformance job | T-337, `215dc0c1` |
| FIX-34 | test | Linux test setup: `fakes.rs` kept no `host-bin` on `PATH`. `fm2` read the status row after a pass ended the attempt. `watch_once_runs_the_phase4_pass` has no `PATH` and used the installed `horch` as job host. The asserted effect `leased/push` is retired by T-307 (design.md, stall ladder, rung 6): it is now `ready`, `push`, `crashed`, `not_before` set | T-338, `30bf47d7`, `c7e167b5` |
| FIX-35 | product | the roster parser and CRLF (see "Windows") | T-339, `e120be16` |
| FIX-36 | product | a verbatim path never reaches a child (see "Windows") | T-340, `c95240f8`, T-355, `f18eb76e` |
| FIX-38 | test | Windows test setup (see "Windows") | T-341, `6c0fba97` |
| FIX-39 | product | the detached child of `horch tell` (see "Windows") | T-342, `b7c65960` |

The test harness got 3 more fixes that are not CI fixes:

- T-363 (`d2e710fd`). Fault: every `AgentShell` of a test polled the same folder `agent-shell`.
  When 1 shell got its `go.sh`, every shell sourced it, and 1 shell truncated the code file of
  another between its write and its read. That gave `ParseIntError { kind: Empty }` under load.
  Fix: 1 folder `agent-shell-<n>` for each shell, and the code file goes to `code.tmp` and then to
  `code` with `/bin/mv`. 10 of 10 runs passed (macOS).
- T-365 (`2ddb204d`). Fault: the jobs of fleetd outlive a fleetd by design ("Long work is a job"),
  and the teardown of an e2e world reported "the test leaked N process(es)" 25 s later. Fix: the
  teardown stops each job of its world by the group that the `jobs` row records: TERM, the stop
  grace, KILL, then `EXIT_LIMIT`. Evidence: `gates/t4-teardown-evidence.md` (3 of 30 runs failed
  before the fix).
- T-356 (`f7c102c8`). `p3_worktrees` runs in 196 to 331 s, not 680 s (see "Worktree custody").

### Host needs

- git 2.41 or newer. The rescue job needs `git --attr-source` (`rescue_git.rs:296-303`). On an older
  git the rescue job fails with "a snapshot with the filters off needs git 2.41 or newer", the tree
  stays on disk, and 1 needs-you item `rescue_failed:<id>` opens. No work is lost. No worktree then
  reaches `rescued`, so no rescued worktree is removed on that host (`gates/linux-git-241-proof.md`
  gaps G1 to G4, verified by reading and by a run in `t250-linux:local` with git 2.39.5).
- The check of the git version is not in the tip. Today `horch doctor`, the start of fleetd and the
  conformance read no git version (verified by reading in `linux-git-241-proof.md`; the doctor run
  there printed no git line). T-389 is running. Proof of the 19 tests that need git 2.41: all 19 and
  4 full files are green on Linux with git 2.51.2 (`t-git241-linux:local`, 3 of 3 rounds,
  `gates/linux-git-241-proof.md`, verified). The image stays on the host until the operator removes
  it (release step 6).
- util-linux `setsid` on Linux. A job sandbox starts the command with `/usr/bin/setsid` or
  `/bin/setsid`. When neither exists, the sandbox fails with "setsid is not in /usr/bin or /bin;
  install util-linux" (`crates/horch-platform/src/linux/sandbox.rs:34-47`, verified by reading).
  `bubblewrap` and `socat` are needed as before.

### The merge of `origin/main`

- The trial merge now covers the 3 new commits of main: `c28e6dd6` (#364, Chrome code-sign clone
  flag), `50e63b2a` (#363, `herdr-core` and the Herdr desktop app as the pane host `hc`) and
  `5863c698` (Release v0.5.2). `merge-main-proof-2.md` (T-330): 34 files with 70 conflict hunks, 7
  fix commits. `cargo check --workspace --all-targets`, the CI form of clippy and `cargo fmt --all --check`
  exit 0. The `--all-targets` form of clippy has 1 error that is red before the merge
  (`disk.rs:190`). The commit message of `5efc8c11` (T-327) says that it fixes it (not run since).
  `horch-core` 3773 passed, 2 failed; `horch` 617 passed, 1 failed. Those failures are red before the
  merge. Not run as 1 command: `cargo test --workspace --locked`.
- fleetd runs its workers only on `herdr`. In the trial, `horch fleet` and `horch fleetd` refuse the
  backend `hc` (`HC_REFUSED`), and the 6 `hc` tests of main in `hc_fleet_start.rs` are replaced by 2
  tests of the refusal (`3fd534f7`). This refusal is in the trial branch, not in the tip. Task T-351
  scoped an `hc` adapter for fleetd: 15 gaps, 2 security findings, 9 units, 3 options
  (`gates/hc-adapter-scope.md`).
- Docs of main that promise `hc` fleets (`README.md`, ADR 0003, `docs/live-checks/hc-backend.md`,
  `docs/specs/pane-host.md`, `docs/testing-and-gates.md`, `docs/architecture.md`, `GLOSSARY.md`) merge
  with no conflict and describe the retired flow. Nothing edits them yet.
- The schema numbers move to 10 to 18 (see "The ledger schema").

### Limits that are documented

Each limit below is written in the code comments or in a gate file. It is not a defect that a test
hides.

1. A job that cannot run its handler leaves its gate or check group alive. The git writer wait for
   an orphan group has no bound from fleetd (comments of `jobs::stop` and `note_group_left`).
2. The folder lock: the put-back after a rename runs on Windows and on a `flock` error. On Windows the
   guard holds nothing (the "Limits" doc at `remove_lock`).
3. A Windows path keeps its `\\?\` prefix in the cases that "Windows" lists.
4. A Linux host needs git 2.41 and util-linux `setsid` (see "Host needs").
5. fleetd workers run only on `herdr`, not on `hc` (see "The merge of `origin/main`").
6. A job process has 8 handler slots and registers at most 3 (see "Stop of a job").

### Findings of the 3 delta reviews

The files are in `docs/specs/fleetd/reviews/`. Each review read the commits from base `50fe4705` to a
tip near `05ff4081` (the review of the ledger and the lock: `5ab3293c`). Each verdict is "not ready"
or "not approved". Each finding is labeled "verified by reading" in its file.

| Finding | Severity | What | State at tip `72b48f3e` |
| --- | --- | --- | --- |
| D242-F1 | Critical | a stop can signal an unrelated reused process group | open, no task found |
| D242-F2 | Critical | a TERM can arrive before a gate or check group is armed | not fixed in the tip. T-369 is landing |
| D242-F3 | Important | the Windows detached spawn continues when a handle flag fails | fixed, T-383, `c67ba24f` |
| D242-F4 | Minor | the RR-13 invariant comment is gone | fixed, T-383, `c67ba24f` |
| D243-F1 | Critical | 2 holders of the folder lock after the 5 s guard timeout | fixed, T-382, `249faec1` |
| D244-F1 | Critical | a profile with a first "not proved" conformance is dispatchable | not fixed. T-384 is running |
| D244-F2 | Important | the reconciler reads the owner of each active tree with SQL for each row | not fixed. T-385 is queued |
| D244-F3 | Important | a stale resume burns the last attempt | not fixed. T-386 is running |

A fix of D243-F1 differs from the fix that the review names. The review asks that a release return
with no removal. The fix writes an inert mark and lets the next taker break the lock. A new review has
not read it.

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
| Linux: no extra host tools | `bubblewrap`, `socat`, util-linux `setsid` and git 2.41 or newer are required | Job and worker sandboxes (T-267, T-309), and the rescue job (`--attr-source`, T-377) |
| A job past its deadline is killed at once | TERM first, then KILL after `stop_grace_s` (20 s); on Linux the sandbox ends too | The job can end its own sandbox (T-319) |
| A conformance check that cannot be proved is "fail" | It is "not proved"; no unhealthy mark and no drift idea | An e2e world with no fleetd marked each profile unhealthy (T-347) |
| The tree of landed work stays on disk | fleetd removes it at the land of its branch | The states `landing` and `landed` (T-286) |
| `horch tell` on Windows can hang its reader | The detached courier inherits no handle | T-342 |
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

Found since the first draft. Each item has its section in "Changes since the first draft":

22. T-286 (done): the states `landing` and `landed` did not exist, so the tree of landed work was never removed.
23. T-319 (done): a stop of a job left the sandbox of its git or gate alive on Linux.
24. T-327 (done): `horch ledger status --json` could fail with `no run <id>`, because 2 reads of a pass were not 1 snapshot.
25. T-347 (done): an e2e world with no fleetd marked each profile unhealthy, because a check that cannot be proved was "fail".
26. T-359 (done): the failure text of a read-only ledger could name another ledger.
27. T-368 and T-382 (done): the folder lock could give 2 holders (2 races).
28. T-337 (done): the real-CLI gate script made its temp fleet in `/tmp`, which the Linux sandbox hides, so the release gate could not pass on Linux.
29. T-339 (done): on Windows, the roster base files with CRLF did not load.
30. T-340 (done): on Windows, git refused a `\\?\` path.
31. T-342 (done): on Windows, `horch tell` could hang its reader through the detached courier.

## How it was proved

"verified" names the evidence: a file under `docs/specs/fleetd/gates/` or `docs/specs/fleetd/reviews/`,
or a commit. "not verified" means that no evidence file shows a pass. CI run 5 and the final gates
are not done. Every row of "not verified" waits for them.

| Item | Result | Label |
| --- | --- | --- |
| Gate of Phase 0 | `cargo test --workspace --locked` at `f8c0b7a6`: 2866 pass, 0 fail, 11 ignored; each red test fails with its listed reason | verified, `gates/fleetd-p0.md` |
| Gate of Phase P | PL-2 green on macOS (18 of 18). PL-6 on Linux passes in Docker (2938 tests, no difference) with no CI run id. PL-6 on Windows pending CI | macOS verified, `gates/fleetd-pp.md`; Linux and Windows not verified |
| Gate of Phase 1 | Each row passes on macOS at `97236dda` (3206 ok, 0 failed). Linux in Docker: 11 failed, 10 by the container, 1 fixed in `3709bb4d` | macOS verified, `gates/fleetd-p1.md`; CI not verified |
| Gate of Phase 2 | Green on macOS at `9a2bf61d`. Each row passes on Linux in Docker | macOS verified, `gates/fleetd-p2.md`; CI not verified |
| Gate of Phase 3 | Green on macOS at `d84c9ddd`. Linux and Windows: not run | macOS verified, `gates/fleetd-p3.md`; Linux and Windows not verified |
| Gates of Phases 4, 5, 6, 7 and the final gate | Not written (T-223, T-224, T-225 and T-227 are queued) | not verified. To fill at the end |
| Pre-gate of Phase 4, tip `895e3848` | Each rule row passes on macOS. Red then: `arch_scan` `fitness_no_context_touches_foreign_tables` (14 lines) and 2 Phase 4 expected failures in `taskledger.txt`. T-327 (`5efc8c11`) and T-286 (`497d9cf7`) fixed both after the pre-gate (their commit messages) | verified for `895e3848`, `gates/pre-gate-p4.md`. The fix is not verified: no gate ran again on the tip |
| Pre-gate of Phase 5, tip `895e3848` | Each rule row passes on macOS except the `arch_scan` guard (fixed by T-327, same note). The Windows pipe test (REQUIRED 1) runs only in CI | verified for `895e3848`, `gates/pre-gate-p5.md` |
| Pre-gate of Phase 6, tip `895e3848` | 306 passed, 1 failed (the `arch_scan` guard), 2 ignored (`nfr` measurements) | verified for `895e3848`, `gates/pre-gate-p6.md` |
| Early full gate on macOS | Running (T-378). No report is in the tip | not verified. To fill at the end |
| End reviews of phase groups | R1 to R6 done (T-25 to T-30): `reviews/end-review-p0-pp-p1.md`, `-p2-p3.md`, `-p4.md`, `-p5.md`, `-p6.md`, `-p7.md` | files verified. The state of each finding is in `docs/tickets/fleetd-finish/FIX-INDEX-*.md` (not tracked in git) and is not checked in this file |
| End review A of the run, tip `08c6d4ff` | 0 Critical, 3 Important, 2 Minor. R8-F1 fixed (T-300). R8-F2 open: T-301 measured that no Seatbelt form limits a check to loopback, see `gates/macos-check-sandbox-lan.md`. R8-F3 fixed (T-302). R8-F4 and R8-F5 fixed (T-303) | verified, `reviews/end-review-a.md` and the decisions log |
| 3 delta reviews, tip near `05ff4081` | 3 files, 8 findings (see "Findings of the 3 delta reviews"). 3 fixed, 5 not fixed in the tip | files verified: `reviews/delta-review-jobs-and-signals.md`, `reviews/delta-review-ledger-reads-and-locks.md`, `reviews/delta-review-custody-and-conformance.md`. The 3 fixes: verified by their commits, not by a new review |
| End review R8 of the final change | Queued (T-222) | not verified. To fill at the end |
| CI run 1 (CI0), run 44680906908, head `085427fb` | 1 of 6 jobs passed. macOS 4411 passed, 666 failed | verified, `gates/ci-baseline-2026-11.md` |
| CI run 2 (CI1), run 44701297167, head `7fbdc0d3` | 1 of 6 jobs passed. macOS 4796 / 270, Linux 4691 / 375 (passed / failed). Windows: 0 tests ran | verified, `gates/ci-run-2.md` |
| CI run 3 (CI2), run 44784709135, head `001e73c2` | 2 of 6 jobs passed (`docs`, `build-windows`). macOS 5002 / 188, Linux 4821 / 368. `release-lint` 2 errors. Windows: 1 compile error | verified, `gates/ci-run-3.md` |
| CI run 4 (CI3), run 44861461538, head `2c6ef0b4` | The orchestrator cancelled it at 06:31Z. 2 of 6 jobs passed (`release-lint`, `build-windows`). macOS 5186 passed, 23 failed. Linux 5157 / 52. `docs` failed (1 rustdoc error). `test-windows` ran 3 h 0 min 42 s, then was cancelled: 2666 passed, 813 failed, 7 ignored in 280 test binaries, then a hang in `fleet18_messaging.rs` | verified, `gates/ci-run-4.md`. The label of the first draft, "CI3", is this run |
| The fixes of CI run 4 | 13 fixes (FIX-26 to FIX-36, FIX-38, FIX-39) and 3 harness fixes. Each has a proof on macOS or in a Linux container in its commit message. The Windows fixes have no Windows run: `cargo check` of `horch-platform` for `x86_64-pc-windows-msvc` only | commits verified (see "CI run 4 and its fixes"). Not verified in CI |
| CI run 5 | Running (T-376) | not verified. To fill at the end: the run id and the result |
| Linux proof of the tests that need git 2.41 | 19 tests and 4 files green on Linux with git 2.51.2, 3 of 3 rounds. With git 2.39.5, 2 of them fail with the version text | verified, `gates/linux-git-241-proof.md` |
| Trial merge of `origin/main` to v0.5.1 | `1e782060`: 26 conflicts resolved. `horch-core` 3699 / 0, `horch` 614 / 0, `ui-prop-tester` 241 / 0, both clippy forms and `cargo fmt --all --check` exit 0 | verified, `gates/merge-main-proof.md` |
| Trial merge of `origin/main` to v0.5.2 | `3fd534f7`: 34 files with conflicts, 7 fix commits. `cargo check --workspace --all-targets --locked`, the CI form of clippy and `cargo fmt --all --check` exit 0. `horch-core` 3773 / 2 and `horch` 617 / 1 (the 3 failures are red before the merge) | verified, `gates/merge-main-proof-2.md` |
| Trial merge: rest | `cargo test --workspace --locked` as 1 command, the e2e files outside the table of the proof, the CI of the trial, the real `hc` contract test, the tests of #364 that start Chrome | not verified, `gates/merge-main-proof-2.md` "For the operator" item 7 |
| Final tip, CI links and gate results | To fill at the end | not verified |

## Not proved, and known gaps

1. Windows. The sandbox concern is `NotSupported`, and `test-windows` has `continue-on-error: true`. No CI run shows Windows tests passing. CI run 3: 1 compile error. CI run 4 compiled and ran 280 test binaries (T-261 landed the work of T-243): 813 failed, then it hung. The fixes of FIX-35, FIX-36, FIX-38 and FIX-39 have no Windows run. T-342 gives the job a limit of 120 minutes.
2. Real CLIs. Each e2e test uses a fake CLI. No run of `scripts/live/p7-real-conformance.sh` for real Claude and Codex is recorded (`end-review-a.md`, Operator decision 2; `end-review-p5.md` OD-3).
3. Antigravity. Its adapter is proved with a fake only. Antigravity is unsupported by design: its "Operator boundary" is "not supported", it makes no drift idea, and its profiles stay unhealthy (T-347). Real `agy` gets no dispatch (`end-review-a.md`, Operator decision 5).
4. The macOS check sandbox reaches the LAN address of the host (R8-F2, `gates/macos-check-sandbox-lan.md`). On Linux a check has its own network namespace.
5. FR-7 and SR-8 as the literal design text: the tests prove a closed list of 9 typed lines, not 1 doorbell line, and an OS sandbox refusal leaves no `requests` row (`pre-gate-p5.md` Q1, Q2).
6. Open findings of the delta reviews: D242-F1 (a stop can signal a reused process group, no task), D242-F2 (T-369 landing), D244-F1 (T-384), D244-F2 (T-385) and D244-F3 (T-386). See "Findings of the 3 delta reviews".
7. Linux stop of a job: a job that cannot run its handler leaves its gate or check group alive, and an orphan git holds the git writer of its repository until the operator ends it (documented limits). A fleet root under `/tmp` on Linux gave "Operator boundary" fail and an unhealthy profile (decisions log 05:41Z). The gate script of the release now uses `/var/tmp` (T-337). A production fleet root under `/tmp` is not verified at the tip.
8. The old git. `horch doctor` and the start of fleetd do not check the git version (T-389 is running). The first sign is a failed rescue job.
9. A released gate tree stays on disk (T-289, running).
10. Main's change S5 (a quota refusal is held no longer than its window) is not tested through a fleetd hold (`merge-main-proof.md`).
11. `hc`. fleetd runs its workers only on `herdr`. In the trial, `horch fleet` and `horch fleetd` refuse `hc`. The docs of main that promise `hc` fleets are not edited (`gates/hc-adapter-scope.md`, `gates/merge-main-proof-2.md` "For the operator" items 1 to 3). A row with `backend = 'hc'` in a ledger of horch 0.5.2 gets the herdr watchdog of fleetd (item 5, not verified as a behavior).
12. The real merge. T-221 is queued. The trial merge has no CI run.

Expected-failure entries that remain (`crates/horch-e2e/expected-failures/*.txt`, 5 entries in 3 files):

| File | Test | Phase | Owner |
| --- | --- | --- | --- |
| `i308_fleet_steward.txt` | `a_fleet_with_an_abandoned_worktree_spawns_the_worktree_steward` | 7 | operator decision (T-87) |
| `i308_fleet_steward.txt` | `an_exhausted_claude_pool_starts_no_codex_steward` | 7 | operator decision (T-87) |
| `i308_fleet_steward.txt` | `the_no_change_steward_task_finishes_without_a_worktree` | 7 | operator decision (T-87) |
| `p3_refusals.txt` | `run_gate_folder_is_added_and_removed_through_the_jobs` | 7 | T-289 (the entry names T-199, cancelled) |
| `taskledger.txt` | `the_whole_flow_on_the_fake_herdr` | 4 | T-289 (`497d9cf7`: the gate tree `gate-F-0001` stays open) |

T-286 removed 3 entries and the file `p3_worktrees.txt`. The list of 8 entries in the first draft is
now false. T-94 (queued) audits the rows.

Ignore marks (`git grep -n '#\[ignore' fleet/fleetd-finish -- crates`): 30 on the run branch, 14
on `origin/main`. 21 are child helpers that a named parent test runs. The others:

- `crates/horch-e2e/tests/p3_refusals.rs:429`, `:558`, `:686`, `:730`, `:828`: "flaky until T-199". T-199 is cancelled. The work moved to T-286 and T-289. T-286 added the mark at `:730` for the test `orphaned_tree_is_rescued_and_removed_when_its_idea_is_merged` (`497d9cf7`). The first draft listed 4 marks.
- `crates/horch-core/tests/nfr.rs:433` (`nfr_02_tick_budget`), `:668` (`nfr_02_live_store_rss`), `crates/horch-core/tests/telemetry_collect.rs:834` (`project_rollups_timing`): measurements that need `HORCH_PERF_*`.
- `crates/horch-core/tests/context_prime.rs:1090`: live, needs the real prime agent in a herdr pane.

## Decisions for the operator

1. **comment-sweep under fleetd** (`merge-main-plan.md` Q1, `merge-main-proof.md` item 4). Main runs comment-sweep in each profile through the retired `horch ledger finish`. Options: (a) fleetd does not run it, and the docs say so; (b) port it as a fleetd verifier step that starts 1 `comment-remover`. Recommendation: (a) for the merge, and a ticket for (b). The trial fixes `05cdf1e9` and `14833c51` follow (a).
2. **Schema numbers** (`merge-main-plan.md` Q2, `merge-main-proof-2.md` item 4). The fleetd steps move to 10 to 18 after main's versions 8 and 9 (#363 adds `workers.backend` as version 9). Does a ledger file exist that a run-branch build or a trial build migrated past version 7, other than a scratch `HORCH_LEDGER_DB` file? Recommendation: renumber. The decisions log (03:52Z) says no run-branch build was installed. `horch --version` gives 0.5.1 on the machine of the run, and the live ledger was not read, so this is not verified.
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
15. **Design lines with old schema numbers** (`merge-main-proof.md` item 1). `design.md:1611` says "Schema v8 adds a `leases` table" (code: 9 on the run branch, 11 after the merge). `design.md:1615` says "Schema v9" for ideas (code: 16 on the run branch, 17 after the merge). `preservation-05-contracts-and-values.md:8`, `:9`, `:98` say "v8 leases, v9 ideas". Recommendation: change the text after the merge.
16. **Unlisted request kinds** (`end-review-p5.md` OD-1). `worker_progress`, `worker_done`, `run_adopt` and `question_answer` are not in the Phase 6 map. `spawn_adopt` is retired (T-302). Recommendation: add the 4 kinds to the map.
17. **When a gate tree is removed** (decisions log 22:40Z). The design has no rule for the end of a gate tree. T-289 implements a removal. It is running and not in the tip.
18. **The main checkout branch** (decisions log line 44). The main checkout is on a fleetd branch, so a new solo run starts from it. Recommendation: switch it to `main`.
19. **fleetd and the second pane host `hc`** (`gates/hc-adapter-scope.md`, `gates/merge-main-proof-2.md` "For the operator" items 1 to 3). fleetd runs its workers only on `herdr`. The trial refuses `hc` with `HC_REFUSED`, and 6 tests of main are replaced. The scope report lists 15 gaps, 2 security findings, 9 units and 3 options for an adapter. Decide: write the adapter, or keep the refusal and edit the docs of main that promise `hc` fleets. Recommendation: not given here. Read the 3 options in the scope report.
20. **Open delta findings** (`reviews/delta-review-*.md`). D242-F1 has no task. D242-F2, D244-F1, D244-F2 and D244-F3 have tasks that are not in the tip (T-369, T-384, T-385, T-386). Recommendation: do not merge before the 3 Critical findings (D242-F1, D242-F2, D244-F1) are fixed and read again.
21. **The Windows job `test-windows`** (`continue-on-error: true`, `timeout-minutes: 120`). Keep it advisory until a CI run shows Windows tests passing, or make it required. Recommendation: keep it advisory for this merge, and say that Windows is not a release target (item 8).
22. **Git 2.41 on Linux hosts** (`gates/linux-git-241-proof.md`). Until T-389 lands, an old git shows only as a failed rescue job. Recommendation: land T-389 before the merge.

## Release steps (not done by this pull request)

1. Decide each item of "Decisions for the operator" before the merge.
2. Run the real CLI conformance (item 13).
3. Add a line under `## Unreleased` in `CHANGELOG.md` only if the merge must publish a release. This pull request adds no CHANGELOG line, because the goal prompt forbids a release from the run, and a merge to `main` with a line publishes one at once (`merge-main-plan.md` S9).
4. After the release, `horch self-update` restarts a running fleetd before it replaces itself (OR-6, D9). A run in flight follows "What a release does to a run in flight" in the design: the release backfills the rows of each phase.
5. On each Linux host, install `bubblewrap`, `socat`, util-linux `setsid` and git 2.41 or newer.
6. Remove the Docker images that the run left: `t250-linux:local` (2.08 GB), `t-git241-linux:local` (`docker rmi t-git241-linux:local`, `gates/linux-git-241-proof.md`) and `ubuntu:24.04` (117 MB, owner not known) (decisions log 05:34Z).
7. To fill at the end: the final tip, the CI links of run 5 and of the final gates, and the gate results.

## How to review

1. Read `docs/specs/fleetd/design.md`: Summary, Amendments, Requirements, Migration plan.
2. Read `crates/horch-platform/src/` (the OS layer), then `crates/horch-core/src/fleetd/` in this order: `fleet_runtime`, `task_execution`, `worktree_custody`, `integration`, `ideas`, `worker_profiles`.
3. Read `crates/horch-core/src/taskledger/schema.rs` (the migrations) and `crates/horch-core/src/execution/lifecycle.rs` (the wrapper).
4. Read `crates/horch/src/main.rs` and `crates/horch/src/cmd/` (the new and retired commands).
5. Read `crates/horch-e2e/tests/p0_*` to `p7_*` and `crates/horch-e2e/expected-failures/`.
6. Read the gate files in `docs/specs/fleetd/gates/` and the reviews in `docs/specs/fleetd/reviews/`. Start with `ci-run-4.md`, the 3 `delta-review-*.md` files and `merge-main-proof-2.md`.
7. Read the section "Changes since the first draft" of this file for the fix wave.

The full check, each command through `scripts/dev-sandbox.sh`:

```sh
scripts/dev-sandbox.sh cargo fmt --all --check
scripts/dev-sandbox.sh cargo clippy --workspace --all-targets --locked
scripts/dev-sandbox.sh cargo build --workspace --bins --locked
scripts/dev-sandbox.sh cargo test --workspace --locked
scripts/dev-sandbox.sh cargo clippy --workspace --bins --release --locked -- -D warnings
```

The last command is the form that CI runs. A gate that is narrower than CI passed while CI failed
on 2026-11-08, so run both forms of clippy. The result of these commands at the final tip: to fill
at the end.

Build the binaries before `cargo test`, or each `horch-e2e` test fails with "horch is not built".
Under a load average above 25, e2e tests can report a leaked process at teardown. T-365 stops the jobs of a test world at teardown, and a conformance or docs job that fleetd starts at boot can still run at that time. A test that fails only under load: run it again alone before you call it a fault.
