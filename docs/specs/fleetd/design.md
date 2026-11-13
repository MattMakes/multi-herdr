# Herdr Agent Fleet: System Design

Nov 10, 2026 · @Matthew

Amended 2026-11-12 after a design review. See Amendments.

## Summary

Make correctness a property of code and files, not of model memory: a supervised daemon owns the ledger and acts on facts it can check, workers are bound to tasks before they start and hold them only by earned leases, and a task is done only when its checks pass on disk.

Your multi-herdr (`horch`) already has the right parts: recorded PIDs, a supervisor per worker, worktrees per task and finish steps. The failures you see come from five jobs it still leaves to an LLM: creating tasks, claiming them, reporting results, reacting to typed mail and cleaning up worktrees. Each change below takes one of those jobs away from the model, and the AI that is left works on one idea at a time.

**The decisions that matter most**

1. **One supervised, level-triggered control loop.** Replace the lazily started watchdog with `horch fleetd`, which runs under the service manager of the OS (launchd, systemd or a Windows service) through 1 platform layer. Every 5 s it compares the ledger with real processes, herdr panes and files, and fixes the difference itself. From Phase 5 it is the only ledger writer. Before Phase 5, each writer goes through the same `transition()` (see Migration plan). 1 file lock makes sure that only 1 fleetd runs. A pass makes its ledger changes in 1 transaction and does its effects (a process start, a file write, a git ref move) after the commit, and the next pass observes each effect. A missed event is caught on the next pass, and a hung daemon is restarted.
2. **Push dispatch with leases, not claims.** fleetd binds a task to a worker and writes the task file before the agent starts, so there is nothing to find. The task is held by a lease that renews only on real activity. An exec worker that waits on a question exits and its task waits in `blocked_on_user` with no attempt burned, and an interactive persona's waits are capped at an hour, so an idle worker loses its task in minutes, not days. A usage limit is not a stall: the task moves to its fallback persona or waits in `waiting_quota`.
3. **Done is a fact on disk.** A stop hook will not let an agent end its turn without writing `result.json`, exec mode turns "stopped" into "process exited", and only the verifier moves a task to done: its acceptance checks, then today's review scripts for code, or a paired judge task for a judgment task.
4. **A program dispatches ideas; an AI leads each one.** `horch idea add` drops a rough idea into a queue. A deterministic dispatcher inside fleetd moves each idea through a fixed state machine and keeps one sub-orchestrator pane per feature. That AI plans 1 idea at a time: it researches the idea, interviews you in its own pane, writes its assumptions down and waits for your quick approval before it writes the spec and tickets. An idea belongs to exactly 1 feature, and the ledger records it. No task exists outside an approved, validated plan; a ticket you wrote enters through the `--direct` fast lane.
5. **Every worktree is leased before it exists.** fleetd records each worktree in the ledger, tied to its task and owner, before it runs `git worktree add`; workers never create one. A reconciler compares the ledger with `git worktree list`, and anything orphaned is committed to a rescue branch before cleanup, so work you paid for is never lost.
6. **Every task lands on a feature branch through a landing state machine in the ledger, and no branch is lost.** Every branch is a ledger row before it exists, with a landing state, 1 owner and a time limit. fleetd lands ready task branches in bundles behind the fast gate, detects conflicts between features early, and checks the no-loss invariants against git on every pass. Each feature reaches the primary branch through 1 pull request.
7. **There is no spend budget and no cap on parallel work. Opus 5.5 at medium effort is the default worker.** The purpose is to use the subscription usage fully. fleetd paces dispatch to each pool's usage window, and only health guards for disk, load, memory, processes and open files pause new dispatch.

The migration takes nine phases, each gated by a test. Phases 1 to 4 stop most daily stalls and stop losing worktrees.

## Amendments

The table below has 16 decisions. A design review on 2026-11-11 and 2026-11-12 made D1 to D14. The orchestrator made D15 in the DDD round. You made D16 after you read the DDD draft. This document includes all of them.

| # | Decision | Sections changed |
| --- | --- | --- |
| D1 | fleetd supports macOS, Linux and Windows as first-class hosts. All OS-specific code sits behind 1 platform layer with 1 module per OS, so core code has no `cfg(target_os)`. D16 decides the move of the existing OS-specific code. | Summary; Goals, non-goals and defaults; Requirements (PL-1 to PL-4); Architecture and roles (fleetd, wrapper, Platform layer, What each OS provides); Supervision, liveness and reconciliation; Failure modes (FM-11, FM-12); Migration plan (Phase 1); Open questions |
| D2 | A usage limit or a provider outage is not a stall. Every worker persona names a fallback. The task restarts at once on a fallback of another provider, else waits in `waiting_quota` with its lease paused. No attempt is burned. | Summary; Requirements (RR-11); Ledger; Worker profiles and adapters (`fallback`); Supervision, liveness and reconciliation; Failure modes (FM-5); Migration plan (Phase 2) |
| D3 | Everything is an idea. `horch idea add --direct <ticket file>` is a fast lane for a ticket you wrote: no research, no interview, a plan with 1 work task, plus its judge task when the kind is a judgment kind, that still passes plan validation and still needs a check. | Summary; Requirements (FR-10, FR-12); Ideas; Migration plan (Phase 6) |
| D4 | A judgment task gets a structural check and a paired judge in a different session, on a different provider when the roster has one. The judge ranks each miss CRITICAL, HIGH, MEDIUM or LOW, and only CRITICAL blocks. | Summary; Requirements (FR-4, RR-3); Architecture and roles (Verifier, Done for each kind of task); Workspace layout (plan example); Failure modes (FM-9, FM-33) |
| D5 | Features run in parallel by default. Conflicts are found early by trial merges, a conflict stacks the younger feature on the older one with 1 resolver task, feature branches are rebased continuously, and each project declares a fast gate and a full gate. | Features, landing and integration (Between features, evidence); Ledger (`gate_runs`, `trial_merges`); Open questions |
| D6 | Code review keeps today's review scripts unchanged, with their limits. | Architecture and roles (Verifier, Done for each kind of task); Migration plan (Phase 4) |
| D7 | Exec mode is the default for workers. A persona can be marked interactive; sub-orchestrators are interactive. | Goals, non-goals and defaults; Worker profiles and adapters (`mode`); Open questions |
| D8 | No spend budget and no cap on parallel work. Health guards and pacing stay. Opus 5.5 at medium effort is the default, and plan validation checks the model guide. | Summary; Goals (G7) and defaults; Requirements (OR-4, OR-5); Architecture and roles; Ideas; Worker profiles and adapters; Workspace layout; Failure modes (FM-20, FM-21, FM-29); Open questions |
| D9 | Rollout is automatic and immediate. fleetd is a subcommand of the single `horch` binary, `horch self-update` restarts the running daemon, and a phase is on for every user once the tests of its gate pass. | Requirements (OR-6); Migration plan; Failure modes (FM-34) |
| D10 | Every branch is a ledger row before it exists, with a landing state, 1 owner and a time limit. Ready task branches land in bundles behind the fast gate, and no-loss invariants are checked against git on every pass. | Features, landing and integration (Landing, No work is lost); Ledger (`branches`, I8 to I13); Worktrees; Supervision (reconcile pass) |
| D11 | Every idea lands in a feature branch. A feature is 1 sub-orchestrator, 1 feature branch and 1 pull request to the primary branch. Task branches land on the feature branch with no human approval. | Summary; Features, landing and integration (Features); Requirements (FR-2); Goals, non-goals and defaults; Architecture and roles; Ledger (`features`); Ideas |
| D12 | A feature is an intake funnel until its pull request merges. `horch idea add --to <feature>` joins an open feature; with no `--to`, an idea starts a new feature. | Features, landing and integration (Features); Ideas; Ledger (`ideas.feature_id`) |
| D13 | A worker question that nothing approved covers and that changes no assumption is decided by the sub-orchestrator as its own reversible assumption. Only product direction, scope or a trade-off you must own goes to you. | Ideas (question flow); Requirements (RR-12); Ledger (`questions`); Workspace layout; Failure modes (FM-35) |
| D14 | The DDD run does all 9 steps as an auto draft. | No section changed |
| D15 | Decisions of the orchestrator in the DDD round. You can reverse each one. A request carries an idempotency key, and a refusal carries its reason. `reset_at` of a usage limit is optional. `result.json` has a closed list of statuses. A worker question escalates after 15 min with no answer from a running sub-orchestrator. A requeue by you resets the attempt count, and a review keeps its own counter. Supervision and conformance are an operating concern. Each module has 1 owner inside the 1 binary. | Requirements (RR-11, RR-12, RR-18); Architecture and roles (Docs sync, Module owners inside the 1 binary); Workspace layout and file protocol (`result.json`); Worker profiles and adapters; Ideas (the question flow); Supervision, liveness and reconciliation (stall ladder, task state machine); Docs sync and adapter conformance |
| D16 | The 178 existing OS-specific sites in production code move into the platform layer in 1 dedicated phase, Phase P, between Phase 0 and Phase 1. The move changes no behavior. | Summary; Amendments (D1); Goals, non-goals and defaults (Host OS); Requirements (PL-2, PL-6); Architecture and roles (Platform layer, One platform layer for three OSes); Migration plan (Phase 0, Phase P, Phase 1, New work for each phase); Open questions |
| Fix round 1 | A15 to A18 (J1 to J4): hang detection on all 3 OSes; the branch of a `--direct` feature; a `--direct` plan of a judgment kind; the kind `judge`, which needs no judge. | Amendments (D3); Goals, non-goals and defaults; Requirements (FR-4, FR-12, PL-2, PL-5); Architecture and roles (fleetd, Platform layer, Done for each kind of task, Supervision tree, What each OS provides); Workspace layout (plan example); Ledger (`features`); Ideas; Features, landing and integration (Features); Supervision, liveness and reconciliation; Failure modes (FM-12); Migration plan (Phase 0, Phase 1) |
| Revision round 2: A19 to A21 | A19 (D16): the platform layer phase; A20 (discover hotspot H6): a state name is never written without its entity, and the states that share a name; A21: preservation row 06 S14. | Summary; Amendments (D1, D16); Goals, non-goals and defaults (Host OS); Requirements (PL-2, PL-6); Architecture and roles (Platform layer, One platform layer for three OSes); Ledger (States that share a name, I8); Ideas (`integrating`); Features, landing and integration (No work is lost, Between features); Supervision, liveness and reconciliation (reconcile pass); Preservation analysis (06 S14); Migration plan (Phase 0, Phase P, Phase 1, New work for each phase); Open questions |
| Fix round 2: A22 (L1, L2) | A22: a red test of Phase 0 does not block a gate. Phase 0 keeps a list of expected failures; the test of PL-6 compares the result of each test before the move and after it; the decision count of this section agrees with the table. | Amendments (first sentence, D9); Requirements (PL-6); Migration plan (the paragraph before the phases, Phase 0, Phase P) |
| Systems review round: A23 to A25, A29 | A23: long work is a job, and a pass never waits for it; a work budget for each pass; a periodic `git fetch`. A24: an effect is a row first, then the effect after the commit, then the observation; 1 fleetd for each ledger; a contract for each command request; untrusted worker files; 3 kinds of file. A25: 1 fleet clock for each timer. A29: each process starts through 1 launcher that removes the key. | Summary; Requirements (RR-5, RR-8, PL-2, PL-4, PL-5, SR-5 to SR-7); What herdr and the agent CLIs give us; Architecture and roles (fleetd, fleet-run wrapper, Verifier, Landing, Platform layer, Supervision tree, The command channel, One fleetd for each ledger); Workspace layout and file protocol; Ledger (`attempts`, `leases`, `branches`, `gate_runs`, `trial_merges`, `jobs`, `remote_tips`, `pass_cursors`, `questions`, `ingested`, `requests`, `fleetd`, One door, Leases); Worker profiles and adapters; Ideas (waiting on you); Worktrees (rescue); Features, landing and integration; Supervision, liveness and reconciliation; Docs sync and adapter conformance; Preservation analysis; Failure modes (FM-11 to FM-13, FM-15, FM-21, FM-23, FM-24) |

The orchestrator settled some engineering details in the same review: the verifier sandbox (SR-4), the command channel, waits, the `pr` setting, knowledge across ideas, the plan format and the adapters that carry over. The review also added the Preservation analysis and the new Phase 0 tests.

## Goals, non-goals and defaults

The fleet's correctness lives in a small deterministic supervisor and in files on disk, so no listed failure depends on an LLM remembering to do something.

**Goals**

- **G1:** You drop rough ideas into a queue. A program, not an AI, tracks each one through a fixed state machine, and nothing is built until you approve the idea's written assumptions. Two ideas can never be merged by accident.
- **G2:** Any agent CLI (Claude Code, Codex CLI, Antigravity or Gemini CLI, others) can run as a named worker whose model, skills, MCP servers, sandbox and context budget are fully declared in one profile file.
- **G3:** Every task ends in exactly one of two states you can see: verified done, or escalated to you with a reason. Nothing stalls silently.
- **G4:** All hand-off between agents happens through files on one shared workspace. No LLM ever types a task into another agent's terminal.
- **G5:** Each agent works inside a context budget of about 256k tokens, and long work is split or checkpointed rather than left to drift.
- **G6:** No work a worker paid for is lost. Every worktree is known to the ledger, and none is deleted until its changes are saved on a rescue branch.
- **G7:** The fleet uses the subscription usage fully, so none is wasted, and gets as much work done as possible, responsibly. There is no spend budget and no cap on parallel work.

**Non-goals for version 1**

- Running the fleet across several machines. One host and one disk first.
- Replacing herdr's interface. herdr stays the place where you watch panes.
- Agents merging to protected branches or deploying. Integration stops at a branch for you to merge.

**Defaults I picked (change any of them)**

| Decision | Default | Why |
| --- | --- | --- |
| Workspace root | `/fleet` (env `FLEET_ROOT`) on one local disk | Every agent and the supervisor see the same paths. |
| Supervisor language | `Rust, as a horch fleetd subcommand inside horch-core` | Reuses your existing ledger, procid, herdr client and arch tests. Code samples below are pseudocode. |
| Ledger | SQLite in WAL mode, one writing process | Simple, durable, inspectable with any SQLite tool. |
| Host OS | macOS, Linux and Windows, all first-class, behind 1 platform layer | The release pipeline already builds 5 targets for the 3 OSes. Production code of the workspace has no OS-specific branches outside the platform layer, so a change for one OS does not break another. Phase P moves the existing OS-specific code into the layer (D16). |
| Process manager for the supervisor | The service manager of the OS, through the platform layer: launchd on macOS, a systemd user unit on Linux, a Windows service on Windows; restart always and hang detection on every OS: the systemd watchdog on Linux, and `horch fleetd check` every 30 s on macOS and Windows | The thing that watches everything is itself watched. |
| Idea dispatcher | Deterministic code inside fleetd; no AI | Queue order and state changes must not depend on a model remembering. |
| Spend and parallel work | No spend budget. No cap on features, sub-orchestrators or workers. Health guards pause new dispatch only while free disk, machine load, free memory, the count of processes or the count of open files of the user is past a safety floor | The purpose is to use the subscription usage fully. A health guard protects the machine; it is not a cap. |
| Default worker model | Opus 5.5 at medium effort. Higher effort needs a stated reason. A Sonnet task must be marked "no decisions". Fable needs your flag | Best intelligence per cost. Plan validation checks the guide. |
| Sub-orchestrator | Claude Code, Opus 5.5, interactive in its own herdr pane, one per feature, planning 1 idea at a time, a fresh process per phase | You answer its questions in place, and a fresh process per idea removes cross-idea merging. |
| Workers | Exec mode: headless run-to-completion processes shown live in herdr panes. A worker starts, does its task and exits. A persona can be marked interactive | A process that exits cannot sit idle holding a task. |
| Code isolation | One git worktree per task under `/fleet/work/`, created by fleetd from a ledger lease | Workers never collide on files, and no worktree exists that the ledger doesn't know. |
| Timers | Heartbeat every 10 s, lease 120 s, stall alarm at 5 min without progress | Starting points to tune from real runs. |

## Where multi-herdr stands today

horch already records PIDs, checks in, reaps dead workers and isolates worktrees; the failures come from five things it still leaves to an LLM: creating tasks, claiming tasks, reporting results, reading typed mail and cleaning up worktrees.

This is from reading [MattMakes/multi-herdr](https://github.com/MattMakes/multi-herdr) at commit `8e315e0` (Nov 8, 2026), read-only. Paths are in that repo.

| Failure you see | What the code does now | Root cause |
| --- | --- | --- |
| Orchestrator forgets tasks | Tasks exist only if the orchestrator LLM runs `horch ledger add-map`, `add` or `task` (`taskledger/tasks.rs` `add`, `add_many`). Spawning is a separate step. | No record of an idea exists before it becomes tasks, and nothing compares tickets on disk or the brief with ledger rows. |
| Workers don't find work | The worker's first prompt is the constant `CLAIM_TASK` ("Run horch ledger claim", `taskledger_spawn.rs:20`). `pick_task` matches the persona string exactly. A spawn without `--run` joins the planned run, so a one-off in `solo-*` is missed. No match ends the worker as `no_task`. | Pull dispatch: the worker must run a command and match the right run and persona. A sandbox error or trust prompt before `claim` gets one `unclaimed-spawn` wake, ever. |
| Workers claim, then sit idle | `take_task` (`tasks.rs:1119`) is a permanent claim with no expiry. Any `worker_waits` row counts as progress: 24 h for `progress "waiting: ..."`, 7 days for an open question or review. Without a wait, a reap takes about 30 min (3 checks of 600 s), or about 70 min while herdr shows working. An on-screen timer or a growing transcript counts as progress with no cap. | Claims instead of leases, and liveness that measures activity, not outcome. |
| Silent disconnects | Checkins come from the `horch worker` supervisor thread, so they prove the process lives, not that the agent works. Death is seen at the next 600 s check. An agent stuck at a login, rate-limit or error prompt looks alive. | Checks are too slow and do not recognise "alive but stuck". |
| Finished but never reported | Reporting needs the LLM to run `horch ledger done`. No Stop or SessionEnd hook enforces it. An agent that ends its turn without `done` stays working until reaped, and the reap requeues work that may be finished. | "Done" is a model's action, not a fact on disk. |
| Watchdog misses updates | `taskledger_watch.rs` is started lazily by `ensure` after any horch command, restarted only if tasks are running and its checkin is over 20 min old, and has no systemd or launchd unit. Its wakes are one-shot (first stall, suspect, one unclaimed flag, one rewake per board hash) and only go to the orchestrator as typed mail. | Edge-triggered, unsupervised, and its fix depends on the orchestrator LLM reading a NOTE and acting. |
| Stacked ideas merge | One orchestrator conversation (300k window) receives your ideas, worker mail and watchdog wakes interleaved, typed in even while it is busy. One non-solo run per workspace. | No intake queue and no per-idea isolation. |

**One more you described: orphaned worktrees.** Workers create their own worktrees, and a worker that dies leaves one on disk that the orchestrator never hears about. horch has a `worktrees` table, triage and archive refs, but a row is written only when the code path that makes the worktree finishes, and nothing compares the table with `git worktree list`. The root cause is that a worktree's existence is decided by a worker, not recorded before it happens.

**Keep, change, add.** The design below is a set of changes to horch, not a rewrite. The names in the rest of this doc map to horch like this:

| This doc | In horch | Status |
| --- | --- | --- |
| fleetd (control plane) | A new long-lived `horch fleetd`, replacing the lazily started `horch ledger watch` | Change |
| Idea dispatcher | A deterministic loop inside fleetd plus `horch idea add`, replacing the orchestrator conversation | Replace |
| Sub-orchestrator | A new `idea-lead` persona, one interactive pane per feature, driving the existing spec-writer and ticket-writer steps | Add |
| fleet-run wrapper | `horch worker` (`execution/lifecycle.rs` `run_worker`), plus a stop gate and earned leases | Change |
| Ledger | The task ledger (`taskledger/schema.rs`, schema v7), with leases, an ideas table, worktree leases and one writer | Change |
| Worker profile | Teammate frontmatter in `teammates/` (`roster/teammate.rs`), plus a few fields | Change |
| Verifier | The existing finish steps (check, sweeps, land), run by fleetd rather than triggered by the worker | Change |
| Docs sync | `crates/herdr-docs-sync`, widened to every agent CLI | Change |
| PID-exact stop, triage, routing, telemetry, archive refs | Unchanged | Keep |

File paths in later sections use a neutral `/fleet` layout for readability. In horch, `/fleet/ideas/<id>/` is `ai_docs/plans/<run>/`, `/fleet/tasks/` sits under the state root, and the ledger stays at `~/.horch/ledger.sqlite3`.

Likewise, the `fleet` helper commands are new `horch` subcommands: `fleet idea add` is `horch idea add`, `fleet result` is `horch result`, `fleet status` is `horch status`, and so on.

## Requirements

Each requirement has an ID that the failure analysis and the tests point back to; "shall" lines are testable. A "Phase 0 test" is written red in Phase 0. The table "Where each rule becomes green" in the Migration plan gives the 1 phase whose gate makes each requirement green.

**Functional (FR)**

| ID | Requirement | Verified by |
| --- | --- | --- |
| FR-1 | `horch idea add` shall record each idea as one immutable file and one `captured` ledger row, with no AI involved. | Intake test: two ideas added back to back produce two files and two rows. |
| FR-2 | The dispatcher shall be deterministic code that keeps 1 sub-orchestrator for each feature; 1 process never plans 2 ideas at the same time. | Property test over the idea state machine; the ledger shows one idea per sub-orchestrator attempt. |
| FR-3 | A plan shall be committed to the ledger all-or-nothing, and only after it passes schema, DAG and coverage checks. | Plan validator unit tests; a plan missing a requirement is rejected. |
| FR-4 | Every task shall name a worker profile, its `kind`, its inputs, its outputs and a machine-checkable acceptance check. For a judgment kind the check is structural (the file exists, the schema or the required sections are valid), and the plan shall pair the task with a judge task in a different session from the author, on a different provider when the roster has one. The judge task names the judged task in `judges:`, and this is not a normal dependency (see Done for each kind of task). The judgment kinds are `research`, `design`, `docs`, `report` and `creative`. A judge task has the kind `judge`, and it needs no judge task. | Plan schema; plan validation refuses a judgment task with no paired judge task. |
| FR-5 | The supervisor shall assign tasks to workers (push). Workers shall never search for work. | No code path where an agent queries the ledger for open tasks. |
| FR-6 | A worker profile shall declare CLI, model, skills, MCP servers, sandbox, writable paths, context budget, timeouts and restart policy. | Profile schema; the adapter refuses an incomplete profile. |
| FR-7 | All task and result hand-off shall be through files under `FLEET_ROOT`. The only bytes ever sent into a terminal are a fixed doorbell line written by the supervisor's code. | Grep of the code for mux send calls; doorbell text is a constant. |
| FR-8 | A docs-sync job shall fetch each CLI's documentation, record the installed version, and run adapter conformance tests on change. | Sync log and test report per CLI version. |
| FR-9 | Each agent shall run with a context limit of 256k tokens by default, set through the CLI's own setting where one exists. | Conformance test reads back the effective setting. |
| FR-10 | No spec or task shall be written for an idea until you approve its assumptions, or you add the idea yourself with `--direct`, and only a command you run outside any fleet process can record that approval. | A sub-orchestrator that writes `approval.json` or runs `horch idea approve` leaves the idea in `awaiting_approval`. |
| FR-11 | Every worktree shall be leased in the ledger, tied to one task and one owner, before it is created, and only fleetd shall create or remove worktrees. | A worker's own `git worktree add` is flagged on the next pass; `arch_scan` finds no worktree calls outside fleetd. |
| FR-12 | Every task shall belong to an idea. `horch idea add --direct <ticket file>` shall take a ticket you wrote, skip research and the interview, and make a plan with 1 work task, plus its judge task when the kind is a judgment kind, that still passes plan validation and still needs a check. | Intake test: a `--direct` idea produces 1 validated plan with 1 work task, plus its judge task when the kind is a judgment kind; no code path creates a task without an idea. |

**Reliability (RR)**

| ID | Requirement | Verified by |
| --- | --- | --- |
| RR-1 | Liveness shall be judged from outside the model: process state, wrapper heartbeat, CLI hook events and output activity. | Fault injection: kill -STOP a worker; alarm fires within one lease. |
| RR-2 | A task shall be held by a lease with an expiry. An expired lease returns the task to ready or escalates; it never stays assigned. | Property test over the state machine. |
| RR-3 | A task shall reach done only when its result file exists, validates, and its acceptance check passes when run by the supervisor. For a code kind, today's review scripts shall also pass. For a judgment kind, its judge task shall report no open CRITICAL item: the judged task waits in `verifying` after its structural check passes, its judge task starts from the tip of its task branch, and the judged task moves to `done` when its judge task is `done` with no open CRITICAL item. An open CRITICAL item starts a fix round, counted in `fix_rounds` (limit 2), not in `max_attempts`. | Worker that says "done" without artifacts stays not-done; a document with an open CRITICAL item stays not-done. Phase 0 test: a judgment task and its judge task both reach `done` with no change of the plan, and a CRITICAL item gives 1 fix round. |
| RR-4 | The supervisor shall reconcile desired and actual state on a fixed period as well as on file events, so a missed event is caught by the next pass. | Drop all file watch events (inotify on Linux) in a test; tasks still complete. |
| RR-5 | Exactly one process shall write the ledger. Every other component writes files that it ingests. Exactly one fleetd shall run for one ledger: it holds an exclusive file lock for its whole life, and each write transaction checks its generation. | SQLite opened read-only everywhere else. Phase 0 test: a second fleetd started beside a live one exits at once and does no effect. |
| RR-6 | Every retry shall be bounded. After the profile's max attempts, the task is escalated to the user, not retried forever. | State machine test. |
| RR-7 | The supervisor shall survive its own restart with no lost or duplicated task, by rebuilding state from the ledger and the files. | Kill the supervisor mid-run; compare outcomes with a clean run. |
| RR-8 | Each write of a message or a snapshot in the protocol shall be atomic (write to a temp name, fsync, rename), a log shall be appended by whole lines, and ingestion shall be idempotent. Each effect outside the ledger (a process start or stop, a file write, a git ref move) shall be a committed row in an intended state before it occurs, shall be keyed by the id of that row, and shall be safe to repeat. Each command request shall carry a `request_id`, and a repeated `request_id` shall get the stored result with no second effect. | Crash test during writes. Phase 0 test: fleetd is killed after the commit of a launch and before the launch, and after the launch; in each case exactly 1 process of the attempt runs after the restart. Phase 0 test: a request sent 2 times with 1 `request_id` has 1 effect. |
| RR-9 | No worktree shall be removed until its commits and its uncommitted and untracked files are saved on a rescue ref, and that ref is read back and shown to hold them. | Kill a worker with dirty and untracked files; after cleanup the rescue ref contains every one. |
| RR-10 | The reconciler shall compare the worktree ledger with `git worktree list` and the work directory on every pass, and adopt or rescue anything on disk that the ledger does not own. | Create a worktree by hand; it is adopted or rescued within one pass. |
| RR-11 | A worker that shows a usage limit or a provider outage shall not be treated as stuck, and shall not burn an attempt. If its persona's fallback is on another provider, fleetd shall restart the task at once on a new worker of the fallback persona, on the same worktree. Otherwise the task shall wait in `waiting_quota` with its lease paused until the pool resets. The reset time `reset_at` of a usage limit is optional: when it is absent, fleetd shall wait until the pool probe reports ok. If both pools are limited, the task shall wait. | Phase 0 test: a usage-limited worker is not reaped, and its attempt count does not change. |
| RR-12 | A worker question shall be a ledger row with an owner, a state and a time limit. fleetd shall start the owner on each pass until an answer is recorded, resume the worker with the answer or start a fresh worker from the question, the answer and the committed work, and escalate the question to you past its time limit: 15 min with no answer from a running sub-orchestrator, a starting value. The wait for you shall have no limit. | Phase 0 test: a question whose owner is not running is answered after fleetd starts it; an unanswered question reaches `needs-you/` at its limit. |
| RR-13 | `horch worktree drop` and each cleanup shall refuse a dirty tree, a branch that a `git stash` entry names, unpublished unique commits and a HEAD that moved during the checks (preservation row 04 W6). | Phase 3 test: a drop for each of the 4 conditions is refused, and the tree stays. |
| RR-14 | A task that cannot land shall park with its worktree, its dependents shall stay blocked, and a requeue shall resume it on the same worktree (04 L5). | Phase 3 test: a parked task keeps its tree, its dependent stays `pending`, and its requeue runs in the same tree. |
| RR-15 | `horch ledger unfinished`, `reopen` and `dismiss` shall keep a record of work that never landed, and each finding shall be decided 1 time (04 L9). | Phase 3 test: an unlanded tree is listed until you reopen or dismiss it, and a dismissed finding is not listed again. |
| RR-16 | Archive, trash and rescue refs shall only move forward. Each ref write shall be read back before a folder is removed. Trash refs shall expire after 30 days (04 W8). | Phase 3 test: a second rescue of 1 tree does not overwrite the first ref, and a failed read-back keeps the folder. |
| RR-17 | Triage shall commit modified and untracked files before ARCHIVE or DELETE, shall refuse a dirty or unique detached HEAD, and shall delete a branch only when the trash ref holds its tip (04 W9). | Phase 3 test: triage of a tree with modified and untracked files saves each one, and a unique detached HEAD is refused. |
| RR-18 | A requeue by you shall set the attempt count of the task to 0 (04 L8). A requeue by a review shall keep its own counter (05 V-14). | Phase 3 test: a task escalated after its last attempt runs again after your requeue, and a review requeue does not change its attempt count. |

**Safety and isolation (SR)**

| ID | Requirement | Verified by |
| --- | --- | --- |
| SR-1 | Each worker shall write only to its task directory and its worktree, plus paths its profile names. | Sandbox conformance test per CLI. |
| SR-2 | Secrets shall reach agents only through env vars named in the profile, never through task files. | Lint on task files. |
| SR-3 | No agent shall push to a protected branch, deploy, or message anyone outside the fleet. | Git hooks and profile defaults. |
| SR-4 | An acceptance command shall run in the sandbox profile of the task's worker, never with the rights of fleetd. | A check that writes outside the worker's allowed paths is refused. |
| SR-5 | The command channel shall accept only the user that owns the ledger. The socket or named pipe shall check the peer user through the platform layer. The request folder shall have mode 0700 and each request file 0600, or an equal access rule on Windows, and fleetd shall check the owner of a file before it reads it. `horch idea approve`, `abandon` and `horch idea add --direct` shall refuse to run inside a fleet attempt. | Phase 0 test: a request from another user, and a request file of another owner or with a wider mode, are refused with a reason. |
| SR-6 | fleetd shall not trust a file that a worker writes. It shall read a file only when it is a regular file, opened without following a link, under the size limit of its type, with its real path inside its own attempt folder, and with the task id and attempt id of its path. A file that fails is recorded as rejected and is not read again. | Phase 0 test: a `result.json` that is a symlink, a `progress.jsonl` over its limit, and a result with the attempt id of another attempt are each rejected, and fleetd stays healthy. |
| SR-7 | Each process that fleetd, the fleet-run wrapper, a job, docs sync or the conformance suite starts shall start through 1 launcher function. The launcher shall apply the environment rule of the profile, and shall always remove `ANTHROPIC_API_KEY`. fleetd shall not use the native agent resume of herdr. | An `arch_scan` rule finds no direct process start in production code outside the launcher. Phase 0 test: with the forbidden name set in the environment of fleetd, no child of a worker, a job, a verifier check, docs sync or a conformance launch has it. |

**Platform (PL)**

| ID | Requirement | Verified by |
| --- | --- | --- |
| PL-1 | fleetd shall run on macOS, Linux and Windows as first-class hosts. | CI runs the test suite on all 3 OSes. |
| PL-2 | Production code of the workspace shall have no `cfg(target_os)`, `cfg(unix)`, `cfg(windows)` or `cfg(target_family)`, and no `cfg!` of these, outside the platform layer. It shall call 1 platform interface, and each OS shall have 1 module that implements it. From Phase P, the interface covers the concerns of the existing code: process identity and liveness (alive, start time, parent, children); signals and stop; process groups (stop and kill of a group); file locks; file permissions; paths and home directories; symlinks and links; shell and quoting; local sockets and pipes; detach of a child process; the machine probe (free disk, memory, load, the count of processes and of open files of the user); the sandbox host; and the small rest (for example archive format, file identity, user PATH). Phase 1 adds the fleetd concerns: service install and restart, hang detection, sleep and wake, file watching, the fleetd lock, and the transport of the fleetd command channel with its peer-user check. Test code is not in scope. | From Phase P, an `arch_scan` rule finds no OS `cfg` in production code outside the platform layer. The rule names each site that cannot call the layer (a build script) by path, with its reason. |
| PL-3 | Where an OS has no equivalent, its module shall return an explicit "not supported" value, and core code shall decide what to do with it. Nothing shall fail silently. | Unit test per module for each concern it does not support. |
| PL-4 | Each operational timer shall be a deadline on the fleet clock, which does not run while the machine sleeps or while fleetd is down: the lease, each rung of the stall ladder, the start timeout, each retry backoff, the wait limits (`max_wait_s` and the 30 min stop of an idle sub-orchestrator), the question limit, the `due_at` of each row, and the time limit of a job. 2 timers shall use awake time from the platform layer: the hang check of fleetd, and `max_minutes` of the fleet-run wrapper. These shall use calendar time: the 7 day park, the reminders, the retention periods, the daily docs sync and the usage windows of a provider. A sleep shall not expire a lease or fire a deadline. | Phase 0 test: a machine sleep in the middle of an attempt does not expire its lease. Phase 0 test: a sleep past the deadline of a branch, a question, a stall rung and a retry backoff fires none of them on wake; each fires after the rest of its fleet time. |
| PL-5 | A hung fleetd shall be restarted on all 3 OSes. On each pass, fleetd shall write a heartbeat: the time of its last full pass. On Linux the systemd watchdog restarts it. On macOS a launchd timer job, and on Windows a scheduled task, shall run `horch fleetd check` every 30 s. `horch fleetd check` shall restart the service when the heartbeat is older than 30 s of awake time. Each `horch` CLI command shall make the same check before it sends its request. The heartbeat shall not depend on a job: a pass never waits for a gate, a check, a trial merge, a rebase or a git network call, and a pass ends before the hang limit by its work budget. | Phase 0 test on each OS: a hung fleetd is restarted. Phase 0 test: a fast gate and a full gate of 5 min each run while fleetd stays healthy and is not restarted. |
| PL-6 | The move of the existing OS-specific code into the platform layer in Phase P shall change no behavior. A gap of 1 OS that exists today stays a gap, and its OS module states it as an explicit "not supported" value. | On each of macOS, Linux and Windows, each test has the same result before the move and after it. A test that passes before the move passes after it. A test on the list of expected failures still fails. The `arch_scan` rule of PL-2 passes. |

**Observability (OR)**

| ID | Requirement | Verified by |
| --- | --- | --- |
| OR-1 | `fleet status` shall show every idea, task, lease, worker and its last signal in one view. It shall show each hold on dispatch with its cause: a health floor, pacing of a pool, an unhealthy profile, a git job that holds the repository, a missing fast gate. fleetd shall record an event when a global hold starts and when it ends. | Manual check. Phase 0 test: a ready task that does not run shows the cause of its hold. |
| OR-2 | Every state change shall be an appended event with a cause, so any outcome can be explained after the fact. | Event table audit. |
| OR-3 | Anything that needs you shall land in one place (`/fleet/needs-you/`), in `horch ideas`, and as a herdr notification. Each item shall have a stable key (its kind and its row) and a state (`open`, `resolved`, `superseded`). A producer shall write by key, so 1 thing has 1 item. Only `open` items shall be shown, and an item that is not open shall be removed after 30 days. | Escalation test. Phase 0 test: the same escalation raised on 2 passes gives 1 item, and its item is `resolved` when its cause ends. |
| OR-4 | fleetd shall read the usage window of each pool and dispatch so that the usage is consumed by the reset. | Pacing test with a fake pool window. |
| OR-5 | fleetd shall pause new dispatch only while free disk, machine load, free memory, the count of processes or the count of open files of the user is past a safety floor, read from the machine probe of the platform layer, and shall resume by itself. A machine probe that returns unknown (Windows today) shall be reported, not read as healthy or unhealthy. | Fault injection: a low-disk probe pauses dispatch; a normal probe resumes it. |
| OR-6 | `horch self-update` shall restart the running daemon on the new binary at once, and a CLI that finds an older daemon shall restart it. Schema migration shall be forward-only at daemon start; a newer schema is refused, a backup is written before each migration, and a development build does not migrate the shared ledger. | Phase 0 test: a daemon restart on a new binary in the middle of a run loses no task. |

## What herdr and the agent CLIs give us

herdr already supplies panes, named agents, a status signal and a scriptable socket, so fleetd should drive herdr through its socket API and treat herdr's status as one input among several, not as the truth.

### herdr

[herdr](https://github.com/herdrdev/herdr) (herdrdev/herdr, Rust, version 0.9.5 in its Cargo.toml as read on 2026-11-10) is its own terminal multiplexer built for coding agents. It is not a tmux wrapper. A background server owns the terminals and clients attach to it. Facts below come from its [README](https://raw.githubusercontent.com/herdrdev/herdr/master/README.md), [bundled skill](https://raw.githubusercontent.com/herdrdev/herdr/master/skills/herdr/SKILL.md), [CLI reference](https://herdr.dev/docs/cli-reference/), [socket API](https://herdr.dev/docs/socket-api/), [integrations](https://herdr.dev/docs/integrations/), [agents](https://herdr.dev/docs/agents/) and [session state](https://herdr.dev/docs/session-state/) pages.

| Need | herdr provides | What it means for the design |
| --- | --- | --- |
| Make a pane | `herdr pane split --direction right\|down --cwd PATH --env KEY=VALUE --no-focus`, returns a pane ID such as `w1:p1` | fleetd creates panes with the attempt's env baked in. `--env` exists only on pane, tab and workspace creation. |
| Start a named agent | `herdr agent start <name> --kind claude\|codex\|gemini\|agy\|... --pane ID -- <native args>`; names match `[a-z][a-z0-9_-]{0,31}` | Worker names map to herdr agent names. Model, MCP and skills go in the native args after `--` and in env vars. |
| Send input | `agent prompt <target> <text> [--wait]`, `pane send-text`, `pane run` | Docs warn a timeout "does not prove the prompt was never delivered". So the only input fleetd sends is the constant doorbell, which is safe to deliver twice. |
| Read output | `pane read --source recent --lines N`, `pane wait-output --regex` | Used for stall evidence and the transcript tail in `fleet status`, never as the result channel. herdr's own skill says to have agents write replies to files when reads fail. |
| Status | `idle`, `working`, `blocked`, `done`, `unknown` from foreground process, screen manifests and integrations; `agent explain --json` shows why | Good signal for "blocked on an approval dialog". Screen-derived, so it can be wrong; docs say `unknown` "does not prove completion" and Antigravity counts as idle at its prompt even with background work. |
| Push our own state | `herdr pane report-agent <pane> --source fleet --state ... --seq N` | The wrapper reports the authoritative state so the herdr sidebar shows the same truth as the ledger. |
| Events | Socket `events.subscribe` with `pane.agent_status_changed`, `pane.output_matched`; on overload the server sends `events_lost` and closes | Edge events can be lost by design. fleetd uses them to wake early, then re-reads `session.snapshot` every pass (level-triggered). |
| Persistence | Detach keeps processes. A herdr server restart restores layout but not processes; agents come back only through an integration's resume command | After a herdr restart every live attempt must be re-checked. The reconciler handles this the same way as any dead process. fleetd does not use the native agent resume of herdr: it starts each attempt again through the launcher (SR-7). |
| Scripting | Unix socket at `~/.config/herdr/herdr.sock` (per session under `sessions/<name>/`), newline-delimited JSON; `herdr api schema --json` dumps the schema | fleetd uses a dedicated named session, `herdr --session fleet`, and talks JSON to it. |

Not confirmed: whether herdr can set MCP servers, skills or context limits per agent (I found no per-agent config schema), the manifest TOML fields, and an event-subscription CLI (the automation docs say subscription is socket-only). The design keeps all of that inside our own adapter so herdr only has to make panes, start processes, report status and read screens.

### The agent CLIs

The OpenCode, pi and Prime adapters of today carry over with Claude Code, Codex and Antigravity (`preservation-06-wiring-and-platform.md`, row S15). The table covers the 3 CLIs that the design changes most.

All three CLIs can be fully configured per process and all three have a headless mode and hooks, but only Claude Code and Codex expose a compaction threshold, and every one of them has a trust or approval prompt that can freeze an unattended pane unless it is pre-seeded.

On 2026-11-11 this machine ran herdr 0.9.4-preview, Claude Code 2.1.322 and Codex CLI 0.167.0. The table below was written from the documents of the versions it names, and Phase 7 verifies it again.

Versions as documented on 2026-11-10: Claude Code docs up to v2.1.290 ([CLI reference](https://code.claude.com/docs/en/cli-reference.md), [env vars](https://code.claude.com/docs/en/env-vars.md), [hooks](https://code.claude.com/docs/en/hooks.md), [sandboxing](https://code.claude.com/docs/en/sandboxing.md)); Codex CLI 0.161.0 ([config reference](https://learn.chatgpt.com/docs/config-file/config-reference.md), [non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode.md), [sandboxing](https://learn.chatgpt.com/docs/sandboxing.md), [hooks](https://learn.chatgpt.com/docs/hooks.md)); Antigravity CLI `agy` 1.4.4+, Google's successor to Gemini CLI since May 2026 ([CLI reference](https://antigravity.google/docs/cli/reference), [headless](https://antigravity.google/docs/cli/headless), [hooks](https://antigravity.google/docs/hooks?tab=cli), [sandbox](https://antigravity.google/docs/sandbox?tab=cli)).

| Concern | Claude Code (`claude`) | Codex CLI (`codex`) | Antigravity CLI (`agy`) |
| --- | --- | --- | --- |
| Private config home | `CLAUDE_CONFIG_DIR=<dir>` | `CODEX_HOME=<dir>` (must exist) | No env var documented; override `HOME` (unconfirmed) |
| Exact MCP set | `--strict-mcp-config --mcp-config w.json` | Only what that `CODEX_HOME/config.toml` lists, or `exec --ignore-user-config` plus `-c mcp_servers...` | Only what `~/.gemini/config/mcp_config.json` and `<ws>/.agents/mcp_config.json` list; `enabledTools` per server |
| Skills | `$CLAUDE_CONFIG_DIR/skills`, `.claude/skills`; `skillOverrides`, `disableBundledSkills` | `.agents/skills`, `$HOME/.agents/skills`; `[[skills.config]] enabled=false` | `.agents/skills`, `~/.gemini/config/skills`; plugins |
| Instructions | `CLAUDE.md`, `AGENTS.md` (v2.1.277+), `--append-system-prompt-file` | `AGENTS.md` chain, `developer_instructions`, `model_instructions_file` | `AGENTS.md` or `GEMINI.md`; `.agents/rules/*.md` need `trigger` frontmatter or are silently dropped |
| Sandbox and approvals | `--permission-mode dontAsk\|acceptEdits\|...`, `sandbox.filesystem.allowWrite`, `sandbox.network.allowedDomains` | `--sandbox workspace-write`, `-a never`; `--full-auto` is removed | `--mode auto`, `toolPermission`, `permissions.allow` |
| Write to a shared folder | `--add-dir /fleet/tasks/T` | `--add-dir` or `-c 'sandbox_workspace_write.writable_roots=[...]'` | `--add-dir` plus `permissions.allow: ["write_file(/fleet/...)"]` |
| Network inside sandbox | `sandbox.network.allowedDomains` | `sandbox_workspace_write.network_access=true` | `read_url(host)` entries; off by default |
| Compact at 256k | `CLAUDE_CODE_AUTO_COMPACT_WINDOW=256000` (plain integer; `256k` is read as 256) or `--autocompact 256000` | `-c model_auto_compact_token_limit=256000` | Not configurable. The wrapper tracks usage and restarts with a handoff |
| Headless | `claude -p --output-format stream-json --verbose --include-hook-events` | `codex exec --json -o last.txt` | `agy -p --output-format stream-json` |
| Resume | `--resume <id>`, `--session-id <uuid>` | `codex exec resume <id>` | `--conversation <id>` |
| Hooks for liveness | 33 events incl. `PreToolUse`, `PostToolUse`, `Notification`, `PermissionRequest`, `Stop`, `PreCompact`, `SessionEnd`; `http` hook type posts JSON | `PreToolUse`, `PostToolUse`, `PermissionRequest`, `Stop`, `PreCompact`, `SessionEnd`; user hooks must be trusted via `/hooks` first | `PreToolUse`, `PostToolUse`, `Stop` (with `fullyIdle`); command hooks only |
| Machine-readable docs | [llms.txt](https://code.claude.com/docs/llms.txt), `.md` per page | [llms.txt and llms-full.txt](https://learn.chatgpt.com/docs/llms.txt), repo `docs/` | [llms.txt](https://antigravity.google/llms.txt) |

**Gotchas the adapters must handle**

- **Codex sandbox.** `codex exec` defaults to `read-only`. A worker that must write its result outside its cwd needs `--sandbox workspace-write` plus the task folder in `writable_roots`, or it fails quietly. On Linux the sandbox is bubblewrap, which must be installed.
- **Codex hook trust.** Hooks you add yourself do not run until trusted through `/hooks`. Use admin-managed or requirements config for workers, and have the conformance test prove a hook fires (how to pre-trust is unconfirmed).
- **Antigravity soft denials.** In headless mode a tool call that needs approval is denied and the run still exits 0. The adapter must read the stream for denials, not trust the exit code.
- **Antigravity idle.** herdr reports `agy` idle once it returns to its prompt even if background work continues, so exec mode is the safer default for it.
- **Trust prompts.** Claude workspace trust, Codex project trust and Antigravity `trustedWorkspaces` must be pre-seeded in each private config home, or the first launch blocks on a dialog.
- **No API key for Claude.** Your repo's rule stands: every `claude` launch strips `ANTHROPIC_API_KEY` and uses your subscription login. Every process starts through 1 launcher that removes it (SR-7). A private `CLAUDE_CONFIG_DIR` therefore needs its login seeded, which the docs-sync conformance test checks.

## Architecture and roles

The design splits the system into a deterministic control plane (fleetd with its idea dispatcher, and the per-attempt wrapper) and disposable LLM processes that only read and write files, so models do judgment and code does bookkeeping.

**The components and the files between them** (paths from Workspace layout and file protocol):

- You to fleetd: `horch` commands over the command channel, or a request file in `/fleet/run/requests/`. `horch idea add` writes `ideas/I-NNNN/idea.md`.
- Sub-orchestrator to fleetd: `interview.md`, `assumptions.md`, `spec.md`, `requirements.yaml` and `plan.yaml` in `ideas/I-NNNN/`.
- fleetd to the sub-orchestrator: `approval.json` and `plan.verdict.json` in `ideas/I-NNNN/`, and the worker questions of its feature.
- fleetd to a worker: `task.yaml` and `attempts/A<n>/inbox/START.md` in `tasks/T-NNNN-NN/`, and the worktree `work/T-NNNN-NN/`.
- Worker to fleetd: `outbox/result.json`, `outbox/progress.jsonl` and `outbox/questions/` of its attempt.
- fleet-run wrapper to fleetd: `heartbeat.json` and `transcript.jsonl` of the attempt.
- Verifier to fleetd: `verdict.json` of the task.
- fleetd to you: `needs-you/` and herdr notifications.

Sub-orchestrators and workers never touch the ledger, each other or git worktrees directly; fleetd is the only path between them, and it acts on files, your commands and process facts.

| Component | What it is | Owns | Never does |
| --- | --- | --- | --- |
| **fleetd** | One long-running `horch fleetd` process under the service manager of the OS (launchd, systemd or a Windows service), installed and restarted through the platform layer. On Linux: a systemd user unit with `Restart=always` and `WatchdogSec=30`. On macOS a launchd timer job, and on Windows a scheduled task, runs `horch fleetd check` every 30 s. The control plane. Exactly 1 fleetd runs for 1 ledger: it holds an exclusive file lock through the platform layer for its whole life (see One fleetd for each ledger). | The ledger (sole writer from Phase 5), the idea dispatcher, plan validation and commit, task dispatch, leases, worktree leases and rescue, the reconcile loop, the jobs (each piece of work that can take more than 1 s: a task check, a gate, a trial merge, a rebase, a rescue, a `git fetch`, a `git push`), acceptance checks, escalation, the herdr adapter. | Calling an LLM. It is plain code, so its behaviour is testable and repeatable. |
| **Idea dispatcher** | Code inside fleetd: the idea state machine. | Moving each idea to its next state when the required files validate, keeping one sub-orchestrator pane per feature, recording your approvals. | Judging an idea's content. It only checks that files exist, validate and carry your approval. |
| **Sub-orchestrator** | Claude Code, Opus 5.5, interactive in herdr pane `idea-NNNN`, one per feature, a fresh process for each phase of one idea, planning 1 idea at a time. | Researching the idea, interviewing you, writing `assumptions.md`, then `spec.md`, `requirements.yaml` and `plan.yaml`; answering its workers' questions; replanning a failed task. | Seeing another idea, approving its own assumptions, creating tasks or worktrees directly, or typing into another pane. |
| **fleet-run wrapper** | A small program that every agent process is launched through, one per attempt. | Building the attempt's private config home from the profile, starting the CLI in its own process group through the launcher (SR-7), writing `heartbeat.json`, mapping CLI events and hooks to agent state, reporting state to herdr, enforcing the time limit `max_minutes` (a hang guard, counted in awake time from the platform layer, so it works while fleetd is down and a sleep does not use it up) and the 256k context limit, writing `exit.json`. | Deciding outcomes. It reports facts; fleetd decides. |
| **Worker** | Any agent CLI (Claude Code, Codex, Antigravity or Gemini CLI) started by the wrapper with a named profile. | Doing one task attempt in the worktree fleetd leased to it and writing `outbox/result.json`. | Finding work, touching the ledger, creating or removing worktrees, or working on more than one task. |
| **Verifier** | Code inside fleetd that queues each task's check as a job. The job runs the check in the worktree, in its own process outside the reconcile pass, with today's check timeout as its time limit (`preservation-05-contracts-and-values.md`, row V-10). The acceptance commands run in the sandbox profile of the task's worker, never with the rights of fleetd. The check job starts through the launcher (SR-7), so it never has `ANTHROPIC_API_KEY`. For a code kind, today's review scripts then run unchanged. For a judgment kind, the check is structural, and a paired judge task reviews the content (see Done for each kind of task). | Writing `verdict.json` and moving the task to `done` or back to `ready`. | Trusting a worker's own claim of success. |
| **Landing** | Code inside fleetd: the landing state machine, bundles, trial merges and the 2 gates (see Features, landing and integration). The build of a bundle candidate, each gate, each trial merge, each rebase, each `git fetch` and each `git push` is a job. A pass queues the jobs that are due and acts on the jobs that ended. It never waits for one. | Landing task branches on their feature branch, rebasing feature branches, opening resolver and triage tasks, running the full gate, pushing the feature branch for its pull request. | Merging to the primary branch, unless the project setting says so. By default you merge. |
| **Docs sync** | A `horch` subcommand that fleetd schedules as a job (daily, plus on any CLI version change). | Mirroring each CLI's docs, recording versions, running adapter conformance tests. | Changing a profile by itself. It opens a task when something drifts. |
| **Platform layer** | 1 interface inside horch with 1 module per OS: the leaf crate `crates/horch-platform`, with the modules `mac.rs`, `linux.rs` and `windows.rs`. It depends on no other crate of the workspace, and each crate of the workspace calls it. It owns the process start. Only the launcher calls it (SR-7). | From Phase P: process identity and liveness (alive, start time, parent, children), signals and stop, process groups (stop and kill of a group), file locks, file permissions, paths and home directories, symlinks and links, shell and quoting, local sockets and pipes, detach of a child process, the machine probe (free disk, memory, load, the count of processes and of open files of the user), the sandbox host, and the small rest (for example archive format, file identity, user PATH). From Phase 1: service install and restart, hang detection, sleep and wake, file watching, the fleetd lock (an exclusive file lock held for the whole life of fleetd), and the transport of the fleetd command channel (a Unix socket, or a named pipe on Windows) with its peer-user check. | Failing silently. Where an OS has no equivalent, the module returns an explicit "not supported" value, and core code decides what to do. |
| **herdr** | The pane runtime, in a dedicated session `herdr --session fleet`, also under the service manager of the OS. | Terminals, agent names, screen-based status, the sidebar you watch. | Holding any state fleetd depends on. If herdr restarts, the reconciler rebuilds from the ledger and files. |

**Module owners inside the 1 binary.** fleetd, the wrapper and the `horch` commands are 1 binary. Each concern has 1 owner module, and an `arch_scan` rule enforces the module boundaries.

- The fleet-run wrapper belongs to worker-profiles, with the environment rule and the health of each profile.
- Plan validation is a module of ideas with its own rules: coverage, the DAG, judge pairing, the model guide and the declared gates.
- The needs-you list belongs to ideas.
- git has 2 callers. Worktree-custody owns worktrees and rescue refs. Integration owns branches and merges.
- Task-execution and integration change together. A change of one plans its change of the other.
- Docs sync is a `horch` subcommand that fleetd schedules.
- Supervision and conformance are an operating concern outside the flow of 1 idea. The reconcile pass supervises every phase of each idea, each landing row and each question.

**Done for each kind of task.** Each task declares a `kind`. Code kinds are `feature`, `quality`, `refactor` and `security`. Judgment kinds are `research`, `design`, `docs`, `report` and `creative`. A judge task has the kind `judge`. It is not a judgment kind. A resolver task has a code kind, and it follows the code path.

- **Code kind.** The task's check runs. Then today's review scripts run unchanged: the code-sweep rounds, the findings F<n>, the answers file, the waivers and the verification profiles. Their limits stay: at most 2 requeues, and review rounds of 1 (light) and 3 (full) (`preservation-05-contracts-and-values.md`, row V-14).
- **Judgment kind.** The verifier runs a structural check: the file exists, and the schema or the required sections are valid. Plan validation refuses a plan in which a task of a judgment kind (`research`, `design`, `docs`, `report` or `creative`) has no paired judge task. The judge runs in a different session from the author, on a different provider when the roster has one. It ranks each miss:

| Rank | Effect |
| --- | --- |
| CRITICAL | Blocks. A fresh session, not the author and not the judge, fixes it: a new attempt of the judged task. Then a fresh judge checks only the earlier CRITICAL items and the change: a new attempt of the judge task. This is a fix round. |
| HIGH | Does not block alone. The sub-orchestrator of the feature fixes it or dismisses it with a written reason. |
| MEDIUM, LOW | Never block. |

"Good enough" passes. The second judge cannot raise new MEDIUM or LOW items; this is the guard against gold-plating. After 2 fix rounds, a result that still has a CRITICAL item goes to you.

**The judge pair.** A judge task depends on the task it judges in another way than a normal `after:` dependency:

1. The judged task writes its result. The verifier runs its structural check. When the check passes, the judged task stays in `verifying`, and its task branch stays in `building`.
2. The dependency of the judge task is now met. Its worktree starts from the tip of the task branch of the judged task, so the judge reads the result before it lands.
3. The judge task writes its verdict file, and the verifier runs its structural check. Its task branch holds only the verdict file on top of the judged branch, and it stays in `building` while the judged task is not `done`.
4. With no open CRITICAL item, the judge task moves to `done`. Then the judged task moves to `done`, and its task branch moves to `ready`. Then the task branch of the judge task moves to `ready`, and it lands after the judged branch.
5. With an open CRITICAL item, the attempt of the judge task ends with the outcome `critical_open`. It is not a failure and burns no attempt. fleetd starts a fix round. It needs no change of the plan. fleetd adds 1 to `fix_rounds` of the judged task, moves the judged task from `verifying` to `ready` for its fix attempt, and moves the judge task from `verifying` to `pending`. The fix attempt runs in a fresh session, in the same worktree, with the verdict file as input. It does not count in `max_attempts`.
6. The dependency of the judge task is met again when the fix attempt of the judged task passes its structural check. fleetd writes a rescue ref of the judge branch and moves it onto the new tip of the judged branch. Then the judge task runs its next attempt in a fresh session. Its scope is the earlier CRITICAL items and the change. Then step 3 runs again.
7. After 2 fix rounds with an open CRITICAL item, the judged task and the judge task both move to `escalated`. So neither task leaves `done`.

- **Judge kind.** A task of kind `judge` needs no judge task. This is the explicit end of the chain. The verifier runs a structural check on its verdict file: the file exists, it has 1 verdict, and each finding has a rank.

**Supervision tree and restart policy (OTP style)**

- The service manager of the OS supervises fleetd and the herdr server: restart always. On each pass, fleetd writes a heartbeat: the time of its last full pass. A pass never waits for a job, so a long gate does not delay the heartbeat. On Linux the systemd watchdog (`WatchdogSec`) restarts a hung fleetd. On macOS a launchd timer job runs `horch fleetd check` every 30 s, and on Windows a scheduled task does the same. `horch fleetd check` restarts the service when the heartbeat is older than 30 s of awake time. Sleep time does not count. Each `horch` CLI command also makes this check before it sends its request. A hung fleetd is killed and restarted on all 3 OSes, and recovers from the ledger (RR-7).
- fleetd supervises attempts: one-for-one, so a crashed worker never affects its siblings. Each profile sets `max_attempts` and backoff. After the last attempt the task moves to `failed`, and the sub-orchestrator of its feature replans it 1 time. Then it moves to `escalated`, and it waits for you.
- The wrapper supervises its CLI process group: when `max_minutes` of awake time pass, it stops the whole group through the platform layer, so no grandchild processes linger. On Linux the module sends SIGINT, waits 20 s, then SIGKILLs the group.

**The command channel.** Each `horch` command that changes state sends a request to fleetd over its channel (a Unix socket, or a named pipe on Windows, through the platform layer). When fleetd is down, the command writes a request file in `/fleet/run/requests/`, and fleetd reads it at start. fleetd stays the only ledger writer.

- **Each request has a contract.** The client makes a `request_id` for each request. It is the idempotency key. A request also has a kind and a payload that never changes. A retry of the same command sends the same `request_id`.
- **1 row for each request.** fleetd writes a `requests` row with the state `pending` when it reads a request. It applies the request and writes its result in 1 transaction: the row goes to `applied` with its result, or to `refused` with its reason. A request with an id that is already applied or refused gets the stored result again and has no second effect. A request with a known id and a different payload hash is refused.
- **A request file is archived after its row.** fleetd moves a request file to `/fleet/run/requests/archive/` only after its `requests` row is committed. A crash before the commit leaves the file in place, and the next pass reads it again. A crash after the commit finds the row by its `request_id`, and the file is only archived.
- **You can ask for the result.** `horch request <request_id>` prints the state and the result of a request at any time. A command that got no reply asks for the result of its `request_id` before it sends again.
- **Only you can send a command.** The channel accepts only the user that owns the ledger. The socket or named pipe checks the peer user through the platform layer. The request folder has mode 0700 and each request file 0600, and fleetd checks the owner and the mode of a file before it reads it. On Windows the platform module sets an equal access rule. A file that fails these checks is refused and recorded with its reason. A worker cannot send an operator command: `horch idea approve`, `abandon` and `horch idea add --direct` refuse to run inside a fleet attempt.

**One fleetd for each ledger.** fleetd takes the exclusive file lock `/fleet/run/fleetd.lock` through the platform layer when it starts, and holds it for its whole life. A second fleetd that cannot take the lock exits at once. The `fleetd` row of the ledger has a generation number, and fleetd adds 1 to it at each start, after it takes the lock. Each write transaction checks that the generation is its own. When it is not, fleetd exits with no effect. A restart by the service manager, by `horch fleetd check` or by `horch self-update` stops the old process by the PID and start time in the `fleetd` row, waits until the lock is free, and only then starts the new one. So 2 fleetd processes never do effects at the same time.

**One platform layer for three OSes.** Production code of the workspace has no `cfg(target_os)`, `cfg(unix)`, `cfg(windows)` or `cfg(target_family)` outside the platform layer. It calls the platform interface. The Linux details in this document (`systemd`, `Restart=always`, `WatchdogSec=30`, `/proc`, `inotify`, bubblewrap, `SIGINT` then `SIGKILL`) are the Linux module's implementation. 4 crates have OS-specific code today: `horch-core`, `horch`, `herdr-install` and `horch-marketplace`. `herdr-install` and `horch-marketplace` do not depend on `horch-core`, so the layer is the leaf crate `crates/horch-platform`, and each crate calls it. Phase P moves all 178 existing OS-specific sites in production code behind the layer, with no change of behavior (D16, `docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6). The 340 `cfg` sites in tests stay.

**What each OS provides** (from `preservation-06-wiring-and-platform.md`, sections 7 and 8)

| Concern | macOS | Linux | Windows |
| --- | --- | --- | --- |
| Service manager, restart always | launchd (`launchctl`, `KeepAlive`) | systemd user unit, `Restart=always` | a Windows service or Task Scheduler. horch has no supervisor code on any OS today |
| Hang detection | a launchd timer job runs `horch fleetd check` every 30 s | `WatchdogSec=30`, `sd_notify` | a scheduled task runs `horch fleetd check` every 30 s |
| Process start time | `proc_pidinfo` (`procid.rs:185`) | `/proc/<pid>/stat` (`procid.rs:233`) | `GetProcessTimes` (`procid.rs:340`) |
| Process alive | `kill -0 pid` | `kill -0 pid` | `OpenProcess` (`fsx.rs:331`) |
| File watching | FSEvents or kqueue | `inotify` | `ReadDirectoryChangesW`. Events are only a speed-up on every OS |
| Sandbox | Seatbelt `sandbox-exec` (`claude.rs:756`) | bubblewrap when installed (`claude.rs:1091`) | not supported: the Claude sandbox is refused (`claude.rs:1102`) |
| Stop a process group | `SIGINT` then `SIGKILL` to the group (`process.rs:62`) | the same, plus pidfd (`procid.rs:127`) | `TerminateProcess` only; a job object is needed for a group. Today only herdr calls use one (`herdr.rs:676`) |
| Command channel transport | Unix socket | Unix socket | named pipe. Today horch's relay and report sockets are Unix-only (`channel.rs:104`) |
| herdr transport | Unix socket | Unix socket | unknown: horch uses only the herdr CLI, so no horch code names it |
| Machine probe | `sysctl`, `vm_stat`, `statvfs`, `getloadavg` | `/proc/meminfo`, `statvfs` | unknown today: all probes return unknown (`machine.rs:322`) |
- fleetd supervises each sub-orchestrator pane like any other agent. If one dies, the reconciler starts a fresh process in the same phase, which resumes from the idea folder, so an interview loses nothing past the last answer recorded with `horch idea note`.

## Workspace layout and file protocol

Every hand-off is a file under `/fleet`, written atomically, of 1 of 3 kinds (an immutable message, a snapshot or an append-only log), and named so that its owner and order are obvious from the path.

**Directory layout**

```
/fleet/
  fleet.toml                  global config (timers, health floors, paths, pass budget, fetch period)
  profiles/                   worker profiles, one YAML per named worker
  prompts/                    idea-lead.md, worker-base.md
  ledger/fleet.db             SQLite, written only by fleetd
  ideas/I-0007/
    idea.md                   immutable copy of the idea (horch idea add)
    amendments/               later additions, one file each
    interview.md              questions and your answers, appended as asked
    assumptions.md            what the sub-orchestrator assumed, for approval
    approval.json             written by fleetd when you run approve
    spec.md                   written only after approval
    requirements.yaml         numbered acceptance list
    plan.yaml                 tasks and dependencies
    plan.verdict.json         fleetd's validation result
  tasks/T-0007-03/
    task.yaml                 written by fleetd from the committed plan
    attempts/A1/
      inbox/START.md          what the worker is told to read first
      outbox/progress.jsonl   worker progress notes (append only)
      outbox/result.json      worker's claimed result
      outbox/questions/       blocking questions, one file each; the worker commits its work and exits
      heartbeat.json          written by the fleet-run wrapper, not the model
      transcript.jsonl        the CLI's own event stream
      home/                   per-attempt config dir for the CLI
    verdict.json              verifier's decision
  work/T-0007-03/             git worktree for this task, leased by fleetd
  rescue/                     one manifest per rescued worktree
  needs-you/                  approvals, questions and escalations, one file each
  docs/<cli>/<version>/       synced CLI documentation
  run/                        pid files, fleetd.lock, mux handles
    requests/                 request files while fleetd is down (mode 0700, each file 0600)
      archive/                request files whose requests row is committed
```

**Knowledge across ideas.** Project facts (today `horch note --fact`) go to every sub-orchestrator. Each feature keeps its own decisions file.

**Plan and ticket formats.** `plan.yaml` replaces today's `MAP.md`. A ticket stays a Markdown file in today's format C1 (`docs/specs/ticket-format.md`), and `plan.yaml` names it.

**Protocol rules**

1. **Atomic writes.** A writer of a message or a snapshot writes `name.tmp`, fsyncs, then renames to `name`. Readers ignore `*.tmp`. A half-written file can never be read. A writer of a log appends whole lines, and fleetd reads only up to the last complete line.
2. **3 kinds of file.** The rule "never edited after it lands" applies to the first kind only.

| Kind | Files | Rule |
| --- | --- | --- |
| Immutable message | `idea.md`, `task.yaml`, `inbox/START.md`, `outbox/result.json`, a question file, `verdict.json`, a request file | Never rewritten. A change is a new attempt directory or a new sequence number. fleetd ingests it 1 time, by path and hash. |
| Snapshot | `heartbeat.json` | Replaced atomically by its writer. fleetd reads the last one and does not ingest it. fleetd uses a snapshot only when its `seq` is higher than the last `seq` it used for that attempt. |
| Append-only log | `outbox/progress.jsonl`, `transcript.jsonl` | Only appended. fleetd reads `progress.jsonl` from the last offset that the ledger recorded in `ingested`, and records the new offset. It reads only the tail of `transcript.jsonl`. |
3. **One writer per file.** The path says who owns it: fleetd owns `task.yaml`, `inbox/` and `verdict.json`; the wrapper owns `heartbeat.json`; the agent owns `outbox/`.
4. **Schemas.** Every file type has a JSON Schema in `/fleet/schemas/`. fleetd rejects a file that fails it and records why.
5. **Agents use the `fleet` helper.** `fleet progress`, `fleet result` and `fleet ask`, plus `horch idea note` for sub-orchestrators, validate and write files for the agent, so a model never hand-assembles JSON. Raw writes still work if they validate.
6. **The only terminal input is a constant.** When a pane must be woken, fleetd's code sends the fixed line `Read inbox/START.md in your task folder and continue.` Everything that changes lives in files.
7. **A file that a worker writes is not trusted.** Before fleetd reads a file of the attempt folder or a request file, it checks: the file is a regular file; fleetd opens it without following a link; its real path is inside the outbox of its own attempt (for `heartbeat.json` and `transcript.jsonl`, inside its own attempt folder; for a request file, inside `/fleet/run/requests/`); its size is under the limit of its type; for a message or a snapshot of an attempt, the task id and the attempt id in the payload equal those of the path. fleetd records a file that fails 1 of these checks as `rejected`, with the reason, in `ingested`, and does not read that path again.

| File | Size limit | Line limit |
| --- | --- | --- |
| `outbox/result.json` | 64 KiB | - |
| A question file | 64 KiB | - |
| A request file | 64 KiB | - |
| `heartbeat.json` | 4 KiB | - |
| `outbox/progress.jsonl` | 8 MiB | 10,000 lines, each at most 4 KiB |
| `transcript.jsonl` | no limit on the file; fleetd reads at most the last 256 KiB | each line at most 1 MiB |

These are starting values in `fleet.toml`, to tune from real runs.

**Example: an idea, as `horch idea add` writes it**

```markdown
---
id: I-0007
created: 2026-11-10T15:02:11-07:00
source: horch idea add
priority: 0
depends_on: []
---
Add retry with backoff to the sync client, and log each retry with the attempt number.
```

**Example: requirements.yaml (written in specifying, after approval)**

```yaml
idea: I-0007
requirements:
  - id: R1
    text: Sync client retries failed requests with exponential backoff, max 5 tries.
  - id: R2
    text: Each retry is logged with attempt number and delay.
  - id: R3
    text: Existing sync tests still pass.
```

**Example: plan.yaml (written in ticketing)**

```yaml
idea: I-0007
repo: /fleet/repos/app
feature: F-0003
base_branch: feature/retry-sync-backoff   # the feature branch; every task of the plan lands here
tasks:
  - key: backoff
    title: Add exponential backoff to SyncClient.request
    ticket: tickets/01-backoff.md   # Markdown, today's format C1
    kind: feature
    profile: coder-codex
    covers: [R1]
    inputs: [src/sync/client.py]
    outputs: [src/sync/client.py, tests/test_backoff.py]
    acceptance:
      - run: pytest tests/test_backoff.py -q
      - run: pytest tests/sync -q
    budget: {max_minutes: 40}
  - key: logging
    title: Log each retry with attempt and delay
    kind: feature
    profile: coder-claude
    covers: [R2]
    after: [backoff]        # met when backoff is done and its task branch is landed on the feature branch
    acceptance:
      - run: pytest tests/test_backoff.py -q -k logs
  - key: sync-suite
    title: Run the existing sync suite on the feature branch
    kind: quality
    profile: coder-codex
    covers: [R3]
    after: [backoff, logging]
    acceptance:
      - run: pytest -q
  - key: retry-doc
    title: Document the retry behavior in docs/sync.md
    kind: docs              # a judgment kind: structural check plus a judge
    profile: writer-claude
    covers: [R2]
    outputs: [docs/sync.md]
    acceptance:
      - sections: [docs/sync.md, "## Retries"]   # structural: the file and the section exist
  - key: retry-doc-judge
    title: Judge docs/sync.md against R1 and R2
    kind: judge             # not a judgment kind: it needs no judge of its own
    judges: retry-doc       # plan validation refuses a judgment task with no judge; met when retry-doc is in verifying and its structural check passed
    profile: judge-codex    # a different session, and a different provider from the author
                            # no after: [retry-doc]; the worktree starts from the tip of the retry-doc task branch, and retry-doc is done only after this verdict
    outputs: [docs/sync.review.md]   # each miss ranked CRITICAL, HIGH, MEDIUM or LOW
```

**Example: task.yaml (written by fleetd)**

```yaml
task: T-0007-02
idea: I-0007
key: logging
attempt: A1
profile: coder-claude
worktree: /fleet/work/T-0007-02
branch: fleet/I-0007/logging
read_first: [/fleet/ideas/I-0007/requirements.yaml]
goal: Log each retry with attempt and delay.
covers: [R2]
acceptance:
  - run: pytest tests/test_backoff.py -q -k logs
lease: {renew_every_s: 10}   # the expiry is a deadline on the fleet clock, kept in the ledger
report_to: /fleet/tasks/T-0007-02/attempts/A1/outbox/
```

**Example: result.json (written by the worker)**

```json
{
  "task": "T-0007-02",
  "attempt": "A1",
  "status": "completed",
  "summary": "Added retry logging in SyncClient._retry with attempt and delay.",
  "commits": ["3f9c2ab"],
  "files_changed": ["src/sync/client.py", "tests/test_backoff.py"],
  "acceptance_self_check": [{"run": "pytest tests/test_backoff.py -q -k logs", "exit": 0}],
  "notes_for_next": "Delay is logged in ms."
}
```

`status` is 1 of the closed list `completed`, `blocked`, `failed`. The schema refuses any other value. The status is the claim of the worker: only the verifier moves a task to `done`.

**Example: heartbeat.json (written by the wrapper every 10 s)**

```json
{
  "task": "T-0007-02", "attempt": "A1", "pid": 48211, "pgid": 48211,
  "mux_pane": "herdr:fleet/w-ada",
  "proc_state": "running",
  "agent_state": "working",
  "last_output_at": "2026-11-10T15:12:03-07:00",
  "last_tool_event": {"at": "2026-11-10T15:11:58-07:00", "name": "Bash"},
  "tokens": {"context": 118400, "limit": 256000},
  "seq": 412, "written_at": "2026-11-10T15:12:05-07:00"
}
```

## Ledger

The ledger is an event log plus derived tables in one SQLite file, written by fleetd alone from Phase 5, and every state change goes through one function that checks the transition is legal.

Today your agents write the ledger directly. That is the root of "orchestrator forgot a task" and "worker took a task and sat on it": the truth depends on a model running the right SQL at the right time. In this design agents only write files; fleetd turns validated files into ledger rows. Every other process, including `horch status`, `horch ideas` and every agent, opens the database read-only.

**Before Phase 5, more than 1 process writes.** fleetd writes from Phase 1, and the `horch` commands of workers and of you write directly until Phase 5 moves them to files and requests. In Phases 1 to 4 this rule holds: each writer calls the same `transition()`, with the same table of allowed transitions; SQLite gives 1 writer at a time; and fleetd treats each row as a fact that it reads again on each pass. No rule of Phases 1 to 4 needs the single writer. Only fleetd checks the generation of the `fleetd` row. From Phase 5, fleetd is the only writer (RR-5).

**Schema**

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE features (                    -- 1 sub-orchestrator, 1 feature branch, 1 pull request
  id          TEXT PRIMARY KEY,            -- F-0003
  name        TEXT UNIQUE,                 -- kebab-case, from the sub-orchestrator, or from the ticket title of a --direct idea; checked by fleetd
  branch      TEXT UNIQUE,                 -- created when the sub-orchestrator first writes its assumptions, or when fleetd accepts a --direct idea
  primary_branch TEXT NOT NULL,            -- the project's base branch
  stacked_on  TEXT REFERENCES features(id), -- the older feature, after a trial merge conflicted; cleared when it is merged or abandoned
  state       TEXT NOT NULL CHECK (state IN ('open','ready','merged','abandoned')),
  pr          TEXT,                        -- pull request or compare URL
  push_state  TEXT CHECK (push_state IN ('ok','failed')),  -- the last push job of the feature branch
  push_failures INTEGER NOT NULL DEFAULT 0, -- failed pushes in a row; at 3, 1 needs-you item
  push_not_before INTEGER,                 -- fleet clock: the end of the push backoff
  pr_state    TEXT NOT NULL DEFAULT 'none' CHECK (pr_state IN ('none','open','closed')),  -- closed means closed and not merged
  created_at  TEXT NOT NULL, updated_at TEXT NOT NULL
);

CREATE TABLE ideas (
  id          TEXT PRIMARY KEY,            -- I-0007
  feature_id  TEXT NOT NULL REFERENCES features(id), -- exactly 1 feature; --to or a new one
  state       TEXT NOT NULL CHECK (state IN
              ('captured','clarifying','awaiting_approval','specifying',
               'ticketing','in_progress','integrating','merged',
               'parked','cancelled')),
  priority    INTEGER NOT NULL DEFAULT 0,
  pane        TEXT,                        -- herdr pane of its sub-orchestrator
  waiting_on_you_since TEXT,               -- set in clarifying with a question open, and in awaiting_approval
  approved_sha256 TEXT,                    -- hash of the assumptions.md you approved
  plan_sha256 TEXT,                        -- hash of the committed plan.yaml
  phase_attempts INTEGER NOT NULL DEFAULT 0,
  created_at  TEXT NOT NULL, updated_at TEXT NOT NULL
);

CREATE TABLE requirements (
  idea_id TEXT REFERENCES ideas(id), req_id TEXT, text TEXT,
  PRIMARY KEY (idea_id, req_id)
);

CREATE TABLE tasks (
  id           TEXT PRIMARY KEY,           -- T-0007-02
  idea_id      TEXT NOT NULL REFERENCES ideas(id),
  key          TEXT NOT NULL,
  profile      TEXT NOT NULL,
  state        TEXT NOT NULL CHECK (state IN
               ('pending','ready','leased','running','stalled','verifying',
                'blocked_on_user','waiting_quota','done','failed','escalated','cancelled')),
  attempts     INTEGER NOT NULL DEFAULT 0,
  max_attempts INTEGER NOT NULL DEFAULT 3,
  not_before   INTEGER,                    -- fleet clock: a ready task in a retry backoff is not dispatched before it
  fix_rounds   INTEGER NOT NULL DEFAULT 0, -- judge fix rounds of a judged task; limit 2, not counted in attempts
  replans      INTEGER NOT NULL DEFAULT 0, -- replans by the sub-orchestrator after failed; limit 1, then escalated
  due_at       INTEGER,                    -- fleet clock: the time limit of the current state; past it, the row goes to needs-you
  spec_json    TEXT NOT NULL,              -- the task as committed
  UNIQUE (idea_id, key)
);

CREATE TABLE task_deps  (task_id TEXT, dep_id TEXT, PRIMARY KEY (task_id, dep_id));
CREATE TABLE task_covers(task_id TEXT, idea_id TEXT, req_id TEXT);

CREATE TABLE attempts (
  id TEXT PRIMARY KEY,                     -- T-0007-02/A1, or I-0007/clarifying/1
  task_id TEXT REFERENCES tasks(id),       -- null for a sub-orchestrator attempt
  idea_id TEXT REFERENCES ideas(id),
  worker TEXT NOT NULL, mux_handle TEXT, pid INTEGER, pid_start TEXT,
  state TEXT NOT NULL CHECK (state IN ('launching','live','ended')),  -- launching: committed, the launch is an effect after the commit
  heartbeat_seq INTEGER,                   -- the last heartbeat seq that fleetd used; a lower or equal one is ignored
  started_at TEXT, ended_at TEXT, exit_code INTEGER,
  outcome TEXT                             -- verified, rejected, crashed, timed_out, stalled, killed, no_result; asked, quota and critical_open burn no attempt
);

CREATE TABLE leases (
  task_id    TEXT PRIMARY KEY REFERENCES tasks(id),
  attempt_id TEXT NOT NULL REFERENCES attempts(id),
  expires_at INTEGER NOT NULL,             -- a deadline on the fleet clock (seconds)
  renewed_at TEXT NOT NULL,
  renew_basis TEXT NOT NULL                -- which signal renewed it
);

CREATE TABLE worktrees (                   -- a row exists before the directory does
  path        TEXT PRIMARY KEY,            -- /fleet/work/T-0007-03
  task_id     TEXT NOT NULL REFERENCES tasks(id),
  idea_id     TEXT NOT NULL REFERENCES ideas(id),
  owner_attempt TEXT REFERENCES attempts(id), -- null while no attempt holds it
  branch      TEXT NOT NULL,               -- fleet/I-0007/T-0007-03
  base_sha    TEXT NOT NULL,
  state       TEXT NOT NULL CHECK (state IN
              ('reserved','created','active','landing','landed','removed',
               'orphaned','rescued')),
  head_sha    TEXT,                        -- last HEAD the reconciler saw
  dirty       INTEGER NOT NULL DEFAULT 0,  -- uncommitted or untracked files seen
  rescue_ref  TEXT,                        -- refs/horch/rescue/T-0007-03/1
  disk_bytes  INTEGER,
  seen_at     TEXT, created_at TEXT NOT NULL
);

CREATE TABLE branches (                    -- a row exists before the branch does
  name        TEXT PRIMARY KEY,            -- fleet/I-0007/T-0007-03
  feature_id  TEXT NOT NULL REFERENCES features(id),
  task_id     TEXT REFERENCES tasks(id),   -- null for a feature branch
  landing_state TEXT CHECK (landing_state IN
              ('building','verifying','ready','in_bundle','landed',
               'conflicted','gate_failed','abandoned')),  -- task branches
  owner       TEXT NOT NULL,               -- worker, fleetd, sub-orchestrator or operator
  due_at      INTEGER NOT NULL,            -- a deadline on the fleet clock (seconds); past it, the row goes to the needs-you list
  queue_seq   INTEGER,                     -- bundle order
  rescue_ref  TEXT,                        -- written before each rebase or forced update
  seen_tip    TEXT,                        -- the local tip that fleetd last saw; each local ref move is a compare-and-set on it
  seen_remote_tip TEXT,                    -- the remote tip that fleetd last saw; each push that rewrites uses --force-with-lease on it
  orphan      INTEGER NOT NULL DEFAULT 0   -- found in git with no row, adopted and reported
);

CREATE TABLE bundles (                     -- 1 candidate of ready task branches of 1 feature; a bisect part is a child bundle
  id          INTEGER PRIMARY KEY,
  feature_id  TEXT NOT NULL REFERENCES features(id),
  base_tip    TEXT NOT NULL,               -- the feature branch tip that the candidate is built on
  candidate_ref TEXT,                      -- refs/horch/bundle/<id>
  candidate_tip TEXT,                      -- set when the build job ends
  parent_id   INTEGER REFERENCES bundles(id), -- the failed bundle, for a bisect part
  state       TEXT NOT NULL CHECK (state IN ('building','gating','passed','failed','dropped')),
  job_id      INTEGER REFERENCES jobs(id), -- the build job, then the fast gate job
  due_at      INTEGER NOT NULL             -- a deadline on the fleet clock; past it, the row goes to needs-you
);

CREATE TABLE bundle_members (
  bundle_id   INTEGER NOT NULL REFERENCES bundles(id),
  branch      TEXT NOT NULL REFERENCES branches(name),
  position    INTEGER NOT NULL,            -- queue order inside the candidate
  PRIMARY KEY (bundle_id, branch)
);                                         -- a task branch is a member of at most 1 bundle that is not ended (building or gating)

CREATE TABLE gate_runs (
  id          INTEGER PRIMARY KEY,
  feature_id  TEXT NOT NULL REFERENCES features(id),
  gate        TEXT NOT NULL CHECK (gate IN ('fast','full')),
  tip         TEXT NOT NULL,               -- the candidate or feature tip it judged
  base_tip    TEXT NOT NULL,               -- the feature branch or primary branch tip it judged
  job_id      INTEGER NOT NULL REFERENCES jobs(id), -- the gate runs as a job, outside the pass
  result      TEXT CHECK (result IN ('pass','fail')),  -- null while its job runs; a pass is stale once either tip moves
  at          TEXT NOT NULL
);

CREATE TABLE trial_merges (               -- facts from git merge-tree --write-tree
  left_ref TEXT, right_ref TEXT, left_tip TEXT, right_tip TEXT,
  job_id  INTEGER NOT NULL REFERENCES jobs(id), -- the trial merge runs as a job, keyed by the 2 tips
  clean INTEGER, conflict_files TEXT, at TEXT NOT NULL,  -- clean is null while its job runs
  PRIMARY KEY (left_tip, right_tip)
);

CREATE TABLE jobs (                        -- each piece of work that can take more than 1 s; it runs outside the pass
  id          INTEGER PRIMARY KEY,
  kind        TEXT NOT NULL CHECK (kind IN
              ('task_check','fast_gate','full_gate','bundle_build','trial_merge',
               'rebase','rescue','fetch','push')),
  subject     TEXT NOT NULL,               -- the row it works for: a task, a branch, a feature, a worktree or a repository
  repo        TEXT NOT NULL,               -- at most 1 job that changes git state runs for 1 repository at a time
  input_key   TEXT NOT NULL,               -- e.g. the 2 tips of a trial merge, or the candidate tip and the base tip of a gate
  state       TEXT NOT NULL CHECK (state IN ('queued','running','passed','failed','abandoned')),
  pid INTEGER, pid_start TEXT,             -- the job process, alive by PID and start time
  time_limit_s INTEGER NOT NULL,           -- from the kind: the check timeout, a gate limit, a git network bound
  deadline    INTEGER,                     -- start + time_limit_s on the fleet clock; past it, the job is stopped
  result_json TEXT,                        -- the outcome, read by the next pass
  created_at TEXT NOT NULL, started_at TEXT, ended_at TEXT,
  UNIQUE (kind, input_key)                 -- the same work is not run 2 times; its result is used again
);

CREATE TABLE remote_tips (                 -- remote git state, from the fetch job of each repository
  repo        TEXT NOT NULL,
  branch      TEXT NOT NULL,               -- the primary branch or a feature branch, on the remote
  tip         TEXT NOT NULL,               -- the tip that the last passed fetch saw
  fetched_at  TEXT NOT NULL,               -- the calendar time of that fetch
  stale_since TEXT,                        -- set by the first failed fetch after it; cleared by a passed fetch
  PRIMARY KEY (repo, branch)
);

CREATE TABLE pass_cursors (                -- the work budget of a pass: where the next pass continues
  kind        TEXT PRIMARY KEY,            -- e.g. dispatch, attempts, outbox files, worktrees, branches, features, jobs
  after_key   TEXT,                        -- the last row handled; the next pass starts after it, and wraps at the end
  lap_started_at TEXT NOT NULL,            -- when the cursor last wrapped
  prev_lap_started_at TEXT                 -- a waiting row was last handled after this time
);

CREATE TABLE questions (                   -- 1 row per worker question
  id          TEXT PRIMARY KEY,            -- T-0007-02/A1/Q1
  task_id     TEXT NOT NULL REFERENCES tasks(id),
  feature_id  TEXT NOT NULL REFERENCES features(id),
  owner       TEXT NOT NULL,               -- the sub-orchestrator of the feature
  state       TEXT NOT NULL CHECK (state IN ('open','answered','escalated')),
  answer_case TEXT CHECK (answer_case IN ('covered','revise','replan','own_decision')),
  citation    TEXT,                        -- checked by fleetd for a covered answer
  due_at      INTEGER NOT NULL,            -- a deadline on the fleet clock (seconds); past it, escalates to needs-you
  asked_at    TEXT NOT NULL, answered_at TEXT
);

CREATE TABLE needs_you (                   -- the needs-you list; /fleet/needs-you/ and horch ideas show only open items
  key         TEXT PRIMARY KEY,            -- the kind and the row, for example question:Q-0012 or feature_pr_closed:F-0003
  kind        TEXT NOT NULL,
  entity      TEXT NOT NULL, entity_id TEXT NOT NULL,
  state       TEXT NOT NULL CHECK (state IN ('open','resolved','superseded')),
  text        TEXT NOT NULL,
  created_at  TEXT NOT NULL, ended_at TEXT  -- an item that is not open is removed 30 days after ended_at
);

CREATE TABLE events (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  at TEXT NOT NULL, entity TEXT NOT NULL, entity_id TEXT NOT NULL,
  kind TEXT NOT NULL, from_state TEXT, to_state TEXT,
  cause TEXT NOT NULL,                     -- e.g. lease_expired, approved_by_user, worktree_rescued
  data_json TEXT
);

CREATE TABLE ingested (                    -- makes file ingestion idempotent
  path TEXT PRIMARY KEY,
  sha256 TEXT,                             -- for a message; null for a log
  log_offset INTEGER,                      -- for a log: the byte after the last complete line read
  outcome TEXT NOT NULL CHECK (outcome IN ('accepted','rejected')),
  reason TEXT,                             -- for a rejected file: the check it failed; the path is not read again
  at TEXT NOT NULL
);

CREATE TABLE requests (                    -- 1 row per command request; makes each command idempotent
  request_id  TEXT PRIMARY KEY,            -- made by the client: the idempotency key
  kind        TEXT NOT NULL,               -- e.g. idea_add, approve, revise, cancel, requeue
  payload_json TEXT NOT NULL,              -- never changes
  payload_sha256 TEXT NOT NULL,            -- a known request_id with another hash is refused
  caller      TEXT NOT NULL,               -- the peer user that the platform layer checked, or the owner of the request file
  state       TEXT NOT NULL CHECK (state IN ('pending','applied','refused')),
  result_json TEXT,                        -- returned again for a repeated request_id
  reason      TEXT,                        -- for a refusal
  received_at TEXT NOT NULL, decided_at TEXT
);

CREATE TABLE fleetd (                      -- exactly 1 row
  id          INTEGER PRIMARY KEY CHECK (id = 1),
  generation  INTEGER NOT NULL,            -- +1 at each start; each write transaction checks it
  pid INTEGER NOT NULL, pid_start TEXT NOT NULL,  -- the fleetd that holds the generation and the lock
  started_at  TEXT NOT NULL,
  fleet_clock_s INTEGER NOT NULL,          -- the fleet clock: seconds; each pass adds the time since the last pass, at most 2 pass periods
  clock_ticked_at TEXT NOT NULL            -- calendar time of the last tick, for status only
);
```

**Three rules that make the ledger trustworthy**

1. **One door.** All writes go through `transition(entity, id, to_state, cause)`, which looks up the allowed-transition table, writes the row and the event in one transaction, and refuses anything illegal. An illegal transition never stops the pass: `transition()` refuses it, records an event with the row and the cause, and puts the row on the needs-you list. Each other row of the pass goes on. There is no other `UPDATE`. Each write transaction first checks that the generation in the `fleetd` row is its own, and fleetd exits when it is not. No effect outside the ledger (a process start or stop, a file write, a git command) occurs inside a transaction. An effect is a row in an intended state (an attempt in `launching`, a worktree in `reserved`, a job in `queued`), and fleetd does it after the commit (see The reconcile pass).
2. **Leases, not claims.** A task in `leased` or `running` always has a lease row with an expiry. Leases are renewed by fleetd from outside signals (heartbeat file, process alive, output or hook activity), never by the model. When the fleet clock passes `expires_at`, the reconciler moves the task to `stalled` in the same pass that notices it, unless a valid `result.json` of the attempt is present in that pass: the result wins, and the task moves to `verifying`. A task in `waiting_quota` has its lease paused: the lease does not expire, and the task leaves the state when its pool resets.
3. **Invariants checked every pass.** The reconciler asserts these and repairs or escalates on any breach:
   - I1: every `leased`/`running` task has exactly one unexpired lease and one attempt with a live process or pane.
   - I2: no task is `ready` while a dependency is not met. A normal dependency (`after:`) is met when its task is `done` and its task branch is `landed`. The dependency of a judge task on the task it judges (`judges:`) is met when the judged task is in `verifying` and its structural check passed. Task `done` means verified. Task branch `landed` means its changes are in the feature branch.
   - I3: every requirement of an idea in `in_progress` or later is covered by at least one non-cancelled task.
   - I4: no worker runs more than one attempt at a time.
   - I5: every live pane and process under the fleet belongs to a live attempt; anything else is an orphan and is stopped.
   - I6: every worktree that `git worktree list` or `/fleet/work/` shows has a `worktrees` row, every row that is not `removed` has its directory, and no worktree is removed unless its branch landed or its rescue ref was verified.
   - I7: no idea is past `awaiting_approval` unless `approved_sha256` matches the current `assumptions.md`, and each feature that has an idea from `clarifying` to `integrating` has exactly 1 live sub-orchestrator attempt, or waits for you.
   - I8: a task branch is `landed`, and a feature is `merged`, only when its changes are in the target branch by content. This holds for a squash merge. fleetd checks it at the transition. `landed` and `merged` are terminal, and fleetd does not check them again.
   - I9: a feature branch and the primary branch move only by fast-forward to a candidate that passed its gate. Each move of a ref is a compare-and-set on the tip that fleetd last saw (`seen_tip`, `seen_remote_tip`): `git update-ref <ref> <new> <expected old>` for a local ref, `--force-with-lease=<ref>:<expected old>` for a push that rewrites. When the tip is not the expected one, fleetd moves nothing, records the new tip, makes the gate result stale and decides again on the next pass.
   - I10: before each rebase or forced update of a branch, a rescue ref of the old tip exists. A rescue ref of class `unique` is deleted only by you.
   - I11: every fleet branch in git has a `branches` row; one without is adopted as an orphan row and reported, never deleted.
   - I12: a branch is deleted only when it is landed, merged or rescued, and no `git stash` entry names it. A task branch is a member of at most 1 bundle that is not ended.
   - I13: a sub-orchestrator exists for each feature that is not merged and not abandoned.

The task state machine is drawn under Supervision, liveness and reconciliation.

**A state name is never written without its entity.** Each state machine has its own table and column, so the ledger is not ambiguous. Each status line, event, log line and message that gives a state also gives the kind of entity: task, task branch, feature, worktree or idea.

**States that share a name**

| State | Meaning |
| --- | --- |
| task `ready` | Its dependencies are done, and it is not dispatched. |
| task branch `ready` | It is verified, and it waits for the next bundle. |
| feature `ready` | Its full gate passed on its tip over the current primary branch. |
| task branch `landed` | Its changes are in the feature branch by content. |
| worktree `landed` | Its branch is landed, so nothing in it is unique. |

## Worker profiles and adapters

Keep your teammate files as the profile format and add the fields that liveness and isolation need; each harness adapter turns one profile into exact flags, env and files, and the conformance test proves the result.

Your `teammates/*.md` frontmatter already declares `agent`, `model`, `effort`, `fallbacks`, `skills` and friends, `mcp_servers`, `mcp_config_files`, `permission_mode`, `allowed_tools`, `sandbox`, `env`, `args` and `compact_window` (`roster/teammate.rs`). That covers most of FR-6. The additions are in the second block below.

```yaml
---
name: codex-sol
base: fleet-worker
brief_description: General Rust and Python implementation, strong on tests.
agent: codex
model: gpt-5.6-sol
effort: high
skills: [execute, tdd, check]
mcp_servers: [context7]
sandbox: workspace-write
compact_window: 256000

# new fields
fallback: opus                 # required: the roster refuses a persona without one
mode: exec                     # exec (run to completion, the default) | interactive (herdr agent)
writable_paths: ["{task_dir}", "{worktree}"]   # nothing else, ledger included
network: [crates.io, static.crates.io, github.com]
liveness:
  start_timeout_s: 90          # no first signal -> attempt failed
  stall_after_s: 300           # no activity -> lease not renewed
  lease_s: 120
  max_wait_s: 3600             # interactive personas only: cap on a declared wait (today: up to 7 days)
retry:
  max_attempts: 3
  backoff_s: [30, 120, 480]
result_gate: required          # stop hook or wrapper repair pass
budget:
  max_minutes: 60              # a hang guard, not a spend limit
---
```

**The clock of each timer.** `start_timeout_s`, `stall_after_s`, `lease_s`, `max_wait_s` and `backoff_s` are durations on the fleet clock, so a sleep of the machine uses none of them (see Supervision, liveness and reconciliation). `max_minutes` is counted by the wrapper in awake time from the platform layer.

**The health of a profile.** The worker-profiles module marks a profile `unhealthy` when a conformance check fails or a dialog stops a worker (rung 4 of the stall ladder). It marks the profile healthy again when the conformance checks pass. fleetd does not dispatch to an unhealthy profile.

**Every worker persona names a fallback persona.** The roster refuses to load a persona without one. fleetd uses it when the persona's pool shows a usage limit or a provider outage (see the stall ladder).

**No budget, no cap, and the model guide.** There is no spend budget: a profile has no token limit. `max_minutes` stays as a hang guard, and the 256k context limit (G5) stays as a quality rule. There is no cap on features, sub-orchestrators or workers. Health guards are not caps: fleetd pauses new dispatch only while free disk, machine load, free memory, the count of processes or the count of open files of the user is past a safety floor, and resumes by itself. The machine probe of the platform layer reads each one. On Windows the machine probe returns unknown today (`preservation-06-wiring-and-platform.md`, row S12). Pacing: fleetd reads the usage window of each pool and dispatches so that the usage is consumed by the reset. Plan validation checks the model guide:

- Opus 5.5 at medium effort is the default.
- Higher effort needs a stated reason.
- A Sonnet task must be marked as "no decisions".
- Fable needs your flag.

**1 launcher for each process.** Each process that fleetd, the wrapper, a job, docs sync or the conformance suite starts goes through 1 launcher function. The launcher applies the environment rule of the profile, and it always removes `ANTHROPIC_API_KEY`, also for a process with no profile (a job or a version command). This section, worker profiles and adapters, owns the environment rule and the fleet-run wrapper. The platform layer owns the process start. An `arch_scan` rule finds no direct process start in production code outside the launcher.

**Adapters.** Today's OpenCode, pi and Prime adapters (launch, skills, resume, context and quota readers) carry over with Claude Code, Codex and Antigravity (`preservation-06-wiring-and-platform.md`, row S15). Dropping them would remove the `opencode-zen` and `local` pools.

**How each adapter applies a profile**

| Field | Claude Code | Codex CLI | Antigravity CLI |
| --- | --- | --- | --- |
| Config isolation | Keep today's `--settings` overlay and `--strict-mcp-config` (no `CLAUDE_CONFIG_DIR`, so your subscription login keeps working) | Private `CODEX_HOME` per attempt, as today | Private `HOME` per attempt with seeded `~/.gemini/antigravity-cli/settings.json` and login (to confirm in conformance) |
| `writable_paths` | `--add-dir` each path; `sandbox.filesystem.allowWrite` | `--add-dir`, or `sandbox_workspace_write.writable_roots`. Drop `broad_ledger_rule`: workers no longer write the ledger | `--add-dir` plus `permissions.allow: write_file(path/)` |
| `network` | `sandbox.network.allowedDomains` | `network_access=true` plus the proxy domain list | `read_url(host)` entries |
| `compact_window` | `CLAUDE_CODE_AUTO_COMPACT_WINDOW=256000` in the settings env | `model_auto_compact_token_limit=256000` | Not settable: the wrapper ends the attempt with a handoff at 256k |
| `mode: exec` | `claude -p --output-format stream-json --verbose --include-hook-events` | `codex exec --json` | `agy -p --output-format stream-json` |
| `mode: interactive` | `herdr agent start <role> --kind claude` | `--kind codex` | `--kind agy` (exec preferred: herdr counts it idle at its prompt) |
| `result_gate` | `Stop` hook: exit 2 with "run `horch result` first" while both `result.json` and a question file are missing | `Stop` hook (managed config, since user hooks need trust), with the same rule | `Stop` hook, with the same rule; plus the wrapper's repair pass in every case, which also accepts a question file in place of `result.json` |
| Usage-limit signal | The adapter maps the usage-limit and outage signal of the CLI in exec mode and in interactive mode to the outcome `quota` (the exact signal to confirm in conformance) | The same | The same |
| Liveness feed | `http` hooks on `PreToolUse`, `PostToolUse`, `Notification`, `PermissionRequest` post to the wrapper | Command hooks on the same events; `--json` stream | `PreToolUse`, `PostToolUse` hooks; `step_update` events |
| Never waits on a dialog | `--permission-mode dontAsk` with explicit allow rules; `--permission-prompts none` in exec | `-a never` | `--mode auto`; parse the stream for soft denials |

**Exec or interactive.** Exec mode is the default for workers because a run-to-completion process cannot sit idle: when the model stops, the process exits and the wrapper sees it at once. You still watch it live, because the wrapper renders the stream into the pane. Interactive mode stays for sub-orchestrators, which interview you in their panes, for personas you like to steer by hand, and for any CLI whose headless mode can't do the job. In interactive mode the stop gate and the `idle_no_result` rung carry the same guarantee one step later. Sub-orchestrators are interactive. A persona marked interactive is also the way for you to pair with a worker in a pane.

## Ideas: the dispatcher and sub-orchestrators

The top of the fleet is a program, not a model: you drop rough ideas into a queue, and the dispatcher inside fleetd keeps one sub-orchestrator pane running for every feature that is not merged or abandoned, moving each idea through a fixed state machine that only code can advance.

An AI orchestrator gets cloudy because one conversation holds every idea at once. Here no AI sees more than one idea, and no AI decides what happens next to an idea. The dispatcher reads the idea's state and the files in its folder, and starts the next step.

**Capturing an idea needs no AI.** From any terminal:

```
horch idea add "Retry sync with backoff and log each retry"
horch idea add -f rough-notes.md
horch idea add --direct ticket.md   # fast lane: a ticket you wrote, no research or interview
horch idea amend I-0012 "Also cap the delay at 30 s"
horch ideas                      # the board: every idea, its state, what it waits on
```

Each `add` writes one immutable file and one ledger row in state `captured`, and prints the ID. Half-formed ideas are fine; making them clear is the sub-orchestrator's job.

**Everything is an idea, with a fast lane.** There is 1 state machine and 1 way in. There is no task without an idea. `horch idea add --direct <ticket file>` takes a ticket that you wrote. It skips research and the interview, and makes a plan with 1 work task, plus its judge task when the kind is a judgment kind. When the work task has a judgment kind, fleetd adds the paired judge task to the plan itself. You do not write it. The idea goes from `captured` to `ticketing`; the ticket you wrote stands for your approval, and `horch idea add --direct` refuses to run inside a fleet attempt, like `horch idea approve`. The plan still passes plan validation, and the work task still needs a check. When plan validation refuses the `--direct` plan, the idea goes to `needs-you/` with the reason. A `--direct` idea belongs to a feature (see Features, landing and integration): with `--to <feature>` it joins that feature and uses its branch. With no `--to` it starts a new feature, and it has no assumptions phase. For it, fleetd makes the branch name from the ticket title (kebab-case, with a number suffix when the name exists), and creates the branch when it accepts the idea. fleetd writes the ledger row first. The feature of a `--direct` idea still has a sub-orchestrator, which owns its questions, conflicts and gate failures.

**The idea state machine**

| State | Who acts | Leaves when |
| --- | --- | --- |
| `captured` | Dispatcher | On the next pass it opens pane `idea-12` and moves to `clarifying`, unless another idea of its feature is in `clarifying`, `specifying` or `ticketing`: then it waits here until that idea leaves those states. An idea added with `--direct` moves to `ticketing` under the same rule. |
| `clarifying` | Sub-orchestrator, with you in its pane | It has researched the repo and docs, interviewed you, and written `assumptions.md` with no item left open. |
| `awaiting_approval` | You | You run `horch idea approve I-0012`, or `horch idea revise I-0012 "..."` to send it back to clarifying. |
| `specifying` | Sub-orchestrator | `spec.md` and `requirements.yaml` validate, and every requirement cites an approved assumption or one of your answers. |
| `ticketing` | Sub-orchestrator | Tickets and `plan.yaml` pass the plan validator and commit in one transaction. |
| `in_progress` | Workers; the sub-orchestrator answers their questions and replans failed tasks | Every task that is not `cancelled` is `done`, and every task branch that is not `abandoned` is `landed` on its feature branch. A cancelled task does not block its idea: fleetd writes a rescue ref of its branch and moves the branch to `abandoned`. |
| `integrating` | fleetd | Every task of the idea that is not `cancelled` is `done`, and every task branch that is not `abandoned` is `landed` on its feature branch. The idea waits here until its feature's changes are in the primary branch. |
| `merged` | Nobody | Terminal. Reached only when git shows its feature's changes in the primary branch and each worktree of the idea is `removed` or `rescued`. fleetd then removes the `rescued` trees, and the rescue refs stay. |
| `parked`, `cancelled` | You | `horch idea park` or `cancel`; escalations also park. Worktrees are rescued first. |

The table above is the idea state machine: 10 states, with 1 approval gate at `awaiting_approval`.

Every idea belongs to exactly 1 feature, and every feature that is not merged and not abandoned has 1 sub-orchestrator. Merged also requires that each worktree of the idea is `removed` or `rescued`.

**How many sub-orchestrators.** One for each feature that is not merged and not abandoned. There is no cap on features, sub-orchestrators or workers, so no idea waits for a slot of the fleet, and ideas of different features never wait for each other. Ideas of 1 feature are planned in order: while 1 idea of a feature is in `clarifying`, `specifying` or `ticketing`, a new idea of that feature waits in `captured`. The live process of a feature, in any phase, owns the open worker questions of the whole feature (see How a worker's question is resolved). The count always matches the ledger, because the reconciler starts a missing pane or stops an extra one on every pass.

**A fresh process for every phase.** Clarifying, specifying, ticketing and supervising are each a new sub-orchestrator process in the same pane, whose only memory is the idea folder: `idea.md`, amendments, `interview.md`, `assumptions.md`, then `spec.md` and the plan. Nothing carries over in a context window, so nothing can blur between ideas or between phases, and a crashed sub-orchestrator restarts mid-phase from its files.

**The interview.** In `clarifying` the sub-orchestrator first reads the code and docs the idea touches, then asks you one question at a time in its pane, covering what done looks like, scope edges, constraints, affected areas and priority. It records each question and answer with `horch idea note`, so `interview.md` is complete even if the pane dies. It ends by writing `assumptions.md`:

```markdown
# I-0012 assumptions
1. [confirmed] Retries apply to all sync requests, not uploads. (Q3)
2. [assumed] Max 5 tries is enough; no config option needed.
3. [assumed] Log level INFO for each retry.
```

The validator refuses the file while any item is marked `[open]`. `[assumed]` items are exactly what you approve or correct.

**Quick approval stays with you, in code.** When an idea reaches `awaiting_approval`, fleetd posts a herdr notification and `horch ideas` shows it under Needs you. `horch idea show I-0012` prints the assumptions; `approve` or `revise` records your decision. The sub-orchestrator cannot approve for you: `horch idea approve` refuses to run when its caller's process tree contains any recorded fleet attempt, agent profiles deny the command, and fleetd writes `approval.json` itself, with the hash of the assumptions you saw, and ignores one written by anyone else. A herdr key binding can map one key to approve the focused idea pane (herdr supports custom key commands; the exact fields are unconfirmed).

**Waiting on you is not a stall.** While an idea is in `clarifying` with a question open, or in `awaiting_approval`, its lease is paused and fleetd sends one reminder an hour during your working hours. After 30 minutes on the fleet clock it stops the idle sub-orchestrator process to free memory; an open question or your approval starts it again from the folder. After 7 days of calendar time with no answer the idea moves to `parked`, with a note in `needs-you/`, so the queue only counts ideas that can move.

**A worker that waits holds no process.** An exec worker that asks a question exits. Its task moves to `blocked_on_user` with no process and no time limit of its own; the question has its time limit. The 1 h cap on a declared wait (`max_wait_s`) applies only to an interactive persona; at the cap the process exits and the task parks in the same way.

**How a worker's question is resolved.**

1. The worker writes 1 question file, commits its work in progress, and exits. The Stop hook and the repair pass of the wrapper accept the question file in place of `result.json`. The attempt ends with the outcome `asked`, which is not a failure and burns no attempt. The task moves to `blocked_on_user`, and its worktree stays. The other tasks of the feature go on.
2. The question is a ledger row with an owner (the sub-orchestrator of the feature), a state and a time limit: 15 min on the fleet clock with no answer from a running sub-orchestrator, a starting value.
3. The live process of the feature owns the open worker questions of the whole feature, in any phase: clarifying, specifying, ticketing or supervising, for any idea of the feature. fleetd rings it with the doorbell when a question arrives. It answers open questions before it continues its phase. On each pass, fleetd starts the owner if it is not running. Delivery repeats until an answer is recorded. The time limit covers a process that does not answer.
4. The answer is 1 of 4 cases:

| Case | What occurs |
| --- | --- |
| The approved assumptions or the spec cover it | The sub-orchestrator answers and cites the line. fleetd checks that the citation exists. |
| The answer changes an approved assumption | It goes to you as a `revise`. Only the affected tasks pause. |
| The question shows a flaw in the plan | The sub-orchestrator replans the task. |
| Nothing approved covers it, and it changes no assumption | The sub-orchestrator decides when the code or the spec makes the answer clear. It records the answer as a new assumption marked as its own decision. You can reverse it. Only product direction, scope or a trade-off that you must own goes to you. |

5. fleetd resumes the worker's session with the answer. If the resume fails, a fresh worker starts from the question, the answer and the committed work.
6. The answer goes to the decision log of the feature and to every later task of the feature.
7. Past its time limit, a question escalates to you in `needs-you/`, with a herdr notification. The wait for you has no limit: the task stays in `blocked_on_user` until you answer.

**The needs-you list has a life cycle.** The ideas context owns the list, in the `needs_you` table, and `/fleet/needs-you/` mirrors it. Each item has a stable key (its kind and its row) and a state: `open`, `resolved` or `superseded`. A producer writes by key, so 1 thing has 1 item: an escalation raised on each pass stays 1 item. An item is `resolved` when its cause ends (an answer, an approval, a requeue, a passed push), and `superseded` when a newer item of the same row replaces it. `horch ideas` and the folder show only `open` items. An item that is not open is removed after 30 days.

**Amendments.** Before `awaiting_approval`, an amendment joins the interview. After approval it becomes a revise with its own approval. Once the idea is merged, it becomes a new idea with `depends_on`.

## Worktrees: leased, reconciled, never lost

Every worktree is a ledger resource: fleetd records it before it exists, ties it to one task and one owner, and never deletes it until its work has landed or is saved on a verified rescue ref.

Today a worker creates its own worktree, so one that dies mid-task leaves a directory nobody knows about. horch already has a `worktrees` table, triage, and archive and trash refs; this section makes them authoritative by changing who creates worktrees and when the row is written.

**Lifecycle**

| State | Meaning | Leaves when |
| --- | --- | --- |
| `reserved` | Row written in the same transaction that leases the task. No directory yet. | fleetd runs `git worktree add` on branch `fleet/I-0007/T-0007-03` → `created`. A row left here by a crash is created or dropped on the next pass. |
| `created` | Directory and branch exist; no attempt holds it. | An attempt starts with it as its working directory → `active`. |
| `active` | Owned by exactly one live attempt. HEAD, the dirty flag and disk size are refreshed on each git scan. | The attempt ends: verified → `landing`; retried → back to `created`, so the next attempt continues in the same tree; owner dead → `orphaned`. |
| `landing` | Its branch is in the landing state machine (see Features, landing and integration). A resolver attempt or a fix attempt of its branch runs in it. | Its branch is `landed` on the feature branch → `landed`. Its branch is `abandoned` → fleetd queues a rescue job; the rescue is verified → `rescued`. |
| `landed` | Nothing in it is unique any more. | fleetd removes it and deletes the branch only if it is merged → `removed`. |
| `orphaned` | On disk with no live owner, or on disk with no row at all. | Rescued and verified → `rescued`. A rescue that fails its verification keeps the tree here and puts 1 item on the needs-you list. |
| `rescued` | Its full state is saved on a rescue ref and checked. | Adopted by the next attempt of its task. Its idea is `merged` or `cancelled` → fleetd removes it → `removed`. The rescue ref stays. |
| `removed` | Gone from disk and from `git worktree list`. | Terminal. |

**Only fleetd creates or removes worktrees.** Workers start inside a tree fleetd made. Their profiles deny `git worktree` through each CLI's permission rules, and a new `arch_scan` rule keeps worktree calls out of every horch code path except fleetd. A worker that makes one anyway is found by the reconciler as an unowned tree and rescued like any orphan.

**The reconciler compares three views.** On every pass it reads the `worktrees` rows and the directories under `/fleet/work/`; every 60 s it also reads `git worktree list --porcelain` for each repo the fleet uses, including prunable and locked entries. Then:

- A directory with no row is adopted as `orphaned`, linked to the task its branch name points to, if any.
- A row whose owner attempt is dead becomes `orphaned` in the same pass that sees the death.
- A `reserved` row older than a minute with no directory is dropped.
- A row whose directory vanished without fleetd removing it is escalated, because something outside the fleet deleted work.

**Rescue before any cleanup.** A rescue runs as a job, outside the reconcile pass. Rescue never touches the worktree's own branch or index, so it is safe to run on a tree a dying process was still writing:

```sh
# fleetd, inside /fleet/work/T-0007-03
export GIT_INDEX_FILE=/fleet/run/rescue-T-0007-03.index
git read-tree HEAD
git add -A                         # tracked edits and untracked files; .gitignore still applies
commit=$(git commit-tree "$(git write-tree)" -p HEAD -m "horch rescue T-0007-03 attempt A2")
git update-ref refs/horch/rescue/T-0007-03/1 "$commit"
# verify: the files on disk and the rescue commit match exactly
git diff --quiet "$commit" && test -z "$(git ls-files --others --exclude-standard)"
```

fleetd then writes `/fleet/rescue/T-0007-03-1.json` with the ref, the old HEAD, the file count, the size and the attempt. Only after that may a tree be removed. A rescue that fails verification leaves the tree in place and puts 1 item on the needs-you list. Ignored files such as build output are not saved, which keeps rescue refs small; a profile can list ignored paths worth keeping in `rescue_include`.

**Reuse, don't redo.** The next attempt of the same task gets the same worktree, with the rescue ref as a safety copy, and its `START.md` names the rescue commit and the last attempt's progress notes. Work you paid for becomes the starting point of the retry.

**Disk.** `horch status` shows the total size of all trees. Above `worktree_disk_budget` (default 20 GB) fleetd removes `landed` and `rescued` trees of merged or cancelled ideas, oldest first. It never removes an `active` tree for space; it pauses new dispatch and tells you instead.

**Ideas finish clean.** An idea reaches `merged` only when each of its worktrees is `removed` or `rescued`. fleetd then removes the `rescued` trees. The rescue refs stay, and its closing note lists each rescue ref of class `unique`. `horch worktrees` lists every tree with its state, task, owner, age and size, and `horch worktree restore T-0007-03` turns a rescue ref back into a branch `rescue/T-0007-03-1` and a working tree.

**Built on what horch has.** The existing `worktrees` table gains the state, owner and rescue columns, triage reads them, and `refs/horch/rescue/` sits beside your archive and trash refs. A rescue ref of class `superseded` follows the retention of the trash refs. A rescue ref of class `unique` is kept until you decide (see No work is lost).

## Features, landing and integration

Work lands in 2 steps, and the ledger tracks each branch through both: a task branch lands on its feature branch automatically, and a feature branch reaches the primary branch through 1 pull request.

The primary branch is the base branch of a project. It is `main` in this repository, but it is not `main` in every project fleetd runs.

### Features

- A feature is 1 sub-orchestrator, 1 feature branch and 1 pull request to the primary branch. It holds 1 main idea and its follow-on ideas.
- fleetd creates the feature branch when the sub-orchestrator first writes its assumptions. The sub-orchestrator gives the name. fleetd checks that it is kebab-case and unique. fleetd writes the ledger row before it creates the branch.
- A `--direct` idea with no `--to` has no assumptions phase. For it, fleetd makes the branch name from the ticket title (kebab-case, with a number suffix when the name exists), and creates the branch when it accepts the idea. fleetd writes the ledger row first. A `--direct` idea with `--to` uses the branch of that feature.
- `horch idea add --to <feature>` adds an idea to an open feature. Its tasks land on that feature branch. With no `--to`, the idea starts a new feature. An idea belongs to exactly 1 feature, and the ledger records it.
- A feature is an intake funnel until its pull request merges. After the merge, a follow-up idea starts a new feature.
- The sub-orchestrator of a feature plans 1 idea at a time, with a fresh process for each phase. Tasks of several ideas of 1 feature run in parallel.

**The feature state machine**

| State | Owner | Leaves when | Time limit |
| --- | --- | --- | --- |
| `open` | fleetd | 2 things are true → `ready`: each idea of the feature is `integrating`, `cancelled` or `parked`, and the full gate passed on the feature tip over the fetched tip of the primary branch. You run `horch feature abandon <feature>` → `abandoned`. | None. Its ideas, tasks and branches have their own limits. |
| `ready` | You, or fleetd when the project sets the merge to automatic | Its tip or the fetched primary tip moves, or a new idea is accepted with `--to` → `open`. git shows its changes in the primary branch by content → `merged`. You run `horch feature abandon` → `abandoned`. | None. The pull request waits for you. |
| `merged` | Nobody | Terminal. fleetd decides it 1 time, at the transition, and does not check it again (I8). | None. |
| `abandoned` | You | Terminal. Only you set it, with `horch feature abandon <feature>`. fleetd first writes a rescue ref for each branch of the feature that is not landed, then moves each such branch to `abandoned`, and each idea of the feature that is not `merged` to `cancelled`. | None. |

**The pull request follows the state.** fleetd opens the pull request (`pr = gh`), or shows the compare URL (`pr = manual`), only when the feature is `ready`. When the feature moves back to `open`, fleetd marks the pull request as a draft (`gh`), or changes the needs-you item to "do not merge yet" (`manual`).

**A merge with ideas in flight.** You can merge a pull request at any time. When git shows the merge and the feature still has an idea that is not `integrating`, `cancelled` or `parked`, fleetd records the feature as `merged` and moves each such idea to a new feature. The new feature branch starts from the fetched primary tip, and fleetd writes its row before the branch. The sub-orchestrator and its pane go on with the new feature. fleetd writes a rescue ref of each task branch of a moved idea, then rebases it on the new feature branch as a job. No idea is left without a feature and an owner, and no branch lands on the branch of a `merged` feature.

**Push and pull request results are facts on the feature row.** A failed push job sets `push_state` to `failed`, adds 1 to `push_failures`, and sets `push_not_before` to the end of a backoff on the fleet clock. After 3 failures in a row, fleetd puts 1 item on the needs-you list. A passed push sets `push_state` to `ok` and `push_failures` to 0. `pr_state` is `none`, `open` or `closed`. With `pr = gh`, the fetch job reads the pull request state, and a pull request that is closed and not merged puts 1 item on the needs-you list: open it again, or run `horch feature abandon`. With `pr = manual`, you run `horch feature abandon` when you do not merge. A project can set `remote = none`: fleetd does not fetch, push or open a pull request, and a feature is `merged` when its changes are in the local primary branch.

### Landing a task branch on its feature branch

Every branch is a ledger row before it exists. Each row has a landing state, 1 owner and a time limit. Past the limit the row goes to the needs-you list. No state is silent. Each time limit is a starting value in `fleet.toml`, on the fleet clock.

| State | Owner | Leaves when | Time limit |
| --- | --- | --- | --- |
| `building` | worker | the task moves to `verifying` → `verifying`. The task is `cancelled` → fleetd writes a rescue ref, then → `abandoned`. | The time limits of its task. |
| `verifying` | fleetd | the task is `done` → `ready`. For a judged task this is after its judge (see The judge pair). The check fails and a new attempt starts → `building`. The task is `cancelled` → rescue ref, then `abandoned`. | The time limits of its task. |
| `ready` | fleetd | fleetd puts it in the next bundle → `in_bundle`. | 30 min. |
| `in_bundle` | fleetd | its bundle passes the fast gate and the feature branch moves forward by compare-and-set → `landed`. Its bundle is `dropped` → `ready`. Its bundle fails: the bisect puts it in a child bundle (it stays), or finds it as the failing branch → `gate_failed`, or finds it clean → `ready`. A conflict when the candidate is built → `conflicted`. | The time limit of the build job and the fast gate job, plus 10 min. |
| `landed` | nobody | terminal | None. |
| `conflicted` | fleetd | fleetd starts a resolver attempt for this task branch in its worktree, with the conflict in `zdiff3` form and the tests of both sides. Its result passes its check → `ready`. | 60 min. |
| `gate_failed` | fleetd | fleetd starts a fix attempt of the task, in its worktree, with the output of the gate. Its result passes the check → `verifying` → `ready`. | 60 min. |
| `abandoned` | fleetd, or you with `horch feature abandon` | terminal; a rescue ref is kept | None. |

A resolver attempt and a fix attempt are attempts of the task row. The task stays `done`, because its own result was verified. The attempt is verified on the branch, and the branch, not the task, moves.

**The bundle.** When several task branches of 1 feature are `ready` together, fleetd builds 1 candidate from the feature branch tip plus each ready task branch in queue order. Each task's commits are kept. fleetd first writes 1 `bundles` row in `building`, with the base tip, and 1 `bundle_members` row for each branch, in 1 transaction. The build of the candidate is a job that writes the ref `refs/horch/bundle/<id>`. Then the row moves to `gating`, and the fast gate runs once on the candidate as a job. A pass never waits for either. A later pass reads the result of the gate job. On a pass of the gate, the bundle moves to `passed`, and the feature branch moves forward to the candidate by compare-and-set from the base tip (I9). On a failure, the bundle moves to `failed`, and fleetd bisects it: each half is a child bundle with `parent_id`, built and gated in the same way. The failing branch goes to `gate_failed`, and the others go on. After a restart, fleetd continues each bundle from its row. No ready branch is forgotten, because each one is a row that fleetd reads on every pass.

**A moved tip drops a bundle.** When the tip of a feature branch moves while a bundle of that feature is in `building` or `gating`, the bundle goes to `dropped`, and its members go back to `ready`. fleetd does not start a rebase of a feature branch while 1 of its bundles is in `gating`. It waits for the gate job, which has a time limit.

**fleetd sets the git settings that it needs.** fleetd sets `rerere.enabled` and `merge.conflictStyle=zdiff3` in each worktree and each repository that it manages. It does not assume them.

No human approves a landing on a feature branch.

fleetd keeps these behaviors of today's land path (`preservation-04-behavior.md`):

- F9: under the land lock, just before the fast-forward, fleetd checks that the task is still the worker's. A task cancelled earlier lands nothing.
- F7: a red land resets the branch to its tip from before the rebase.
- F5: a passed step stands only for the tree it judged. New commits reopen the land and every later step.
- F11: every git network call has a time limit (push 10 min, others 3 min). Each one runs as a job, and its time limit is the time limit of the job.
- L6: 2 running tasks of 1 feature never own overlapping paths.

### No work is lost

fleetd checks these on every pass against git. They are invariants I8 to I13 in the Ledger section.

1. A task branch is `landed`, and a feature is `merged`, only when its changes are in the target branch by content. This holds for a squash merge.
2. A feature branch and the primary branch move only by fast-forward to a candidate that passed its gate. Each ref move is a compare-and-set on the tip that fleetd last saw. A landing cannot overwrite another, and someone else's commit is never overwritten.
3. Before each rebase or forced update of a branch, fleetd writes a rescue ref of the old tip.
4. A fleet branch in git that has no ledger row is adopted as an orphan row and reported. It is never deleted.
5. A branch is deleted only when it is landed, merged or rescued. A `git stash` entry that names the branch blocks the deletion (`preservation-04-behavior.md`, row W6).
6. A sub-orchestrator exists for each feature that is not merged and not abandoned.

**Rescue refs have 2 classes, and fleetd owns both.** fleetd decides the class of each rescue ref by content, against the landed branches, each time it reads the ref.

- `unique`: it holds work that is in no landed branch. It is kept until you decide: `horch worktree restore`, or `horch rescue drop <ref>`. No collection deletes it.
- `superseded`: an old tip whose work is in a newer tip or in a landed branch. It expires 30 days after its feature is merged or abandoned, with the trash refs.

`horch status` shows the count and the bytes of each class. When the `unique` class is over `rescue_unique_budget` (a starting value in `fleet.toml`), fleetd puts 1 item on the needs-you list. It deletes nothing.

**A cancelled task does not block its idea.** When a task is cancelled, fleetd writes a rescue ref of its task branch and then moves the branch to `abandoned`. The roll-up of an idea counts only tasks that are not `cancelled` and task branches that are not `abandoned`.

### Between features

- **Parallel is the default.** Any number of features can change the same file. A shared file name never makes a feature wait.
- **Early detection.** When a feature branch tip or the primary branch moves, fleetd queues a trial merge job of each in-flight feature against the primary branch. Between features, it queues 1 job for each pair of in-flight features whose sets of changed files share a file. A pair with no shared file needs no job. A job is keyed by the 2 tips, so a pair whose tips did not move is not merged again: its result is used again. A count of features of n does not make n times n jobs in 1 pass. The job uses `git merge-tree --write-tree`, with no checkout. The result is a fact in the ledger.
- **Stack on evidence.** When the trial merge of 2 features conflicts, fleetd puts the younger feature on top of the older one. The conflict becomes 1 resolver task for a fresh agent. The agent gets the conflict in `zdiff3` form, the requirements of both features for those files, and the unit tests of both. It makes 1 commit for each requirement. The task lands on the younger feature's branch.
- **Continuous rebase.** When the fetched tip of the primary branch moves, fleetd queues a rebase job for each feature branch, the bottom of a stack first. It does not start the rebase of a feature branch while 1 of its bundles is in `gating`, and the move of the tip drops each bundle of that feature in `building`. After a clean rebase the fast gate job runs again. A conflict becomes a resolver task. fleetd sets `rerere.enabled` itself.
- **Build on what is in flight.** At plan time the sub-orchestrator gets the diffs of the files that in-flight features already changed. It can propose to build on one of them. The proposal is in the assumptions that you approve.
- **The full gate.** The full gate runs once for a feature as a job, when its task branches are landed, on its tip over the fetched tip of the primary branch. Failures become triage tasks of the feature. If either tip moves after a pass, the pass is stale and the gate runs again before the feature is marked ready.
- **Two declared gates.** Each project declares its fast gate (its unit tests) and its full gate (the fast gate plus the end-to-end tests). Plan validation refuses a plan for a project that did not declare both. This repository has no unit-only command today (`research-03-this-repo.md`, F10).
- **The pull request.** A project setting `pr` is `gh` or `manual`. `manual` pushes the feature branch and shows the compare URL. Either one happens only when the feature is `ready` (see The pull request follows the state). A stacked feature's pull request has the lower feature's branch as its base. A project setting says who merges; the default is you.
- **Merged.** A feature is `merged` when its changes are in the primary branch. fleetd reads this from git, not from the GitHub API, at the fetched tip of the primary branch. With `remote = none`, it reads the local primary branch.
- **A stack edge ends with its lower feature.** The owner of each step is fleetd. When the lower feature is `merged`, fleetd clears `stacked_on` of the upper one, writes a rescue ref, rebases the upper one on the fetched primary tip, and runs both gates again. It sets the base of the pull request to the primary branch (`gh`), or changes its needs-you item (`manual`). When the lower feature is `abandoned`, fleetd clears `stacked_on`, writes a rescue ref, and rebases the upper one on the primary tip without the commits of the lower one. A conflict opens a resolver task.
- **Remote git state is level-triggered.** 1 `git fetch` job runs for each repository on a period (`fetch_period_s`, default 60 s, in `fleet.toml`). fleetd records the fetched tip of each branch and the time in `remote_tips`. Merge detection and gate freshness use the fetched tip. So a pull request that you merge on the remote is seen within 1 period, with no local command. When a fetch fails, `horch status` shows since when the remote tip is stale.
- **1 git writer for each repository.** At most 1 job that changes git state runs for 1 repository at a time: a bundle build, a rebase, a rescue, a fetch or a push. This removes git lock contention. Jobs that only read git, for example a trial merge, can run together.
- **Starting values, to tune from data.** A resolver task covers at most 100 changed lines or 2 files, else fleetd splits it by file.
- **Not in version 1.** A syntax-aware merge driver: there is no accuracy data for Rust, and in Java wrong clean merges rose from 3% to 11% (`research-02-merge-tools-and-agent-systems.md`, F13). 1 pull request for several features: 1 bad feature would block the others, and each feature loses its own pull request. Both are follow-ups.

**The evidence**

| Fact | Value | Source |
| --- | --- | --- |
| Ideas that change a hot file | 46% to 54% | `research-03-this-repo.md`, F1 |
| Landed tasks that hit a conflict | 2.7% of 376 | `research-03-this-repo.md`, F7 |
| Files changed by 2 or more tasks on 1 branch, and the land conflicts they gave | 66 files, 3 conflicts | `research-03-this-repo.md`, F8 |
| Clean merges that break the build or the tests | about 3% | `research-02-merge-tools-and-agent-systems.md`, F13 |
| 1 agent that builds 2 features, against 2 agents plus a merge | about 30% better | `research-02-merge-tools-and-agent-systems.md`, F20 |

## Supervision, liveness and reconciliation

fleetd runs one level-triggered reconcile pass every 5 seconds that compares what the ledger says should exist with what is actually running and on disk, so a missed event is simply caught on the next pass.

Your current watchdog misses things for three reasons found in `taskledger_watch.rs`: it is started lazily and only restarted after 20 quiet minutes, its wakes are one-shot (first stall, one unclaimed flag, one rewake per board state), and its only remedy is typed mail that the orchestrator LLM must notice and act on. Each reason is removed below.

**Liveness comes from outside the model**

The wrapper and fleetd combine four signals, strongest first. The model's own words are not one of them.

| Signal | Source | Tells us |
| --- | --- | --- |
| Process | Process facts from the platform layer (on Linux `kill -0 pid` and `/proc/<pid>/stat`: running, stopped, zombie), the wrapper's wait status | Alive or dead, for certain. |
| Wrapper heartbeat | `heartbeat.json`, rewritten every 10 s by a wrapper thread that does not depend on the model | The wrapper and its view of the CLI are alive. |
| Activity | CLI event stream timestamps, hook events (tool start and end), token counter, `progress.jsonl`, worktree changes | The agent is making progress, not just existing. |
| herdr status | `session.snapshot` and `pane.agent_status_changed` | Working, blocked on a dialog, or idle at its prompt. |

fleetd derives one agent state per attempt: `starting`, `working`, `waiting_approval`, `idle_no_result`, `hung` (process alive, no activity for `stall_after`, default 5 min), `usage_limited` (the adapter shows the usage-limit or outage signal of its CLI; it is never `hung`, and the attempt ends with the outcome `quota`), or `exited`.

**Lease renewal is earned, not claimed.** fleetd renews a lease only when the process is alive, the heartbeat is under 30 s old and there was activity within `stall_after`. A worker that holds a task and does nothing stops earning renewals, and its lease expires within 120 s of the stall threshold. That is how "took the task and sat on it" becomes impossible to hide.

**One fleet clock for each timer. A sleep fires no deadline.** fleetd keeps 1 fleet clock in the ledger, in the `fleetd` row: a count of seconds. Each pass adds the time since the last pass, but at most 2 times the pass period (10 s). So the clock does not run while the machine sleeps, and does not run while fleetd is down. No actor is judged for a time in which nothing could act. The fleet clock is level-triggered: it needs no sleep or wake event, and a restart of fleetd needs no repair of it.

| Clock | Timers |
| --- | --- |
| Fleet clock | The lease; each rung of the stall ladder (`stall_after`, the 2 min doorbell rung); the start timeout; each retry backoff; the wait limits (`max_wait_s`, and the 30 min stop of an idle sub-orchestrator); the question limit; the `due_at` of each row; the time limit of a job. Each is a deadline on the fleet clock. |
| Awake time from the platform layer | The hang check of fleetd, because the fleet clock stops with fleetd. `max_minutes` of the fleet-run wrapper, because the wrapper is the guard that must work while fleetd is down. |
| Calendar time | The 7 day park; the hourly reminders; the retention periods (for example trash refs and the message spool); the daily docs sync; the usage windows of a provider. |

**Stall ladder.** Every rung is code with a fixed timer; none depends on a model reading anything.

A usage limit or a provider outage is not a stall. A worker that shows one is not stuck, is never reaped as stuck, and does not burn an attempt. If the persona's fallback is on another provider, fleetd restarts the task at once on a new worker of the fallback persona, on the same worktree. If not, the task goes to `waiting_quota`, its lease is paused, and it leaves the state when the pool resets. A usage limit can carry a reset time `reset_at`. It is optional: when it is absent, fleetd waits until the pool probe reports ok. If both pools are limited, the task waits; fleetd does not go to a third persona. Today no code enforces "never reap a usage-limited worker": the rule is only in the orchestrator briefing (`preservation-04-behavior.md`, row S6). fleetd enforces it in code.

1. `idle_no_result` in an interactive pane: send the constant doorbell once. For Claude Code a Stop hook already refuses to let the turn end until `result.json` exists (see Worker profiles), so this rung is mostly for other CLIs.
2. Still idle 2 minutes later: doorbell again and record it.
3. Still idle, or `hung` at any point: end the attempt. Stop the process group through the platform layer (on Linux: SIGINT, wait 20 s, then SIGKILL). Record outcome `stalled`.
4. `waiting_approval`: this is a profile defect, because profiles pre-approve everything a task needs. fleetd ends the attempt, writes the screen excerpt to `needs-you/`, and marks the profile unhealthy until a conformance test passes.
5. Process exited. fleetd looks first for the 2 exits that are not failures. A valid question file gives the outcome `asked`: the task moves to `blocked_on_user`, and its worktree stays. A usage-limit or outage signal of the adapter gives the outcome `quota`: the task follows RR-11. Neither burns an attempt. Only an exit with no valid result, no valid question file and no usage-limit signal gives the outcome `no_result`.
6. Any other outcome that is not verified (`rejected`, `crashed`, `timed_out`, `stalled`, `killed`, `no_result`) burns 1 attempt. If attempts remain, the task moves to `ready` with `not_before` set to the end of the backoff (30 s, 2 min, 8 min) on the fleet clock, and dispatch does not lease it before then. The new attempt runs in the same worktree, with a handoff file built from the old attempt's `progress.jsonl`, `git diff` and transcript tail. Otherwise the task moves to `failed`: the sub-orchestrator of its feature replans it 1 time, within its time limit. Then the task moves to `escalated`, with 1 item in `needs-you/`.

**Finishing without reporting is closed at both ends.** Inside the agent, a stop hook (or for exec mode, the wrapper on exit) checks for `result.json` or a question file. If a process exits with commits but neither, the wrapper runs one short resume of the same session with a fixed instruction to write the result, then exits. Outside the agent, "done" is defined only by the verifier running the acceptance commands, so a result that claims success without passing checks goes back to `ready`.

**The reconcile pass**

```python
def reconcile():
    # Part 1: read the facts, with no transaction.
    snap  = mux.snapshot()          # herdr session.snapshot: panes, agents, status
    procs = scan_processes()        # pids and pgids of attempts and jobs, each with its attempt or job id
    files = scan_outboxes()         # full scan, with the checks for an untrusted file; file watching only makes it run sooner
    trees = scan_worktrees()        # /fleet/work every pass, git worktree list every 60 s
    reqs  = read_requests()         # from the channel, and request files in /fleet/run/requests/
    # Part 2: 1 transaction that decides and writes rows. No effect occurs inside it.
    with ledger.tx() as tx:
        check_generation(tx)                  # the generation in the fleetd row is ours, else exit
        tick_fleet_clock(tx)                  # + time since the last pass, at most 2 pass periods; every deadline uses it
        apply_requests(tx, reqs)              # 1 requests row per request_id; a known id gets its stored result
        ingest(tx, files)                     # messages by (path, sha256), logs by offset; rejected files recorded
        observe_intents(tx, procs, trees)     # each intended row: effect seen -> next state; past its time limit -> again or failed
        mark_orphans(tx, snap, procs)         # panes or pids not owned by a live attempt or job -> to stop
        advance_ideas(tx, files)              # idea state machine: files valid -> next state
        keep_sub_orchestrators(tx, snap)      # 1 live process per feature that is not merged or abandoned (I7, I13); it owns the open questions of the feature; a missing one -> attempt in launching
        finish_exited(tx, procs, files)       # result -> verifying; question -> blocked_on_user (asked); usage limit -> RR-11 (quota); else no_result -> rung 6
        promote_pending(tx)                   # each dependency met (I2) -> ready
        dispatch(tx, health(), pacing(tx), budget)  # lease, worktree in reserved, attempt in launching; no cap; not before not_before
        for a in live_attempts(tx, budget):
            state = liveness(a, snap, procs, files)
            renew_or_expire_lease(tx, a, state)   # a valid result ingested in this pass wins over an expiry
            apply_stall_ladder(tx, a, state)  # a doorbell or a stop is recorded as an effect, not done here
        observe_jobs(tx, procs)               # ended jobs -> results; a running job whose process is gone -> queued
        queue_due_jobs(tx, budget)            # task checks, gates, trial merges, rebases, rescues, fetch, push
        reconcile_worktrees(tx, trees)        # adopt, mark orphaned, queue a rescue job, mark landed trees for removal
        land_bundles(tx)                      # act on ended bundle and fast gate jobs; record the ref move; queue the next bundle
        integrate_features(tx)                # act on ended trial merge, rebase, full gate, push and fetch jobs; the feature state machine; ideas in flight of a merged feature -> a new feature; stack edges; resolver tasks
        check_invariants(tx)                  # I1..I13, repair or escalate
        roll_up_ideas(tx)                     # each task not cancelled is done, each task branch not abandoned is landed -> integrating; merged once the fetched tip of the primary branch shows the feature and each tree is removed or rescued
        effects = intended_effects(tx)        # every row in an intended state, stops first
    # Part 3: after the commit, do the effects. Each is keyed by the id of its row and is safe to repeat.
    for e in effects:
        do_effect(e)                # stop an orphan or an ended attempt; git worktree add or remove; write task.yaml + START.md;
                                    # launch an attempt; start a job; move a ref; send the doorbell; archive a request file
    write_heartbeat(now)            # time of the last full pass; `horch fleetd check` reads it
    platform.watchdog_ping()        # systemd_notify("WATCHDOG=1") on Linux; macOS and Windows use `horch fleetd check`
```

**A pass has 3 parts: the row first, then the effect, then the observation.** (1) fleetd reads the facts, with no transaction. (2) 1 transaction decides and writes rows. Each effect is a row in a state that says "intended": an attempt in `launching`, a worktree in `reserved`, a job in `queued`, a ref move recorded on its `branches` row, a process to stop. (3) After the commit, fleetd does the effects. The next pass observes each effect and moves its row on. No effect occurs inside the transaction. So a crash before the commit leaves no effect, and a crash after the commit leaves an intended row that the next pass completes.

- **Each effect is safe to repeat.** Its key is the id of its row. A launch names the attempt id: the attempt folder, the wrapper and its heartbeat carry it. Before a launch, fleetd looks for a live process of that attempt id, and does not launch a second one. A file that exists with the content of its row is not written again. An intended row with no effect after its time limit is done again or failed, by the same pass rule. An attempt in `launching` with no process past the start timeout is failed.
- **Stops come before launches.** In part 2, the observation of intended rows and the marking of orphans come before `dispatch`. In part 3, each stop runs before each launch. A new attempt never starts in a worktree while a process of an older attempt of that task is alive: fleetd launches only when the process of each older attempt of the task is gone.
- **An attempt id is never used again.** The attempt row is committed before its launch, so a restart of fleetd never makes a second attempt with the same id.

Dispatch order matters for "workers don't find work": after the commit, fleetd writes `task.yaml` and `inbox/START.md` before it launches the process, and passes the START.md path as the launch argument. The worker never searches; the task is the first thing it is given. If the process starts but never touches its files, the start timeout (90 s with no first heartbeat or activity) ends the attempt.

**Long work is a job. A pass never waits for it.** A pass only reads facts, decides, writes rows, and starts or observes jobs. Each piece of work that can take more than 1 s is a job: a task check, a fast gate, a full gate, the build of a bundle candidate, a trial merge, a rebase, a rescue, a `git fetch` and a `git push`. A job is a row in `jobs`, and it runs in its own process, outside the pass. fleetd supervises it like an attempt: the row first, then the process, alive by PID and start time, and stopped at its time limit. A job with the same kind and input key is not run 2 times: its result is used again. After a restart of fleetd, a `running` job whose process is gone goes back to `queued`. So a gate of 20 min or a check of 24 h does not delay the heartbeat.

**A pass has a work budget.** A pass handles at most a set count of rows of each kind (`pass_budget` in `fleet.toml`, a starting value to tune). It keeps a cursor for each kind in `pass_cursors`, and the next pass continues at the cursor, so each row gets its turn in a fair order. This is not a cap on parallel work (D8): a row that a pass does not reach is handled on a later pass. A pass must end before the hang limit of 30 s. `horch status` shows, for each kind, the age of the oldest row that waits for a pass.

**Holds on dispatch are visible.** When a ready task does not run, the cause has a name. `horch status` shows each hold on dispatch with its cause: a health floor, pacing of a pool, an unhealthy profile, a git job that holds the repository, a missing fast gate, or a `not_before` time. fleetd records an event when a global hold starts and when it ends. The health guards read free disk, machine load, free memory, the count of processes and the count of open files of the user from the machine probe of the platform layer. Each one pauses new dispatch past its safety floor, and dispatch resumes by itself. They are not a cap on parallel work (D8).

**Order inside 1 pass.** Results are ingested and acted on before a lease is expired: `ingest` and `finish_exited` run before `renew_or_expire_lease`. A valid `result.json` that is present in a pass wins over a lease expiry in the same pass. An illegal transition fails only its row (see Three rules that make the ledger trustworthy).

**The task state machine**

| State | Owner | Leaves when | Time limit |
| --- | --- | --- | --- |
| `pending` | fleetd | Each dependency is met (I2) → `ready`. A replan or `horch task cancel` → `cancelled`. In a fix round, a judge task waits here until the fix attempt of the judged task passes its structural check. | None. It waits for its dependencies. |
| `ready` | fleetd | Dispatch leases it, when the fleet clock is past `not_before` (the end of a retry backoff) and no hold on dispatch applies → `leased`. `horch task cancel` → `cancelled`. | None. `horch status` shows each hold on dispatch with its cause. |
| `leased` | fleetd | The first heartbeat of the wrapper → `running`. No first heartbeat or activity within the start timeout → the attempt ends `timed_out` (rung 6). | Start timeout, 90 s. |
| `running` | The worker attempt, watched by fleetd | A valid `result.json` → `verifying`. A valid question file → `blocked_on_user` (outcome `asked`). A usage limit or an outage → `ready` at once on the fallback persona of another provider, or `waiting_quota` (outcome `quota`, RR-11). The lease expires → `stalled`. Any other exit → rung 6. | The lease, 120 s, renewed only on earned activity; `max_minutes` of the wrapper. |
| `stalled` | fleetd | The process group of the attempt is stopped (rung 3) → rung 6: `ready` with `not_before`, or `failed`. A valid `result.json` present in the same pass → `verifying`. | The stop of the process group (on Linux, 20 s before SIGKILL). |
| `verifying` | The verifier, through a check job | Code kind: the check and the review scripts pass → `done`; they fail → outcome `rejected`, rung 6. Judgment kind: the structural check passes, and the task waits here for its judge task (see The judge pair); the judge task is `done` → `done`; the judge reports an open CRITICAL item → `ready` for a fix attempt. | The time limit of the check job (today's check timeout). While it waits for its judge, the time limits of the judge task apply. |
| `blocked_on_user` | The live process of the feature that owns the question, then you | The answer is recorded → `ready`. The worker resumes with the answer, or a fresh worker starts from the question, the answer and the committed work, in the same worktree. | None on the task. The question has its own limit, and past it the question escalates to you. |
| `waiting_quota` | fleetd | The pool resets → `ready`: the calendar time passes `reset_at`, or, with no `reset_at`, the pool probe reports ok. The lease is paused. | None. The usage window of the provider, in calendar time. |
| `done` | The verifier | Terminal. `done` means verified. Its task branch then follows the landing state machine. | None. |
| `failed` | The sub-orchestrator of the feature | It replans the task 1 time: the plan replaces the task → `cancelled`, or the task goes back → `ready` with its attempt count at 0. A task that reaches `failed` after its 1 replan (`replans` is 1), or a task past the time limit → `escalated`. | `replan_limit`, 30 min on the fleet clock, a starting value in `fleet.toml`. |
| `escalated` | You, through 1 item on the needs-you list | `horch task requeue` sets the attempt count to 0 → `ready`. `horch task cancel` → `cancelled`. | None. |
| `cancelled` | fleetd | Terminal. fleetd writes a rescue ref of its task branch, then moves the branch to `abandoned`. | None. |

Only the verifier moves a task to done. A stall, a crash, a missing result and a failed check each burn 1 attempt through rung 6, which either retries after a backoff or moves the task to `failed`, then `escalated`, so no path leaves a task held and forgotten. A question and a usage limit burn no attempt.

## Docs sync and adapter conformance

Docs sync turns "read each CLI's docs" into a scheduled job whose output is a pass or fail per CLI version, so a CLI update that changes a flag or a sandbox default blocks that CLI's workers before it can silently break a run.

Your repo already has `crates/herdr-docs-sync`, which mirrors herdr's docs from its sitemap into `herdr-docs/`, reports added, updated and removed pages, and prunes stale ones. The change is to generalise it into `agent-docs-sync` with one source entry per tool, and to attach a conformance test to each.

**Sources**

```toml
# docs-sources.toml
[herdr]
kind = "sitemap"
url = "https://herdr.dev/sitemap.xml"
version_cmd = "herdr --version"

[claude]
kind = "llms_txt"
url = "https://code.claude.com/docs/llms.txt"
md_suffix = ".md"
include = ["cli-reference", "env-vars", "settings", "hooks", "sandboxing", "skills", "mcp", "headless"]
version_cmd = "claude --version"

[codex]
kind = "llms_txt"
url = "https://learn.chatgpt.com/docs/llms.txt"
include = ["config-reference", "non-interactive-mode", "sandboxing", "hooks", "agents-md", "build-skills"]
also_git = "https://github.com/openai/codex"   # docs/ folder and release notes
version_cmd = "codex --version"

[agy]
kind = "llms_txt"
url = "https://antigravity.google/llms.txt"
include = ["cli/reference", "cli/headless", "settings", "hooks", "sandbox", "mcp", "skills", "rules"]
version_cmd = "agy --version"
```

**What one sync does**

1. For each source, record the installed binary's version and fetch the listed pages as markdown into `docs/<tool>/<version>/`, with a manifest of page hashes.
2. Diff against the previous version's manifest and write `docs/<tool>/CHANGES.md` listing changed pages and, for the reference pages, changed flags, env vars and settings keys (a simple regex pass over code spans).
3. Run the adapter's conformance suite against the installed binary (below). Write `docs/<tool>/<version>/conformance.json`.
4. If any check fails, the worker-profiles module marks every profile using that CLI as `unhealthy`, fleetd stops dispatching to it, and docs sync queues an idea "Update the \<tool> adapter for \<version>" with the diff and failing checks attached. You see it in `needs-you/`. When a later run of the conformance checks passes, the module marks each of those profiles healthy again.

Docs sync is a `horch` subcommand. fleetd schedules it as a job, so it runs outside the reconcile pass. Supervision and conformance are an operating concern outside the flow of 1 idea: a drift that docs sync finds becomes an idea, like any other work.

Each version command and each conformance launch starts through the launcher (SR-7), so it never has `ANTHROPIC_API_KEY`.

Run it daily, on `horch self-update`, and whenever fleetd sees a CLI binary whose version differs from the last conformance run (the repo's `exeid` module already notices binary changes).

**Conformance checks per adapter.** Each is a real launch in a throwaway task folder with a tiny fixed prompt, so it proves behaviour rather than reading docs.

| Check | Passes when |
| --- | --- |
| Launch headless | Process starts, emits its first stream event within 60 s and exits 0. |
| Private home honoured | A marker skill or rule placed only in the private config home is visible to the agent. |
| Exact MCP set | A test MCP server in the profile is listed; a server present only in the user's global config is not. |
| Shared write | The agent writes `outbox/result.json` in the task folder outside its cwd. |
| Sandbox boundary | A write outside the allowed paths is refused. |
| Context limit | The effective compaction setting reads back as 256000 where the CLI supports it. |
| Hooks fire | Tool start, tool end and stop events reach the wrapper. |
| No trust prompt | No approval or trust dialog appears (herdr status never shows `blocked`). |
| Stop gate | Exiting without a result triggers the stop hook (Claude Code) or the wrapper's repair pass. |

## Preservation analysis

fleetd keeps the behavior below. The full tables are in the 3 inventory files: `docs/specs/fleetd/preservation-04-behavior.md`, `docs/specs/fleetd/preservation-05-contracts-and-values.md` and `docs/specs/fleetd/preservation-06-wiring-and-platform.md`. This section lists every row that their summaries rank as `NOT MENTIONED`, and every row that the amended design names.

Row ids are those of the inventory files. Some ids exist in 2 files, so each id carries the number of its file: `04 S7` is row S7 of `preservation-04-behavior.md`, and `06 S7` is row S7 of `preservation-06-wiring-and-platform.md`.

### Behavioral invariants

| # | Behavior | Location | Business reason | Risk if lost |
| --- | --- | --- | --- | --- |
| 04 W6 | `worktree drop` and cleanup refuse a dirty tree, a branch with a `git stash` entry, unpublished unique commits and a HEAD that moved during the checks. A rescue ref does not save stashes. | `worktree_ops.rs:177`, `:192`, `:215` | No lost work. | A drop deletes unsaved or stashed work. |
| 04 F9 | Under the land lock, just before the fast-forward, the land checks that the task is still `landing` and held by this worker. | `land.rs:612`, `:567` | #136. | Cancelled work reaches the feature branch. |
| 04 F7 | A red land after a rebase resets the branch to its tip from before the land. | `land.rs:581`, `:600` | No attempt starts from an unchecked rebase. | The next attempt builds on a state that no check passed. |
| 04 F5 | A passed step stands only while its judged tree equals the tree now. New commits after a land reopen the land and every later step. | `finish/mod.rs:751`, `:765`, `land.rs:840` | #154. | Unchecked commits land. |
| 04 L5 | `done --unlanded` parks the task with its worktree; its dependents stay blocked; `requeue` resumes it on the same worktree. | `finish/mod.rs:1098`, `tasks.rs:791`, `:1499` | trace-fixes D16. | The work of a task that cannot land is lost, or its dependents start without it. |
| 04 L9 | `ledger unfinished`, `reopen` and `dismiss` keep a record of work that never landed. A finding is decided once. | `import.rs:964`, `:1010`, `cmd/taskledger.rs:1386` | `task-ledger.md:962`. | Unlanded branches and worktrees are not listed for a decision. |
| 04 W8 | Archive and trash refs only move forward, and each ref write is read back before a folder is removed. Trash refs expire after 30 days. | `triage/refs.rs:16`, `:49`, `:89` | #308. | A ref write overwrites other work; a removal follows a failed write. |
| 04 W9 | Triage commits modified and untracked files before ARCHIVE or DELETE, refuses a dirty or unique detached HEAD, and deletes a branch only when the trash ref holds its tip. | `triage/apply.rs:286`, `:324`, `:449` | #308. | Triage deletes uncommitted work or the default branch. |
| 04 F11 | Every git network call has a wall-clock bound (push 10 min, others 3 min), ssh and http stall limits, and never retries. | `vcs/network.rs:27`, `:39`, `:44` | 2 finish runs hung for more than 1 h 50 min on 2026-11-08. | 1 stalled push holds the land lock for hours. |
| 04 S7 | A reap ends the worker row and requeues the task in 1 transaction before any signal. The control loop exits only between passes. | `stop.rs:276`, `taskledger_watch.rs:35` | #216. | A restart in the middle of a reap loses the requeue; the kill counts as a crash. |
| 04 L6 | No 2 running tasks own overlapping paths, and an exclusive task runs alone. Inside 1 feature this stays. | `tasks.rs:689`, `:698`, `:805`, `:767` | "2 workers never edit 1 file" (`task-ledger.md:211`). | 2 workers of 1 feature edit 1 file at the same time. |
| 04 L8 | An operator requeue of a held task resets its attempt count (`attempt_floor`). | `tasks.rs:1499`, `:1514` | #168. | A task escalated once can never run again. |
| 04 S8 | `stop` takes a worker id only, signals only the recorded PID with its start time, and refuses a worker of another host. | `stop.rs:233`, `:355`, `procid.rs:119` | ADR 0002. | A PID from another machine signals an unrelated local process. |
| 04 S10 | The control loop restarts itself on the new binary when its binary file changes. | `taskledger_watch.rs:450`, `:490` | #216. | fleetd runs old code after `horch self-update`. |
| 04 M5 | Mail and outbox directories are mode 0700 and files 0600. A directory that another user owns or can write is refused. | `messaging/mailbox.rs:9`, `mail.rs:446` | #77. | Another local user plants lines that an agent reads as instructions. |
| 05 E-10, 06 S1 | `ANTHROPIC_API_KEY` is removed from every child process, every pane command line, every detached child, every updater and every probe, and the roster rejects a teammate file that sets it. In fleetd each process starts through 1 launcher that removes it (SR-7). | `runtime/process.rs:190`, `:277`, `workspace/paneshell.rs:11`, `harness/update.rs:507`, `quota_probe.rs:110`, `roster/validation.rs:481`, `:1681` | Absolute operator rule: every `claude` run uses the subscription login. | API billing instead of the subscription; a broken operator rule. |
| 06 S2 | herdr's own pane resume uses an environment that horch did not scrub. horch warns when `resume_agents_on_restore` is not false. fleetd does not use the native agent resume of herdr: after a herdr restart, it starts each attempt again through the launcher (FM-13), and a pane that herdr restored by itself is an orphan by I5 and is stopped. | `cmd/restore.rs:62` | The forbidden key can reach a resumed agent. | A herdr-resumed agent bills the API key. |
| 06 S3 | `agy` children lose `GEMINI_API_KEY`, `GOOGLE_API_KEY` and `GOOGLE_GEMINI_BASE_URL`, and the roster rejects them. | `harness/antigravity.rs:26`, `roster/validation.rs:495` | `agy` stays on your Google login. | API billing instead of the login. |
| 06 S9 | A sandboxed persona does not start on a host that cannot sandbox it (macOS needs `sandbox-exec`, Linux needs `bwrap` and `socat`, other OSes are refused). | `harness/claude.rs:1082` | A sandboxed persona never runs unsandboxed. | A worker writes outside its allowed paths. |
| 06 S4 | The private `CODEX_HOME` is a directory of links to the real codex home except `rules/`, so `auth.json` reaches your login. | `harness/codex.rs:325` | Codex keeps its login in a private home. | A codex worker starts with no login and stalls. |
| 06 S5 | The Prime agent dir links `auth.json`, so a refreshed token writes back to your file. | `harness/prime.rs:33`, `:907` | Prime keeps its login. | Prime workers lose their login. |
| 06 S6 | Every codex launch gets `--dangerously-bypass-hook-trust`. | `harness/codex.rs:1379` | Unattended hooks run. | Codex hooks never fire, and the stop gate fails. |
| 06 S7 | Agent CLI updates wait out a release cooldown; a CLI with unreadable release dates is held back. | `harness/cooldown.rs:1` | A new CLI release is not used in its first week. | A bad CLI release reaches every worker. |
| 06 S8 | Pane auto-update is off for claude and codex; `herdr update` runs only on your request. | `harness-defaults.md:12`, `codex.rs:1353`, `harness/update.rs:30` | `herdr update` can stop the server and close every pane. | Panes update or close in the middle of a task. |
| 06 S10 | Trust is written into the real stores under a lock, atomically, never for `/`, home or an ancestor of home, and never over an explicit codex `untrusted`. | `harness/trust.rs:1`, `:471` | Trust stays narrow. | A trust write opens home or overrides your choice. |
| 06 S11 | The worker channel relay and the report socket need Unix sockets. Phase 5 moves worker reports to files. | `messaging/channel.rs:104`, `messaging/report.rs:237` | Windows is first-class (D1). | Lost worker reports on Windows. |
| 06 S12 | The machine probe returns unknown disk, memory and load on Windows. | `runtime/machine.rs:322` | Input of the health guards. | The health guards have no input on Windows. |
| 06 S13 | The quota probes read limits through each CLI's own client, so horch holds no credential. | `routing/quota.rs:9`, `quota_probe.rs:83`, `:140`, `:195` | `waiting_quota` and pacing depend on these readings. | No usage reading, or a held credential. |
| 06 S14 | The source design assumed systemd or launchd, `WatchdogSec`, `/proc`, inotify and Unix signals to a process group, and it did not name Windows. D1, A1 and A15 answer it: the platform layer, and hang detection on all 3 OSes. | `preservation-06-wiring-and-platform.md`, sections 7 and 8 | Windows is first-class (D1). | fleetd does not run, or a hung fleetd is not restarted, on an OS that the source design did not name. |
| 06 S15 | The OpenCode, pi and Prime adapters carry over. | `routing/quota.rs:46` | They give the `opencode-zen` and `local` pools. | Those pools are lost. |

Rows 04 W6, L5, L9, W8, W9 and L8 are requirements RR-13 to RR-18, and their tests are in the gate of Phase 3. The worktree reconciler stays in report-only mode until these tests are green, so no cleanup of Phase 3 runs before they hold.

### Contract surface

| Command or format | Who depends on it | Shape | Exit codes or status |
| --- | --- | --- | --- |
| Mail contract (05 C-1, M-1 to M-6, V-15) | Interactive sub-orchestrators and you | `horch tell <role> "<msg>"`; `[<role>]` sender tag, untagged text is you; `[orchestrator re #<id>]` answers; redelivery at 30, 60, 120, 240, 300 s, then every 300 s | 0 sent or queued; 1 error |
| Report of unlanded work (05 M-7) | You and `ledger unfinished` | `... \| UNLANDED: <claim>: <reason>`, `... \| UNREGISTERED: <entry>` | Read by `taskledger/import.rs:462` |
| `horch self-update` (05 C-14) | You and friends | `horch self-update [--check]`; also restarts the daemon (OR-6) | 0; 1 error; `--check` exits 0 either way |
| Release assets (05 F-20) | `horch self-update` | `multi-herdr-v<version>-<target>.tar.gz` or `.zip`, `SHA256SUMS`, 5 targets | - |
| Ticket (05 F-1) | ticket-writer, `plan.yaml` | `docs/tickets/<slug>/<NN>-<name>.md`, format C1 (`docs/specs/ticket-format.md`) | - |
| Map (05 F-2) | `add-map` today | `MAP.md`, replaced by `plan.yaml` | - |
| Project facts (05 F-11) | every sub-orchestrator | `horch note --fact`, `<state>/facts/<hash>.txt` | - |
| Evidence logs (05 F-8) | `ledger steps`, reviewers | `~/.horch/evidence/<run>/<task>/...`, land locks under `evidence/locks/` | - |
| Message spool (05 V-17) | mail readers | lines over 600 characters go to a spool file, kept 7 days | - |
| Exit codes | every script that calls `horch` | `crates/horch/src/exit.rs` | 0 success, 1 failure, 2 collector held, 3 refused |

### Domain assumptions

| Value | Location | Why this value | Impact if changed |
| --- | --- | --- | --- |
| Land lock 1 h, stale margin 10 min, 5 tries; check timeout 20 min, max 24 h (05 V-10) | `finish/land.rs:18-40` | `task-ledger.md:661` | A lower value fails long test suites; no lock lets 2 lands race. |
| `vcs/landing.rs` lock stale 1 h, timeout 30 s, heartbeat 60 s (05 V-11) | `vcs/landing.rs:20-22` | not recorded | As V-10. |
| Code-sweep at most 2 requeues; review rounds light 1, full 3 (05 V-14) | `finish/codesweep.rs:280` | format C1 | Review loops, or no review. |
| Git network bounds: push 10 min, others 3 min (04 F11) | `vcs/network.rs:27` | 2 finish runs hung for more than 1 h 50 min | A stalled call holds the land lock. |
| Schema backups: 3 kept; `BUSY_TIMEOUT` 30 s (05 V-23, L-4) | `store.rs:14`, `schema.rs:520` | history before a migration | Lost history after a failed migration. |
| Project facts: 100, 2048 bytes each (05 V-25) | `project_notes.rs:7-9` | oldest expire first | Facts lost, or prompt bloat. |
| Routing: tight 0.85, exhausted 0.98, pace 1.25x at 0.50 used, probe 15 min (05 V-26) | `routing/policy.rs:66-82` | design 13.4 | Wrong refusal or spend. The pace factor in code is 1.25, not 1.5. |
| Inline message limit 600 characters; spool kept 7 days (05 V-17) | `message.rs:17-21` | a busy TUI splits lines | Split lines lose the tag. |
| Harness update cooldown (`update_cooldown_days`) | `harness/update.rs:275` | a release must be more than a week old | A bad CLI release reaches every worker. |

### Wiring map

| Interface | Implementation | Registration | Verified |
| --- | --- | --- | --- |
| `Harness::command` and `build_command` (env scrub) | 1 builder per harness: claude, codex, agy, opencode, pi, prime, none | `harness/mod.rs:121`, `:224` | no |
| Launch flow | skills, briefing, trust, window, `prepare`, command, scrub, run, discovery, `finish` | `harness/launch.rs:519` | no |
| OpenCode adapter (06 S15) | `OPENCODE_CONFIG_CONTENT`, `--auto`, `--pure`, `skills.paths`, session list resume | `harness/opencode.rs:190` | no |
| pi and Prime adapters (06 S15) | `--model`, `--thinking`, `--skill <dir>`, `--session-id`; Prime private agent dir and daemon socket | `harness/pi.rs:134`, `prime.rs:1`, `:1048` | no |
| Private `CODEX_HOME` of links (06 S4) | links every entry except `rules/` | `harness/codex.rs:325` | no |
| Quota probes (06 S13) | claude `get_usage`, codex `account/rateLimits/read`, `agy /usage`, pi `ollama list` | `routing/quota_probe.rs:83`, `:140`, `:195`, `:282` | no |
| Context readers | transcript tail, 256 KiB window, marker cache | `telemetry/context.rs:1` | no |
| Update check and cooldown (06 S7, S8) | per-CLI lock, 24 h freshness, release cooldown; pane self-update off | `harness/update.rs:1`, `harness/cooldown.rs:1` | no |
| Trust writes (06 S10) | `ensure_trusted` before `prepare` | `launch.rs:576`, `trust.rs:471` | no |
| herdr calls | herdr CLI only, JSON envelope, own process group or job object, timeout | `workspace/herdr.rs:1`, `:676` | no |
| Machine probe (06 S12) | `sysctl`, `vm_stat`, `statvfs`, `getloadavg`, `/proc/meminfo`; unknown on Windows | `runtime/machine.rs:322` | no |

### Credential source inventory

Names and paths only. No secret and no value of a variable is written here. `Verified in new code` is "no" for every row, because the new code does not exist.

| Credential | Runtime source | Path or key name | Rotation | Proxy or cert required | Verified in new code |
| --- | --- | --- | --- | --- | --- |
| Claude subscription login | Claude's own store; horch sets no `CLAUDE_CONFIG_DIR` and removes `ANTHROPIC_API_KEY` | `$CLAUDE_CONFIG_DIR` or home; the Keychain on macOS | refreshed by Claude | none recorded | no |
| Codex ChatGPT login | the private home's link to the real file | `$CODEX_HOME/auth.json` | yes | none recorded | no |
| Google login for `agy` | your login; `GEMINI_API_KEY`, `GOOGLE_API_KEY`, `GOOGLE_GEMINI_BASE_URL` removed | `~/.gemini/` | yes | none recorded | no |
| OpenCode providers | your OpenCode credentials, kept by `--pure` | OpenCode's own store | provider dependent | none recorded | no |
| Prime login | linked into the private agent dir; refreshed through the link | Prime `auth.json` | yes | none recorded | no |
| pi | local Ollama, no login | - | no | none recorded | no |
| git fetch and push | your SSH key and agent; horch adds timeouts, never a key | SSH agent (`git-alt` host here) | no | none recorded | no |
| `gh` for the `pr` setting | `gh auth login` store | `gh` config | yes | none recorded | no |
| `horch self-update` | unauthenticated GitHub API | - | no | none recorded | no |
| npm registry for cooldown dates | unauthenticated | - | no | none recorded | no |

### Failure mode analysis

| Risk | Likelihood | Impact | Mitigation |
| --- | --- | --- | --- |
| Stashed or uncommitted work deleted at cleanup (04 W6, W9) | Medium | Lost work | Invariant I12; triage commits before ARCHIVE or DELETE. |
| Cancelled or half-rebased work lands (04 F9, F7, F5) | Low | Unchecked or unwanted code on the feature branch | Ownership check under the land lock; reset after a red land; a pass stands only for the tree it judged. |
| A ref write overwrites work, or removal follows a failed write (04 W8) | Low | Lost work | Forward-only refs, read back before removal; invariant I10. |
| A task that cannot land loses its work, or its dependents start early (04 L5, L9, 05 M-7) | Medium | Lost work | Task-level park with its worktree; `unfinished`, `reopen`, `dismiss`; the `UNLANDED:` report. |
| A stalled git network call holds the land lock (04 F11) | High: seen on 2026-11-08 | Stuck fleet | A time limit on every git network call, which runs as a job outside the reconcile pass. |
| A restart in the middle of a reap loses the requeue (04 S7) | Low | Stuck task | Requeue in 1 transaction before any signal. |
| An escalated task can never run again (04 L8) | Medium | Stuck task | Attempt reset on your requeue. |
| 2 tasks of 1 feature edit 1 file (04 L6) | Medium | Conflicting commits | Own exclusion inside 1 feature. |
| Long suites fail, or 2 lands race (05 V-10, V-11) | Medium | Stuck fleet | Keep today's land lock and check timeout values. |
| A newer schema is opened by an older binary (05 L-3) | Medium with automatic updates | Corrupt or locked ledger | Newer schema refused; a CLI restarts an older daemon (OR-6). |
| A failed migration loses history (05 L-4) | Low | Lost ledger history | Backup before each migration, 3 kept. |
| A development build migrates the shared ledger (05 L-5) | Medium | Installed horch refuses the ledger | Dev-build guard (`HORCH_ALLOW_LEDGER_MIGRATE`). |
| A test or scratch run writes the shared ledger (05 E-1, E-2) | Medium | Corrupt shared state | `HORCH_LEDGER_DB` and `HORCH_STATE_DIR` isolation stay beside `FLEET_ROOT`. |
| `ANTHROPIC_API_KEY` reaches a child (05 E-10, 06 S1, S2) | Medium | API billing; broken operator rule | 1 launcher for every process, and an `arch_scan` rule against a direct process start (SR-7); scrub on every child, pane line, detached child, updater and probe; roster check; no native herdr resume, so a pane that herdr restored by itself is stopped as an orphan. |
| Google keys reach `agy` (06 S3) | Low | API billing | Scrub and roster check. |
| Another local user plants instructions or sends a command (04 M5) | Low | Agent acts on planted lines; unapproved work starts or approved work stops | Mode 0700 and the owner check on every mail, outbox and request directory; the peer-user check on the command channel (SR-5). |
| A sandboxed persona runs unsandboxed (06 S9) | Low | Writes outside allowed paths | Refuse to start on a host that cannot sandbox it. |
| `stop` signals a PID of another host (04 S8) | Low | An unrelated process is killed | Refuse a worker of another host. |

## Failure modes and effects

Every failure you listed, plus twenty-six more found while reading the code, the CLI docs and your new requirements, is either prevented by construction or caught by a timer in code within minutes, and each traces to a requirement in the last column.

"Caught within" assumes the default timers (reconcile every 5 s, git scan every 60 s, lease 120 s, stall after 5 min). Rows FM-1 to FM-8 and FM-25 are the failures you reported. The table "Where each rule becomes green" in the Migration plan gives the 1 phase whose gate makes each row green.

| ID | Failure mode | Effect | Mechanism that removes or catches it | Caught within | Req |
| --- | --- | --- | --- | --- | --- |
| FM-1 | Orchestrator forgets to create a task | Part of an idea is never built, silently | No AI creates tasks: they come only from a validated `plan.yaml`, committed in one transaction after your approval; the coverage check fails the plan if any requirement has no task | Prevented | FR-3, FR-4, FR-10 |
| FM-2 | Worker starts but doesn't find its work | Idle pane, task never runs | Push dispatch: fleetd binds the task to the attempt and writes `task.yaml` before launch; no claim, persona match or run lookup | Prevented; a launch that never starts fails at 90 s | FR-5 |
| FM-3 | Worker holds a task and sits idle | Task blocked for hours or days | Leases renewed only on earned activity; an exec worker that asks a question exits and its task waits in `blocked_on_user` with no process; an interactive persona's waits are capped by `max_wait_s` and need a live question or review row behind them | About 7 min | RR-2 |
| FM-4 | Agent process dies silently | Task looks running | PID and start time checked every 5 s pass; wrapper heartbeat stale at 30 s | 5 to 30 s | RR-1 |
| FM-5 | Agent alive but stuck at a login, rate-limit, trust or error prompt | Looks alive forever | A usage limit or a provider outage is not a stall: no attempt is burned, the task restarts at once on the fallback persona when it is on another provider, else waits in `waiting_quota` with its lease paused until the pool resets. A login, trust or error prompt: herdr `blocked` state, `Notification` and `PermissionRequest` hooks, no activity past `stall_after`; profiles pre-approve so any dialog is a defect | Immediately for dialogs and usage limits, 5 min otherwise | RR-1, RR-11, FR-8 |
| FM-6 | Worker finishes but never reports | Work lost or redone | Stop gate refuses to end the turn without `result.json` or a question file; exec mode exit without result triggers one repair resume; the next attempt reuses the same worktree | Prevented; worst case one retry that starts from the work | RR-3 |
| FM-7 | Watchdog misses updates | Any of the above lingers | One supervised daemon, level-triggered pass every 5 s, full scans instead of trusting events, actions in code instead of mail to an LLM | 5 s | RR-4 |
| FM-8 | Stacked ideas get merged | Wrong plan, tangled branches | Each idea is its own file, ledger row and sub-orchestrator; a fresh process per phase whose only memory is that idea's folder; 1 process never plans 2 ideas at the same time | Prevented | FR-1, FR-2 |
| FM-9 | Worker claims done but work is broken | Bad code lands | Only the verifier moves a task to done: the acceptance commands, then for a code kind today's review scripts, and for a judgment kind the structural check and the paired judge. The paired judge comes in Phase 6 (FM-33) | At verification | RR-3 |
| FM-10 | Plan has a cycle, missing profile or uncovered requirement | Deadlock or gap | Plan validator rejects before commit; the sub-orchestrator reruns ticketing with the errors | Prevented | FR-3 |
| FM-11 | fleetd crashes mid-pass | Half-applied state: an effect with no row, or a row that claims an effect that did not occur | The ledger changes of a pass are 1 SQLite transaction, and no effect occurs inside it. Each effect is a committed row in an intended state first, then the effect after the commit, keyed by its row id and safe to repeat, then the observation on the next pass. A crash before the commit leaves no effect; a crash after it leaves an intended row that the next pass completes or fails. Before a launch fleetd looks for a live process of the attempt id, and stops come before launches. Ingestion is idempotent; the service manager of the OS restarts fleetd; the next pass rebuilds from ledger and files | Under 30 s | RR-7, RR-8 |
| FM-12 | fleetd hangs, or a long gate delays its heartbeat | Nothing moves, or a healthy fleetd is restarted for ever | Long work is a job outside the pass, and each pass has a work budget, so a healthy pass always ends before the hang limit. On Linux the systemd watchdog (`WatchdogSec=30`) kills and restarts it when the pass stops pinging. On macOS a launchd timer job, and on Windows a scheduled task, runs `horch fleetd check` every 30 s, and it restarts the service when the heartbeat is older than 30 s of awake time. Each `horch` CLI command makes the same check. Each restart stops the old process by its PID and start time and waits until the fleetd lock is free before it starts the new one, so 2 fleetd processes never run at once | 30 s to 60 s of awake time on every OS | RR-7, PL-5, RR-5 |
| FM-13 | herdr server restarts | Every agent process is gone, or herdr resumes an agent with an environment that horch did not scrub | Reconciler sees dead PIDs and missing panes, fails those attempts, rescues their worktrees and retries in them; each new attempt starts through the launcher, which removes `ANTHROPIC_API_KEY`; fleetd does not use the native agent resume of herdr, and a pane that herdr restored by itself is an orphan by I5 and is stopped; sub-orchestrators restart from their folders | 5 to 30 s | RR-4, SR-7 |
| FM-14 | Two workers on one task | Conflicting commits | Lease row is unique per task and a worktree has one owner; fleetd stops the old attempt by recorded PID before starting the next (your ADR 0002) | Prevented | RR-2 |
| FM-15 | Typed prompt or command request lost or delivered twice | Missed or duplicated instruction | Nothing meaningful is typed. The doorbell is constant and idempotent; content lives in files. Each command request has a `request_id`: fleetd applies it and writes its `requests` row in 1 transaction, a repeated id gets the stored result with no second effect, and a request file is archived only after its row is committed | Prevented | FR-7, RR-8 |
| FM-16 | One AI conversation buried in ideas, worker mail and wakes | Context churn, ideas blur | There is no long-lived orchestrator conversation. Reports go to files and the ledger; a sub-orchestrator sees only its own feature's questions; you read `horch ideas` and `needs-you/` | Prevented | OR-3, FR-2 |
| FM-17 | Codex sandbox blocks writes to the task folder | No result, looks like a stall | Task folder in `writable_roots`; conformance test proves the write; the old `horch ledger` sandbox escape is no longer needed | Before dispatch | SR-1, FR-8 |
| FM-18 | Trust or approval prompt on first launch | Pane frozen | Trust pre-seeded in each private config home; conformance check "no trust prompt" | Before dispatch | FR-8 |
| FM-19 | CLI update changes a flag or default | Every worker of that CLI fails | Docs sync plus conformance on version change; failing CLI marked unhealthy | Before dispatch | FR-8 |
| FM-20 | Context drift past 256k, or a CLI with no compaction setting | Quality drops, loops | CLI compaction settings; wrapper ends the attempt with a handoff at the 256k context limit | At the limit | FR-9 |
| FM-21 | Retry loop burns quota | Cost with no progress | Bounded attempts and backoff, one replan, then escalation; a per-attempt limit (`max_minutes`, in awake time) as a hang guard | Bounded | RR-6 |
| FM-22 | Orphan processes or panes after a reap | Leaked CPU, stray edits | Process-group kill plus exact pane close; invariant I5 stops anything not owned by a live attempt | 5 s | RR-4 |
| FM-23 | Half-written, oversized, linked or foreign file read | Corrupt result, or fleetd runs out of memory and restarts on the same file | Temp file, fsync, rename; readers skip `*.tmp`; a log is read only to its last complete line; schema validation. A worker file is read only when it is a regular file, opened without following a link, under its size limit, inside its own attempt folder and with the ids of its path; a file that fails is recorded as rejected and not read again | Prevented | RR-8, SR-6 |
| FM-24 | Ledger write contention, a stray writer, or 2 fleetd processes | Busy errors, lost updates, duplicate workers or competing branch moves | Single writer; everything else opens read-only. fleetd holds an exclusive file lock for its whole life, and a second fleetd exits at once; each write transaction checks the generation of the `fleetd` row; a restart stops the old process by its PID and start time and waits for the lock | Prevented | RR-5 |
| FM-25 | Worker dies and leaves its worktree on disk | Paid-for work invisible, later deleted or redone | The worktree was leased before it existed, so owner death marks it `orphaned` in the same pass; a verified rescue ref is written before anything else; a rescue that fails its verification keeps the tree and puts 1 item on the needs-you list; the next attempt reuses the tree | 5 to 30 s | FR-11, RR-9 |
| FM-26 | A worker or script creates a worktree outside fleetd | Unknown branch and directory | Profiles deny `git worktree`; `arch_scan` keeps it out of horch; the reconciler adopts any unknown tree as orphaned and rescues it | 60 s | FR-11, RR-10 |
| FM-27 | A worktree with unmerged work is removed | Work lost | fleetd removes only `landed` trees or trees with a verified rescue ref; a directory that vanishes without a removal event is escalated | Prevented inside the fleet; 60 s otherwise | RR-9 |
| FM-28 | Worktrees fill the disk | Builds fail, everything stalls | Size tracked on each git scan; over budget, fleetd removes landed and rescued trees of finished ideas and pauses dispatch before it would touch an active one | Before the budget is hit | RR-10, OR-1 |
| FM-29 | An idea waits on you forever | Idea silently stuck | Waiting is a state, not a stall: the idle process stops after 30 min to free memory, herdr notification and hourly reminders, parked with a note after 7 days | Visible at once; parked at 7 days | OR-3 |
| FM-30 | An unclear idea is built wrong | Runs wasted on the wrong thing | Research and interview first; `assumptions.md` with no open items; no spec until you approve; every requirement cites an approved assumption or your answer | Prevented at approval | FR-10 |
| FM-31 | A sub-orchestrator dies mid-interview or mid-phase | Answers or a half-written spec lost | Each answer recorded with `horch idea note` as it is given; fresh process per phase resumes from the folder; the reconciler restarts a missing pane | 5 to 30 s | FR-2, RR-7 |
| FM-32 | A model approves its own assumptions or forges an approval | Work starts without your yes | `horch idea approve` refuses inside any fleet attempt and profiles deny it; fleetd writes `approval.json` with the hash you saw and ignores any other; invariant I7 checked every pass | Prevented | FR-10 |
| FM-33 | A document passes a structural check but is wrong | A wrong design, report or doc is accepted | Plan validation requires a paired judge task in a different session, on a different provider when the roster has one; a CRITICAL miss blocks and a fresh session fixes it; a fresh judge checks only the earlier CRITICAL items and the change; after 2 fix rounds it goes to you | At the judge task | FR-4, RR-3 |
| FM-34 | Daemon and CLI of different versions | Old code acts on a newer ledger, or the reverse | fleetd is a subcommand of the single `horch` binary; `horch self-update` restarts the daemon on the new binary at once; a CLI that finds an older daemon restarts it; a newer schema is refused | Prevented; at the next CLI call otherwise | OR-6 |
| FM-35 | A question is never answered | The task parks for ever | Each question is a ledger row with an owner, a state and a time limit; fleetd starts the owner on each pass and repeats delivery until an answer is recorded; past the limit it escalates to `needs-you/` with a herdr notification | At the time limit | RR-12, OR-3 |
| FM-36 | fleetd restarts while a bundle or its bisect is in flight | A bundle is built 2 times, or a branch stays in `in_bundle` for ever | Each bundle and each bisect part is a `bundles` row with its members in `bundle_members`; a branch is in at most 1 bundle that is not ended; fleetd continues each bundle from its row | 1 pass after the restart | RR-7, I12 |
| FM-37 | A branch tip moves under fleetd (a push by you, a rebase, another landing) | A landing overwrites a commit, or a gate result is used for another tip | Each ref move is a compare-and-set on `seen_tip` or `seen_remote_tip`; on a mismatch fleetd moves nothing, records the new tip and makes the gate result stale; a moved feature tip drops each bundle in `building` or `gating`, and no rebase starts while a bundle is in `gating` | 1 pass | I9 |
| FM-38 | A task branch conflicts or fails the gate after its worker exited | The branch never lands, and its idea never reaches `integrating` | fleetd owns `conflicted` and `gate_failed`: it starts a resolver attempt or a fix attempt in the worktree; each landing state has a time limit, and past it 1 item goes on the needs-you list | At the time limit | I9, RR-6 |
| FM-39 | A task is cancelled, or its rescue fails | Its idea and its feature wait for ever | fleetd writes a rescue ref and moves the branch of a cancelled task to `abandoned`; the roll-up skips cancelled tasks and abandoned branches; an idea is `merged` when each tree is `removed` or `rescued`; a failed rescue puts 1 item on the needs-you list | 1 pass | I8, I12 |
| FM-40 | Rescue refs delete unique work, or grow with no bound | Work lost, or the repository grows for ever | 2 classes, both owned by fleetd: `unique` is kept until you run `horch worktree restore` or `horch rescue drop`; `superseded` expires 30 days after its feature ends; over `rescue_unique_budget`, 1 item goes on the needs-you list and nothing is deleted | At the budget | RR-9, I10 |
| FM-41 | You merge a pull request while an idea of its feature is in flight | The idea has no feature and no owner, or its branches land on a merged feature | fleetd records the feature as `merged` and moves each idea in flight to a new feature from the fetched primary tip, with its sub-orchestrator and pane; each task branch gets a rescue ref and a rebase | 1 fetch period | I7, I13 |
| FM-42 | The lower feature of a stack is merged or abandoned | The upper feature keeps a base that is gone | fleetd clears `stacked_on`, writes a rescue ref, rebases the upper feature on the primary tip, runs both gates again and moves the base of its pull request; a conflict opens a resolver task | 1 fetch period | I10 |
| FM-43 | A push fails, or you close a pull request without a merge | The feature waits for ever with no visible cause | `push_state` and `pr_state` are facts on the feature row; a push retries with a backoff, and 3 failures give 1 needs-you item; a closed pull request gives 1 needs-you item: open it again, or run `horch feature abandon` | 3 push attempts; 1 fetch period | OR-3 |
| FM-44 | The needs-you list only grows | You cannot see what needs you | Each item has a stable key and a state; a producer writes by key; only `open` items are shown; an item that is not open is removed after 30 days | 1 pass | OR-3 |
| FM-45 | A ready task does not run, and nothing says why | The fleet looks stuck | `horch status` shows each hold on dispatch with its cause; fleetd records an event when a global hold starts and ends; the health guards cover memory, processes and open files also | At once | OR-1, OR-5 |

## Migration plan

Migrate in nine phases, each a set of horch changes with a test as its exit gate. After the tests of Phase 0 and the platform layer of Phase P, start with the supervised loop, leases and worktree rescue, because they stop the daily stalls and the lost work with the least code.

Each phase keeps the fleet usable. A phase is released when the tests of its gate pass, and it is on for every user at once. The tests are the only gate.

**Rollout is automatic and immediate.** fleetd is a subcommand of the single `horch` binary, so the CLI and the daemon cannot have different versions. `horch self-update` installs a new release and restarts the running daemon on the new binary at once. This is safe because all state is in the ledger and in files, and the loop derives everything again on its next pass. Today the watchdog already restarts itself when its binary changes (`preservation-04-behavior.md`, row S10). A CLI that finds an older daemon restarts it. Harness updates keep today's rule: a release must be more than a week old (`update_cooldown_days`).

**Schema migration is forward-only and runs at daemon start.** Today's safety stays (`preservation-05-contracts-and-values.md`, rows L-3, L-4, L-5): a newer schema is refused, a backup file is written before each migration, and a development build does not migrate the shared ledger.

**Who writes the ledger before Phase 5.** fleetd and the `horch` commands both write in Phases 1 to 4. Each writer calls the same `transition()` with the same table of allowed transitions, SQLite gives 1 writer at a time, and fleetd treats each row as a fact that it reads again on each pass (see Ledger). No rule of Phases 1 to 4 needs the single writer. fleetd is the only writer from Phase 5.

**Before Phase 6, a run stands for its idea and its feature.** In Phases 4 and 5, each run is 1 feature row, and its run branch is the feature branch. Until Phase 6, the orchestrator of the run has the role that this document gives to the sub-orchestrator of a feature: it owns the questions of the run and replans a failed task. Where a phase before Phase 6 reads "idea", read "run".

**Each gate names its rows.** The gate line of each phase names each requirement and each failure mode that its tests make green. Each one is in exactly 1 gate (see Where each rule becomes green). A requirement or a failure mode is green when each of its tests passes and is off the list of expected failures. Each gate also has the test of its phase in "What a release does to a run in flight".

1. **Phase 0: reproduce every failure in tests.** Add scenarios to `crates/horch-e2e` using the fake harness and the existing fault points: an agent that never claims, one that claims and stops, one that exits without `done`, a killed watchdog, a herdr server restart, two ideas sent back to back, a worker killed with dirty and untracked files in its worktree. Add red tests for: several task branches ready at the same time, and none is forgotten; 2 features that conflict; a usage-limited worker that must not be reaped; a machine sleep that must not expire a lease; a daemon restart on a new binary in the middle of a run; a hung fleetd that is restarted, on each OS. CI runs the test suite on Windows; today the Windows job only builds (`.github/workflows/ci.yml`, job `build-windows`). Today most should fail. Phase 0 keeps a list of expected failures. Each red test of Phase 0 is on the list, with the phase that makes it green. A test run passes when each test that is not on the list passes, and each test that is on the list fails. A phase removes its tests from the list. "Green" for a failure mode means that its test passes and is not on the list. *Gate:* FM-1 to FM-8 and FM-25 each have a red test, and each new scenario above has a red test.
2. **Phase P: the platform layer and the move of the existing OS-specific code.** Create the platform interface and the 3 OS modules in the leaf crate `crates/horch-platform`. Then move each of the 178 production sites of `preservation-06-wiring-and-platform.md`, section 6, behind the interface, together with the production sites of the forms `cfg(any(...))` and `cfg!(...)` that section 6 counts. All sites move. A site that a later phase deletes moves also (for example the 45 socket sites of row 06 S11, which Phase 5 removes), and that later phase then deletes the platform function together with its callers. This phase changes no behavior. A gap of 1 OS that exists today stays a gap, and the OS module states it as an explicit "not supported" value, so each gap of an OS is visible in 1 file. A test that fails on Windows for a gap that exists today is fixed, or it is skipped on Windows by name with the row of section 6 that states the gap. Add the `arch_scan` rule of PL-2. Phase P needs only what Phase 0 and Phase P give. A red test of Phase 0 is on the list of expected failures, so it does not block the gate. *Gate:* PL-2 and PL-6 green.
3. **Phase 1: `horch fleetd`, supervised and level-triggered.** Move the watchdog's pass into a long-lived daemon under the service manager of each OS through the platform layer (on Linux a systemd user unit with `WatchdogSec`; on macOS a launchd timer job and on Windows a scheduled task that run `horch fleetd check` every 30 s), a 5 s pass that writes a heartbeat, and the invariants I4, I5 and the process part of I1 (each `leased` or `running` task has 1 attempt with a live process or pane). Add the 3-part pass with its intended rows (an attempt in `launching`), `transition()` with its table of allowed transitions, the `jobs` table, the `pass_cursors` work budget, the fleet clock, the fleetd file lock, the generation number of the `fleetd` row, the launcher (SR-7), the `needs_you` table and `horch fleetd check`. Add the fleetd concerns to the platform interface: service install and restart, hang detection, sleep and wake, file watching, the fleetd lock, and the transport of the fleetd command channel. It acts in code (stop by recorded PID, requeue, respawn) instead of only mailing the orchestrator. Remove the lazy `ensure` start. **Phase 1 stops the old watchdog.** At its first start, fleetd lists each recorded watchdog, stops each one by its PID and start time, and takes its runs. `horch ledger watch` then only points at fleetd. *Gate:* green: RR-4, RR-7, OR-2, OR-4, OR-5, OR-6, PL-1, PL-3, FM-4, FM-7, FM-11, FM-12, FM-22, FM-34, FM-44. Tests: after a herdr restart, fleetd sees the dead processes, ends the attempts and queues the tasks again with the worktree handling of today (the part of FM-13 that Phase 1 can do); a job of the fake harness runs for 5 min while fleetd stays healthy and is not restarted; an update with a live worker and a live old watchdog leaves 1 control path.
4. **Phase 2: leases and capped waits.** Schema v8 adds a `leases` table. Renewal comes only from earned activity. `worker_waits` rows of an interactive persona get `max_wait_s` (default 1 h) and must point at a live question or review; an exec worker that asks a question exits, and its task moves to `blocked_on_user` with the outcome `asked`. Replace "3 checks of 600 s" with the stall ladder. Add the `waiting_quota` state, the required `fallback` persona in the roster, and the restart on a fallback of another provider. Add the lease part of I1, rung 6 with `not_before` on the fleet clock, the `questions` table, `horch task requeue` and `horch task cancel`. *Gate:* green: RR-1, RR-2, RR-6, RR-11, FM-3, FM-5, FM-14. Tests: a usage-limited worker is not reaped and burns no attempt; the idle-claim scenario is caught in under 10 min.
5. **Phase 3: worktree leases and rescue.** Extend the existing `worktrees` table with state, owner, base and rescue columns, and make fleetd (through `horch spawn` until Phase 4) the only code that runs `git worktree add` or `remove`, writing the row first. Add the worktree reconciler, the verified rescue to `refs/horch/rescue/`, reuse by the next attempt, invariant I6, `horch worktrees` and `horch worktree restore`. Carry over the refusals of today's drop, cleanup and triage, and the record of unlanded work (RR-13 to RR-18). The reconciler runs in report-only mode, and lists the orphans already on your disk, until the tests of RR-13 to RR-18 are green. Only then does it act. *Gate:* green: FR-11, RR-9, RR-10, RR-13 to RR-18, FM-13, FM-25 to FM-28. Tests: a worker killed with dirty and untracked files loses nothing; after a herdr restart, each worktree is rescued and the next attempt uses it again (the full FM-13).
6. **Phase 4: push dispatch and the result gate.** `horch spawn` binds a task before the pane exists and writes `task.yaml` and `START.md`; `CLAIM_TASK` goes away. Add Stop hooks for Claude Code, Codex and Antigravity and a `horch result` command that writes `result.json`. fleetd runs the finish steps instead of the worker detaching them and waiting for a typed NOTE: the verifier runs the check and the sweeps and moves the task to `done`, and then the landing state machine lands its task branch. Land is a step of landing, not of the verifier. The Stop hooks and the repair pass accept a question file in place of `result.json`, and the outcomes `asked` and `quota` burn no attempt. For a code kind the review scripts carry over unchanged: code-sweep rounds, findings F<n>, the answers file, waivers and verification profiles, with their limits (V-14). Move the tables `features`, `branches`, `bundles`, `bundle_members`, `gate_runs` and `remote_tips` here from Phase 6, with the `git fetch` and `git push` jobs. In Phases 4 and 5, each run is 1 feature row, and its run branch is the feature branch. The landing state machine, the feature state machine of that 1 feature and the 2 gates work on it. Add invariants I2 and I8 to I12, the `ingested` table with the checks of SR-6 for `result.json`, the 2 classes of rescue refs with `horch rescue drop`, and `horch feature abandon`. A task of a judgment kind gets its structural check in Phase 4. Its paired judge comes in Phase 6, and until then it passes on its structural check and today's finish steps. *Gate:* green: FR-5, SR-2, SR-3, SR-4, PL-4, PL-5, FM-2, FM-6, FM-9 (for code kinds and for the structural check), FM-36, FM-37, FM-38, FM-40, FM-43, FM-45. 1 test for each no-loss invariant: I8, a squash merge with a conflict resolution is seen as landed; I9, a gate result on a stale tip moves no ref; I10, a crash before and after the rescue of a forced update loses no tip; I11, an orphan fleet branch is adopted and not deleted; I12, a `git stash` entry blocks the deletion of its branch.
7. **Phase 5: one ledger writer.** Worker commands (`progress`, `result`, `ask`) write files into the attempt's outbox; fleetd ingests them. Moving worker reports to files removes the Unix-socket need of the worker channel on Windows (`preservation-06-wiring-and-platform.md`, row S11). The fleetd command channel still needs a Windows transport. Every other process opens the ledger read-only, enforced by a new `arch_scan` rule. Remove Codex's `broad_ledger_rule` sandbox escape. Each `horch` command that changes state sends a request to fleetd over its command channel, or writes a request file when fleetd is down. Add the `requests` table, `horch request`, the peer-user check of the channel, the modes of the request folder, and the checks of SR-6 for each worker file and request file. *Gate:* green: FR-7, RR-5, RR-8, SR-5, SR-6, FM-15, FM-17, FM-23, FM-24. Tests: the arch test passes.
8. **Phase 6: the idea dispatcher and sub-orchestrators.** Schema v9 gives `ideas` the state machine above and adds `horch idea add` (with the `--direct` fast lane), `ideas`, `show`, `note`, `approve`, `revise` and `park`. Add the dispatcher loop to fleetd, invariant I7, and the `idea-lead` persona, which runs today's brief, spec-writer and ticket-writer steps for one idea in its own interactive pane, a fresh process per phase. `plan.yaml` replaces `MAP.md`; tickets stay Markdown in format C1 and `plan.yaml` names them. Extend `add-map --dry-run` and `scripts/check-req-coverage.sh` into the plan validator (schema, DAG, coverage, profiles, citations to approved assumptions). Retire the long-lived orchestrator conversation, and allow one feature branch per feature with several at once in place of one non-solo run per workspace. Add the idea level, several features at the same time, the `trial_merges` table, stacks, the rules of the feature level (1 sub-orchestrator for each feature, I7, I13, a merge with ideas in flight), invariant I3, the judge pair, the question flow with the sub-orchestrator as the owner of a question, and `horch idea add --to`. The `features` and `branches` tables come from Phase 4. *Gate:* green: FR-1, FR-2, FR-3, FR-4, FR-10, FR-12, RR-3, RR-12, OR-1, OR-3, FM-1, FM-8, FM-10, FM-16, FM-21, FM-29 to FM-33, FM-35, FM-39, FM-41, FM-42.
9. **Phase 7: docs sync and conformance, then exec mode.** Widen `herdr-docs-sync` to every CLI with the conformance suite and unhealthy marking. Then make exec mode the default worker mode and set `context-windows.md` to 256000 for every seat (today it is 300000 for the orchestrator and 200000 for opus). *Gate:* green: FR-6, FR-8, FR-9, SR-1, SR-7, FM-18 to FM-20.

**What each gate needs**

Each mechanism comes from the same phase or an earlier one. "Today" is code that horch has now.

| Phase | Gate | Each mechanism the gate needs, with the phase that gives it |
| --- | --- | --- |
| Phase 0 | A red test for FM-1 to FM-8, FM-25 and each new scenario | The fake harness and the fault points of `crates/horch-e2e` (today); the list of expected failures (0); the Windows test job of CI (0). |
| Phase P | PL-2, PL-6 | The platform interface and the 3 OS modules (P); the `arch_scan` rule of PL-2 (P); the list of expected failures (0). |
| Phase 1 | RR-4, RR-7, OR-2, OR-4, OR-5, OR-6, PL-1, PL-3, FM-4, FM-7, FM-11, FM-12, FM-22, FM-34, FM-44; the part of FM-13 that Phase 1 can do; the old watchdog test | Process facts, the stop of a process group and the machine probe (P); fleetd under the service manager, hang detection and `horch fleetd check` (1); the 3-part pass with intended rows and attempts in `launching` (1); `transition()` and its events (1); the `jobs` table (1); the `pass_cursors` work budget (1); the fleet clock (1); the fleetd file lock and the generation number of the `fleetd` row (1); the launcher (1); the `needs_you` table (1); I4, I5 and the process part of I1 (1); the health guards and pacing (1); the stop of the old watchdog (1); the quota probes and the self-update restart (today). |
| Phase 2 | RR-1, RR-2, RR-6, RR-11, FM-3, FM-5, FM-14 | The fleet clock, the jobs and the launcher (1); the `leases` table and the lease part of I1 (2); the stall ladder and rung 6 with `not_before` (2); `waiting_quota`, the required `fallback` and the restart on a fallback (2); `max_wait_s` (2); the `questions` table (2); `horch task requeue` and `horch task cancel` (2); the `needs_you` table (1). |
| Phase 3 | FR-11, RR-9, RR-10, RR-13 to RR-18, FM-13, FM-25 to FM-28 | The jobs and the launcher (1); the `needs_you` table (1); `horch task requeue` (2); the state, owner, base and rescue columns of `worktrees` (3); the worktree reconciler in report-only mode, then acting (3); the rescue job and its verification (3); I6 (3); `horch worktrees` and `horch worktree restore` (3); the refusals of today's drop, cleanup and triage (today, kept by RR-13 to RR-18). |
| Phase 4 | FR-5, SR-2, SR-3, SR-4, PL-4, PL-5, FM-2, FM-6, FM-9 (code kinds and the structural check), FM-36, FM-37, FM-38, FM-40, FM-43, FM-45; the tests of I8 to I12 | The jobs, the fleet clock and the launcher (1); the leases and the `questions` table (2); the worktree leases and rescue (3); `horch spawn` with `task.yaml` and `START.md` (4); the Stop hooks and `horch result` (4); the `ingested` table and the checks of SR-6 for `result.json` (4); the verifier as a check job (4); the `features` table with 1 feature row for each run (4); the `branches`, `bundles`, `bundle_members`, `gate_runs` and `remote_tips` tables (4); the landing state machine and the feature state machine of 1 feature (4); the fast gate and the full gate (4); the `git fetch` and `git push` jobs (4); I2 and I8 to I12 (4); the 2 classes of rescue refs and `horch rescue drop` (4); `horch feature abandon` (4); today's review scripts (today). |
| Phase 5 | FR-7, RR-5, RR-8, SR-5, SR-6, FM-15, FM-17, FM-23, FM-24; the arch test | The fleetd lock and the generation number (1); the transport of the command channel (1); the `ingested` table (4); the outbox files and the `fleet` helper (5); the command channel for each command, and the request files (5); the `requests` table and `horch request` (5); the peer-user check and the modes of the request folder (5); the checks of SR-6 for each worker file and request file (5); the read-only ledger outside fleetd and its `arch_scan` rule (5). |
| Phase 6 | FR-1, FR-2, FR-3, FR-4, FR-10, FR-12, RR-3, RR-12, OR-1, OR-3, FM-1, FM-8, FM-10, FM-16, FM-21, FM-29 to FM-33, FM-35, FM-39, FM-41, FM-42 | The `needs_you` table (1); the `questions` table (2); the `features` and `branches` tables and the landing (4); the single writer and the command channel (5); the `ideas`, `requirements` and `task_covers` tables and the idea state machine (6); the dispatcher, the `idea-lead` persona and 1 sub-orchestrator for each feature (6); the plan validator and the model guide (6); the judge pair (6); the question flow with the sub-orchestrator as owner (6); the `trial_merges` table, several features and stacks (6); a merge with ideas in flight (6); I3, I7 and I13 (6); `horch idea add` with `--direct` and `--to`, `ideas`, `show`, `note`, `approve`, `revise`, `amend` and `park` (6). |
| Phase 7 | FR-6, FR-8, FR-9, SR-1, SR-7, FM-18 to FM-20 | The launcher (1); docs sync for each CLI and the conformance suite (7); the unhealthy mark of a profile and its recovery (7); exec mode as the default (7); 256000 as the context of each seat (7); the adapters (today). |

**Where each rule becomes green**

Each requirement and each failure mode has exactly 1 phase. The no-loss invariants I8 to I12 have 1 test each, in the gate of Phase 4, the first phase that uses them.

| Phase | Requirements | Failure modes |
| --- | --- | --- |
| Phase 0 | None. Phase 0 writes the red tests. | None. |
| Phase P | PL-2, PL-6 | None. |
| Phase 1 | RR-4, RR-7, OR-2, OR-4, OR-5, OR-6, PL-1, PL-3 | FM-4, FM-7, FM-11, FM-12, FM-22, FM-34, FM-44 |
| Phase 2 | RR-1, RR-2, RR-6, RR-11 | FM-3, FM-5, FM-14 |
| Phase 3 | FR-11, RR-9, RR-10, RR-13, RR-14, RR-15, RR-16, RR-17, RR-18 | FM-13, FM-25, FM-26, FM-27, FM-28 |
| Phase 4 | FR-5, SR-2, SR-3, SR-4, PL-4, PL-5 | FM-2, FM-6, FM-9, FM-36, FM-37, FM-38, FM-40, FM-43, FM-45 |
| Phase 5 | FR-7, RR-5, RR-8, SR-5, SR-6 | FM-15, FM-17, FM-23, FM-24 |
| Phase 6 | FR-1, FR-2, FR-3, FR-4, FR-10, FR-12, RR-3, RR-12, OR-1, OR-3 | FM-1, FM-8, FM-10, FM-16, FM-21, FM-29, FM-30, FM-31, FM-32, FM-33, FM-35, FM-39, FM-41, FM-42 |
| Phase 7 | FR-6, FR-8, FR-9, SR-1, SR-7 | FM-18, FM-19, FM-20 |

**What a release does to a run in flight**

Each release is on for every user at once, so each phase states what happens to a run that is in flight. Its test starts on the phase before and ends on the new phase.

| Phase | Old rows (backfill) | Old processes | Old commands and files | Test in the gate |
| --- | --- | --- | --- | --- |
| Phase 0 | None. Phase 0 adds tests only. | No change. | No change. | None. |
| Phase P | None. | No change of behavior (PL-6). | No change. | The test of PL-6: each test has the same result before the move and after it. |
| Phase 1 | The `fleetd` row gets generation 1 and a fleet clock of 0. Each live worker row gets an attempt row in `running`, with its recorded PID and start time. | fleetd stops each recorded watchdog by its PID and start time and takes its runs. A live worker goes on. | `horch ledger watch` only points at fleetd. The worker commands of today still write the ledger, through `transition()`. | A run with a live worker and a live old watchdog starts on Phase P. After the update to Phase 1, 1 control path acts, and the task has 1 attempt. |
| Phase 2 | Each held task gets a lease with a full time. An open `worker_waits` row of an interactive persona gets `max_wait_s` from the release. | A live worker goes on, and its lease renews on earned activity. | The worker commands of today still work. | A held task with a live worker starts on Phase 1. After the update, the worker is not reaped, and the task has 1 lease. |
| Phase 3 | Each `worktrees` row whose task has a live attempt becomes `active`, with that attempt as owner. Each other row becomes `orphaned`, so it is rescued before anything else. | A live worker goes on in its tree. A tree that the old claim path makes is adopted as `orphaned` and linked to its task. | `worktree drop`, cleanup and triage keep their refusals (RR-13 to RR-17). | A live tree and a dirty orphan tree start on Phase 2. After the update, the live tree is `active`, the orphan is listed in report-only mode, and nothing is removed. |
| Phase 4 | Each run gets 1 feature row, and its run branch is the feature branch. Each task branch gets a `branches` row: `landed` when its task is landed, else `building`. | An attempt that began with a claim ends by the old path, and only a new attempt uses `task.yaml` and `result.json`. The attempt row records which. A detached finish that runs ends by the old path, and fleetd starts no landing of that task until it ends. | `horch ledger claim` and the old finish work for an old attempt only. | A claimed attempt with a detached finish starts on Phase 3. After the update, it ends by the old path, and its task lands 1 time. |
| Phase 5 | None. The `requests` table starts empty. | A live worker goes on. | An old worker command writes to the outbox when its attempt has one, and sends a request to fleetd when not. | An old worker on Phase 4 runs `progress` and `done` after the update. Each one has 1 effect, and only fleetd writes the ledger. |
| Phase 6 | Each run that exists gets 1 idea row, in `in_progress` when a task is not done, else in `integrating`. Its tickets stand for your approval, as for `--direct`, so I7 holds. The feature row of Phase 4 stays. | fleetd starts 1 sub-orchestrator for each feature (I13), and it takes the open questions of the run. The orchestrator of the run gets no more questions. | `MAP.md` of a run in flight stays readable. A new idea uses `plan.yaml`. | A run with an open question starts on Phase 5. After the update, it has 1 idea row and 1 feature row, and its question has 1 owner. |
| Phase 7 | None. | A worker that runs interactive goes on. Exec is the default for new attempts. | `context-windows.md` gives 256000 to new attempts. | An interactive worker starts on Phase 6. After the update, it ends as before, and the next attempt of its task runs in exec mode. |

**New work for each phase**

| Phase | New work |
| --- | --- |
| Phase P | The platform layer: the interface, the 3 OS modules, the move of the existing OS-specific code, the `arch_scan` rule of PL-2. |
| Phase 1 | The fleetd concerns of the platform layer, the health guards, pacing. The 3-part pass, `transition()`, the `jobs` table, the `pass_cursors` work budget, the fleet clock, the fleetd file lock and the generation number, the launcher, the `needs_you` table, `horch fleetd check`, the stop of the old watchdog. |
| Phase 2 | `waiting_quota`. The lease part of I1, rung 6 with `not_before`, the `questions` table, `horch task requeue`, `horch task cancel`. |
| Phase 3 | RR-13 to RR-18: the refusals of today's drop, cleanup and triage, and the record of unlanded work, before the reconciler acts. |
| Phase 4 | The 2 declared gates, the landing state machine. The `features` table with 1 feature row for each run, the `branches`, `bundles`, `bundle_members`, `gate_runs` and `remote_tips` tables, the `git fetch` and `git push` jobs, the `ingested` table, the 2 classes of rescue refs, `horch rescue drop`, `horch feature abandon`. |
| Phase 5 | The `requests` table, `horch request`, the peer-user check, the checks of SR-6 for each worker file and request file. |
| Phase 6 | Several features at the same time, stacks, the `trial_merges` table, the rules of the feature level, the fast lane, judge tasks, the model guide in plan validation, the question flow. |
| Each phase that changes the schema | The backup before the migration and the newer-schema refusal. |

Phases 1 to 4 remove most of the stalls you see day to day and stop worktrees from being lost. Phase 4 does not wait for Phase 6: it lands the branches of 1 feature for each run. Phase 6 is the largest change and depends on Phase 5, because a plan has to land through the single writer to be all-or-nothing, and only fleetd may record your approvals.

**The numbers of the design**

Each number has 1 of 3 classes. A fixed contract changes only with a change of this design. A default has a key, and you can change it. A starting value has a key, and you tune it from real runs.

| Number | Value | Class | Where it is set |
| --- | --- | --- | --- |
| Reconcile pass | 5 s | Default | `fleet.toml`, `pass_period_s` |
| Hang limit of fleetd, and the period of `horch fleetd check` | 30 s of awake time | Fixed contract (PL-5) | The service definitions of the platform layer, and `horch fleetd check` |
| Work budget of a pass | A count of rows of each kind | Starting value | `fleet.toml`, `pass_budget` |
| Wrapper heartbeat | Every 10 s | Starting value | `fleet.toml`, `heartbeat_every_s` |
| Heartbeat age for a lease renewal | 30 s | Starting value | `fleet.toml`, `heartbeat_max_age_s` |
| Lease | 120 s | Starting value | Profile, `liveness.lease_s` |
| Stall | 300 s | Starting value | Profile, `liveness.stall_after_s` |
| Start timeout | 90 s | Starting value | Profile, `liveness.start_timeout_s` |
| Retry backoff | 30 s, 120 s, 480 s | Default | Profile, `retry.backoff_s` |
| Attempts | 3 | Default | Profile, `retry.max_attempts` |
| Stop grace before the kill of a process group | 20 s | Default | `fleet.toml`, `stop_grace_s`. The platform module gets it as an input. |
| Declared wait of an interactive persona | 1 h | Default | Profile, `liveness.max_wait_s` |
| Time limit of a question | 15 min | Starting value | `fleet.toml`, `question_limit_s` |
| Replan limit | 30 min | Starting value | `fleet.toml`, `replan_limit` |
| Stop of an idle sub-orchestrator | 30 min | Default | `fleet.toml`, `idle_stop_s` |
| Park of an idea that waits on you | 7 days | Default | `fleet.toml`, `park_after_days` |
| Retention of a needs-you item that is not open, of a trash ref and of a `superseded` rescue ref | 30 days | Default | `fleet.toml`, `retention_days` |
| Time limits of the landing states | 30 min, 60 min, and the job limits plus 10 min | Starting value | `fleet.toml` |
| Fetch period | 60 s | Default | `fleet.toml`, `fetch_period_s` |
| Push failures before a needs-you item | 3 | Default | `fleet.toml`, `push_failure_limit` |
| Budget of `unique` rescue refs | No value yet | Starting value | `fleet.toml`, `rescue_unique_budget` |
| Worktree disk budget | 20 GB | Default | `fleet.toml`, `worktree_disk_budget` |
| Size of a resolver task | 100 changed lines or 2 files | Starting value | `fleet.toml`, `resolver_max_lines` and `resolver_max_files` |
| Size limits of worker files | See Protocol rules | Starting value | `fleet.toml` |
| Context of each agent | 256000 tokens | Default | Profile, `compact_window`, and `context-windows.md` from Phase 7 |
| Fix rounds of a judge pair | 2 | Fixed contract (D4) | The code of the judge pair |
| Code-sweep requeues and review rounds | 2 requeues; 1 round light, 3 rounds full | Fixed contract (05 V-14) | Today's review scripts |
| Land lock | Stale after 1 h, timeout 30 s, heartbeat 60 s | Fixed contract (05 V-11) | `vcs/landing.rs` |
| Git network calls | Push 10 min, others 3 min | Fixed contract (04 F11) | `vcs/network.rs`. Each is the time limit of its job. |
| Harness update cooldown | More than 7 days | Default | `update_cooldown_days` |

## Open questions

Your 6 choices are answered below. Eight facts need a live test; none blocks Phases 0 to 5.

**Your call (answered)**

| Question | Answer | Decision |
| --- | --- | --- |
| Exec mode as the default for workers? | Yes. A worker runs headless, does its task and exits. A persona can be marked interactive; sub-orchestrators are interactive, and an interactive persona is the way to pair with a worker in a pane. | D7 |
| One run, one branch and one PR per idea, with several ideas running at once? | No: 1 feature branch and 1 pull request per feature, which holds 1 main idea and its follow-on ideas. Several features run at once, in parallel by default. See Features, landing and integration. | D5, D10, D11, D12 |
| How many sub-orchestrator panes at once? | No cap. 1 sub-orchestrator per feature; an idle one stops after 30 min to free memory. Only health guards pause new dispatch. | D8 |
| Which host runs the fleet day to day, macOS (launchd) or Linux (systemd)? | All 3: macOS, Linux and Windows, behind 1 platform layer. | D1 |
| When does the existing OS-specific code move into the platform layer? | In 1 dedicated phase, Phase P, after Phase 0 and before Phase 1. All 178 production sites move, with no change of behavior. The `cfg` sites in tests stay. | D16 |
| Is 1 hour the right cap on a worker's declared wait? | An exec worker that asks a question exits, and its task waits in `blocked_on_user` with no process; the question has its time limit. The 1 h cap applies only to an interactive persona. | Settled by the orchestrator with D8; question flow D13 |

**To confirm by test**

- [ ] How to pre-trust Codex hooks for unattended workers (managed or requirements config). The docs say user hooks need trust through `/hooks`.
- [ ] Whether a private `HOME` fully isolates `agy` config, skills and MCP, and keeps its login.
- [ ] Whether herdr's `pane report-agent` state from the wrapper overrides its screen-based detection in the sidebar, or sits beside it.
- [ ] Which fields herdr's custom key commands take, so one key can run `horch idea approve` for the focused idea pane.
- [ ] herdr preview 0.9.4 against 0.9.5: whether the facts in the herdr table hold for the installed preview.
- [ ] A Windows transport for the fleetd command channel.
- [ ] The size limit of a resolver task (starting value: 100 changed lines or 2 files).
- [ ] The accuracy of a structural merge tool on this repository's recorded conflicts.
