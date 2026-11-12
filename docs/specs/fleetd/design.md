# Herdr Agent Fleet: System Design

Nov 10, 2026 · @Matthew

Amended 2026-11-12 after a design review. See Amendments.

## Summary

Make correctness a property of code and files, not of model memory: a supervised daemon owns the ledger and acts on facts it can check, workers are bound to tasks before they start and hold them only by earned leases, and a task is done only when its checks pass on disk.

Your multi-herdr (`horch`) already has the right parts: recorded PIDs, a supervisor per worker, worktrees per task and finish steps. The failures you see come from five jobs it still leaves to an LLM: creating tasks, claiming them, reporting results, reacting to typed mail and cleaning up worktrees. Each change below takes one of those jobs away from the model, and the AI that is left works on one idea at a time.

**The decisions that matter most**

1. **One supervised, level-triggered control loop.** Replace the lazily started watchdog with `horch fleetd`, which runs under the service manager of the OS (launchd, systemd or a Windows service) through 1 platform layer. Every 5 s it compares the ledger with real processes, herdr panes and files, and fixes the difference itself. It is the only ledger writer. A missed event is caught on the next pass, and a hung daemon is restarted.
2. **Push dispatch with leases, not claims.** fleetd binds a task to a worker and writes the task file before the agent starts, so there is nothing to find. The task is held by a lease that renews only on real activity. An exec worker that waits on a question exits and its task parks, and an interactive persona's waits are capped at an hour, so an idle worker loses its task in minutes, not days. A usage limit is not a stall: the task moves to its fallback persona or waits in `waiting_quota`.
3. **Done is a fact on disk.** A stop hook will not let an agent end its turn without writing `result.json`, exec mode turns "stopped" into "process exited", and only the verifier moves a task to done: its acceptance checks, then today's review scripts for code, or a paired judge task for a judgment task.
4. **A program dispatches ideas; an AI leads each one.** `horch idea add` drops a rough idea into a queue. A deterministic dispatcher inside fleetd moves each idea through a fixed state machine and keeps one sub-orchestrator pane per feature. That AI plans 1 idea at a time: it researches the idea, interviews you in its own pane, writes its assumptions down and waits for your quick approval before it writes the spec and tickets. An idea belongs to exactly 1 feature, and the ledger records it. No task exists outside an approved, validated plan; a ticket you wrote enters through the `--direct` fast lane.
5. **Every worktree is leased before it exists.** fleetd records each worktree in the ledger, tied to its task and owner, before it runs `git worktree add`; workers never create one. A reconciler compares the ledger with `git worktree list`, and anything orphaned is committed to a rescue branch before cleanup, so work you paid for is never lost.
6. **Every task lands on a feature branch through a landing state machine in the ledger, and no branch is lost.** Every branch is a ledger row before it exists, with a landing state, 1 owner and a time limit. fleetd lands ready task branches in bundles behind the fast gate, detects conflicts between features early, and checks the no-loss invariants against git on every pass. Each feature reaches the primary branch through 1 pull request.
7. **There is no spend budget and no cap on parallel work. Opus 5.5 at medium effort is the default worker.** The purpose is to use the subscription usage fully. fleetd paces dispatch to each pool's usage window, and only health guards for disk and load pause new dispatch.

The migration takes eight phases, each gated by a fault-injection test. Phases 1 to 4 stop most daily stalls and stop losing worktrees.

## Amendments

A design review on 2026-11-11 and 2026-11-12 made 13 decisions. This document includes all of them.

| # | Decision | Sections changed |
| --- | --- | --- |
| D1 | fleetd supports macOS, Linux and Windows as first-class hosts. All OS-specific code sits behind 1 platform layer with 1 module per OS, so core code has no `cfg(target_os)`. Migration of the existing OS-specific code is a separate decision. | Summary; Goals, non-goals and defaults; Requirements (PL-1 to PL-4); Architecture and roles (fleetd, wrapper, Platform layer, What each OS provides); Supervision, liveness and reconciliation; Failure modes (FM-11, FM-12); Migration plan (Phase 1); Open questions |
| D2 | A usage limit or a provider outage is not a stall. Every worker persona names a fallback. The task restarts at once on a fallback of another provider, else waits in `waiting_quota` with its lease paused. No attempt is burned. | Summary; Requirements (RR-11); Ledger; Worker profiles and adapters (`fallback`); Supervision, liveness and reconciliation; Failure modes (FM-5); Migration plan (Phase 2) |
| D3 | Everything is an idea. `horch idea add --direct <ticket file>` is a fast lane for a ticket you wrote: no research, no interview, a plan with 1 task that still passes plan validation and still needs a check. | Summary; Requirements (FR-10, FR-12); Ideas; Migration plan (Phase 6) |
| D4 | A judgment task gets a structural check and a paired judge in a different session, on a different provider when the roster has one. The judge ranks each miss CRITICAL, HIGH, MEDIUM or LOW, and only CRITICAL blocks. | Summary; Requirements (FR-4, RR-3); Architecture and roles (Verifier, Done for each kind of task); Workspace layout (plan example); Failure modes (FM-9, FM-33) |
| D5 | Features run in parallel by default. Conflicts are found early by trial merges, a conflict stacks the younger feature on the older one with 1 resolver task, feature branches are rebased continuously, and each project declares a fast gate and a full gate. | Features, landing and integration (Between features, evidence); Ledger (`gate_runs`, `trial_merges`); Open questions |
| D6 | Code review keeps today's review scripts unchanged, with their limits. | Architecture and roles (Verifier, Done for each kind of task); Migration plan (Phase 4) |
| D7 | Exec mode is the default for workers. A persona can be marked interactive; sub-orchestrators are interactive. | Goals, non-goals and defaults; Worker profiles and adapters (`mode`); Open questions |
| D8 | No spend budget and no cap on parallel work. Health guards and pacing stay. Opus 5.5 at medium effort is the default, and plan validation checks the model guide. | Summary; Goals (G7) and defaults; Requirements (OR-4, OR-5); Architecture and roles; Ideas; Worker profiles and adapters; Workspace layout; Failure modes (FM-20, FM-21, FM-29); Open questions |
| D9 | Rollout is automatic and immediate. fleetd is a subcommand of the single `horch` binary, `horch self-update` restarts the running daemon, and a phase is on for every user once its fault-injection tests pass. | Requirements (OR-6); Migration plan; Failure modes (FM-34) |
| D10 | Every branch is a ledger row before it exists, with a landing state, 1 owner and a time limit. Ready task branches land in bundles behind the fast gate, and no-loss invariants are checked against git on every pass. | Features, landing and integration (Landing, No work is lost); Ledger (`branches`, I8 to I13); Worktrees; Supervision (reconcile pass) |
| D11 | Every idea lands in a feature branch. A feature is 1 sub-orchestrator, 1 feature branch and 1 pull request to the primary branch. Task branches land on the feature branch with no human approval. | Summary; Features, landing and integration (Features); Requirements (FR-2); Goals, non-goals and defaults; Architecture and roles; Ledger (`features`); Ideas |
| D12 | A feature is an intake funnel until its pull request merges. `horch idea add --to <feature>` joins an open feature; with no `--to`, an idea starts a new feature. | Features, landing and integration (Features); Ideas; Ledger (`ideas.feature_id`) |
| D13 | A worker question that nothing approved covers and that changes no assumption is decided by the sub-orchestrator as its own reversible assumption. Only product direction, scope or a trade-off you must own goes to you. | Ideas (question flow); Requirements (RR-12); Ledger (`questions`); Workspace layout; Failure modes (FM-35) |

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
| Host OS | macOS, Linux and Windows, all first-class, behind 1 platform layer | The release pipeline already builds 5 targets for the 3 OSes. Core fleetd code has no OS-specific branches, so a change for one OS does not break another. |
| Process manager for the supervisor | The service manager of the OS, through the platform layer: launchd on macOS, a systemd user unit on Linux, a Windows service on Windows; restart always and hang detection on every OS: the systemd watchdog on Linux, and `horch fleetd check` every 30 s on macOS and Windows | The thing that watches everything is itself watched. |
| Idea dispatcher | Deterministic code inside fleetd; no AI | Queue order and state changes must not depend on a model remembering. |
| Spend and parallel work | No spend budget. No cap on features, sub-orchestrators or workers. Health guards pause new dispatch only while free disk or machine load is past a safety floor | The purpose is to use the subscription usage fully. A health guard protects the machine; it is not a cap. |
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

Each requirement has an ID that the failure analysis and the tests point back to; "shall" lines are testable.

**Functional (FR)**

| ID | Requirement | Verified by |
| --- | --- | --- |
| FR-1 | `horch idea add` shall record each idea as one immutable file and one `captured` ledger row, with no AI involved. | Intake test: two ideas added back to back produce two files and two rows. |
| FR-2 | The dispatcher shall be deterministic code that keeps 1 sub-orchestrator for each feature; 1 process never plans 2 ideas at the same time. | Property test over the idea state machine; the ledger shows one idea per sub-orchestrator attempt. |
| FR-3 | A plan shall be committed to the ledger all-or-nothing, and only after it passes schema, DAG and coverage checks. | Plan validator unit tests; a plan missing a requirement is rejected. |
| FR-4 | Every task shall name a worker profile, its `kind`, its inputs, its outputs and a machine-checkable acceptance check. For a judgment kind the check is structural (the file exists, the schema or the required sections are valid), and the plan shall pair the task with a judge task in a different session from the author, on a different provider when the roster has one. | Plan schema; plan validation refuses a judgment task with no paired judge task. |
| FR-5 | The supervisor shall assign tasks to workers (push). Workers shall never search for work. | No code path where an agent queries the ledger for open tasks. |
| FR-6 | A worker profile shall declare CLI, model, skills, MCP servers, sandbox, writable paths, context budget, timeouts and restart policy. | Profile schema; the adapter refuses an incomplete profile. |
| FR-7 | All task and result hand-off shall be through files under `FLEET_ROOT`. The only bytes ever sent into a terminal are a fixed doorbell line written by the supervisor's code. | Grep of the code for mux send calls; doorbell text is a constant. |
| FR-8 | A docs-sync job shall fetch each CLI's documentation, record the installed version, and run adapter conformance tests on change. | Sync log and test report per CLI version. |
| FR-9 | Each agent shall run with a context limit of 256k tokens by default, set through the CLI's own setting where one exists. | Conformance test reads back the effective setting. |
| FR-10 | No spec or task shall be written for an idea until you approve its assumptions, or you add the idea yourself with `--direct`, and only a command you run outside any fleet process can record that approval. | A sub-orchestrator that writes `approval.json` or runs `horch idea approve` leaves the idea in `awaiting_approval`. |
| FR-11 | Every worktree shall be leased in the ledger, tied to one task and one owner, before it is created, and only fleetd shall create or remove worktrees. | A worker's own `git worktree add` is flagged on the next pass; `arch_scan` finds no worktree calls outside fleetd. |
| FR-12 | Every task shall belong to an idea. `horch idea add --direct <ticket file>` shall take a ticket you wrote, skip research and the interview, and make a plan with 1 task that still passes plan validation and still needs a check. | Intake test: a `--direct` idea produces 1 validated plan with 1 task; no code path creates a task without an idea. |

**Reliability (RR)**

| ID | Requirement | Verified by |
| --- | --- | --- |
| RR-1 | Liveness shall be judged from outside the model: process state, wrapper heartbeat, CLI hook events and output activity. | Fault injection: kill -STOP a worker; alarm fires within one lease. |
| RR-2 | A task shall be held by a lease with an expiry. An expired lease returns the task to ready or escalates; it never stays assigned. | Property test over the state machine. |
| RR-3 | A task shall reach done only when its result file exists, validates, and its acceptance check passes when run by the supervisor. For a code kind, today's review scripts shall also pass. For a judgment kind, its judge task shall report no open CRITICAL item. | Worker that says "done" without artifacts stays not-done; a document with an open CRITICAL item stays not-done. |
| RR-4 | The supervisor shall reconcile desired and actual state on a fixed period as well as on file events, so a missed event is caught by the next pass. | Drop all file watch events (inotify on Linux) in a test; tasks still complete. |
| RR-5 | Exactly one process shall write the ledger. Every other component writes files that it ingests. | SQLite opened read-only everywhere else. |
| RR-6 | Every retry shall be bounded. After the profile's max attempts, the task is escalated to the user, not retried forever. | State machine test. |
| RR-7 | The supervisor shall survive its own restart with no lost or duplicated task, by rebuilding state from the ledger and the files. | Kill the supervisor mid-run; compare outcomes with a clean run. |
| RR-8 | All file writes in the protocol shall be atomic (write to a temp name, fsync, rename) and ingestion shall be idempotent. | Crash test during writes. |
| RR-9 | No worktree shall be removed until its commits and its uncommitted and untracked files are saved on a rescue ref, and that ref is read back and shown to hold them. | Kill a worker with dirty and untracked files; after cleanup the rescue ref contains every one. |
| RR-10 | The reconciler shall compare the worktree ledger with `git worktree list` and the work directory on every pass, and adopt or rescue anything on disk that the ledger does not own. | Create a worktree by hand; it is adopted or rescued within one pass. |
| RR-11 | A worker that shows a usage limit or a provider outage shall not be treated as stuck, and shall not burn an attempt. If its persona's fallback is on another provider, fleetd shall restart the task at once on a new worker of the fallback persona, on the same worktree. Otherwise the task shall wait in `waiting_quota` with its lease paused until the pool resets. If both pools are limited, the task shall wait. | Phase 0 test: a usage-limited worker is not reaped, and its attempt count does not change. |
| RR-12 | A worker question shall be a ledger row with an owner, a state and a time limit. fleetd shall start the owner on each pass until an answer is recorded, resume the worker with the answer or start a fresh worker from the question, the answer and the committed work, and escalate the question to you past its time limit. | Phase 0 test: a question whose owner is not running is answered after fleetd starts it; an unanswered question reaches `needs-you/` at its limit. |

**Safety and isolation (SR)**

| ID | Requirement | Verified by |
| --- | --- | --- |
| SR-1 | Each worker shall write only to its task directory and its worktree, plus paths its profile names. | Sandbox conformance test per CLI. |
| SR-2 | Secrets shall reach agents only through env vars named in the profile, never through task files. | Lint on task files. |
| SR-3 | No agent shall push to a protected branch, deploy, or message anyone outside the fleet. | Git hooks and profile defaults. |
| SR-4 | An acceptance command shall run in the sandbox profile of the task's worker, never with the rights of fleetd. | A check that writes outside the worker's allowed paths is refused. |

**Platform (PL)**

| ID | Requirement | Verified by |
| --- | --- | --- |
| PL-1 | fleetd shall run on macOS, Linux and Windows as first-class hosts. | CI runs the test suite on all 3 OSes. |
| PL-2 | Core fleetd code shall have no `cfg(target_os)`, `cfg(unix)` or `cfg(windows)`. It shall call 1 platform interface, and each OS shall have 1 module that implements it. The interface covers service install and restart, hang detection, process facts (alive, start time, children, process group), stop and kill of a process group, file watching, sandboxing, sleep and wake, the transport of the fleetd command channel, and the machine probe (free disk, load). | An `arch_scan` rule finds no OS `cfg` in core fleetd code. |
| PL-3 | Where an OS has no equivalent, its module shall return an explicit "not supported" value, and core code shall decide what to do with it. Nothing shall fail silently. | Unit test per module for each concern it does not support. |
| PL-4 | Lease time shall count only while the machine is awake. A sleep shall not expire a lease. | Phase 0 test: a machine sleep in the middle of an attempt does not expire its lease. |
| PL-5 | A hung fleetd shall be restarted on all 3 OSes. On each pass, fleetd shall write a heartbeat: the time of its last full pass. On Linux the systemd watchdog restarts it. On macOS a launchd timer job, and on Windows a scheduled task, shall run `horch fleetd check` every 30 s. `horch fleetd check` shall restart the service when the heartbeat is older than 30 s of awake time. Each `horch` CLI command shall make the same check before it sends its request. | Phase 0 test on each OS: a hung fleetd is restarted. |

**Observability (OR)**

| ID | Requirement | Verified by |
| --- | --- | --- |
| OR-1 | `fleet status` shall show every idea, task, lease, worker and its last signal in one view. | Manual check. |
| OR-2 | Every state change shall be an appended event with a cause, so any outcome can be explained after the fact. | Event table audit. |
| OR-3 | Anything that needs you shall land in one place (`/fleet/needs-you/`), in `horch ideas`, and as a herdr notification. | Escalation test. |
| OR-4 | fleetd shall read the usage window of each pool and dispatch so that the usage is consumed by the reset. | Pacing test with a fake pool window. |
| OR-5 | fleetd shall pause new dispatch only while free disk or machine load is past a safety floor, and shall resume by itself. A machine probe that returns unknown (Windows today) shall be reported, not read as healthy or unhealthy. | Fault injection: a low-disk probe pauses dispatch; a normal probe resumes it. |
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
| Persistence | Detach keeps processes. A herdr server restart restores layout but not processes; agents come back only through an integration's resume command | After a herdr restart every live attempt must be re-checked. The reconciler handles this the same way as any dead process. |
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
- **No API key for Claude.** Your repo's rule stands: every `claude` launch strips `ANTHROPIC_API_KEY` and uses your subscription login. A private `CLAUDE_CONFIG_DIR` therefore needs its login seeded, which the docs-sync conformance test checks.

## Architecture and roles

The design splits the system into a deterministic control plane (fleetd with its idea dispatcher, and the per-attempt wrapper) and disposable LLM processes that only read and write files, so models do judgment and code does bookkeeping.

&#91;embedded content: components and the files between them\]

Sub-orchestrators and workers never touch the ledger, each other or git worktrees directly; fleetd is the only path between them, and it acts on files, your commands and process facts.

| Component | What it is | Owns | Never does |
| --- | --- | --- | --- |
| **fleetd** | One long-running `horch fleetd` process under the service manager of the OS (launchd, systemd or a Windows service), installed and restarted through the platform layer. On Linux: a systemd user unit with `Restart=always` and `WatchdogSec=30`. On macOS a launchd timer job, and on Windows a scheduled task, runs `horch fleetd check` every 30 s. The control plane. | The ledger (sole writer), the idea dispatcher, plan validation and commit, task dispatch, leases, worktree leases and rescue, the reconcile loop, acceptance checks, escalation, the herdr adapter. | Calling an LLM. It is plain code, so its behaviour is testable and repeatable. |
| **Idea dispatcher** | Code inside fleetd: the idea state machine. | Moving each idea to its next state when the required files validate, keeping one sub-orchestrator pane per feature, recording your approvals. | Judging an idea's content. It only checks that files exist, validate and carry your approval. |
| **Sub-orchestrator** | Claude Code, Opus 5.5, interactive in herdr pane `idea-NNNN`, one per feature, a fresh process for each phase of one idea, planning 1 idea at a time. | Researching the idea, interviewing you, writing `assumptions.md`, then `spec.md`, `requirements.yaml` and `plan.yaml`; answering its workers' questions; replanning a failed task. | Seeing another idea, approving its own assumptions, creating tasks or worktrees directly, or typing into another pane. |
| **fleet-run wrapper** | A small program that every agent process is launched through, one per attempt. | Building the attempt's private config home from the profile, starting the CLI in its own process group through the platform layer, writing `heartbeat.json`, mapping CLI events and hooks to agent state, reporting state to herdr, enforcing the wall-clock limit (`max_minutes`, a hang guard) and the 256k context limit, writing `exit.json`. | Deciding outcomes. It reports facts; fleetd decides. |
| **Worker** | Any agent CLI (Claude Code, Codex, Antigravity or Gemini CLI) started by the wrapper with a named profile. | Doing one task attempt in the worktree fleetd leased to it and writing `outbox/result.json`. | Finding work, touching the ledger, creating or removing worktrees, or working on more than one task. |
| **Verifier** | Code inside fleetd that runs each task's check in the worktree with today's check timeout (`preservation-05-contracts-and-values.md`, row V-10). The acceptance commands run in the sandbox profile of the task's worker, never with the rights of fleetd. For a code kind, today's review scripts then run unchanged. For a judgment kind, the check is structural, and a paired judge task reviews the content (see Done for each kind of task). | Writing `verdict.json` and moving the task to `done` or back to `ready`. | Trusting a worker's own claim of success. |
| **Landing** | Code inside fleetd: the landing state machine, bundles, trial merges and the 2 gates (see Features, landing and integration). | Landing task branches on their feature branch, rebasing feature branches, opening resolver and triage tasks, running the full gate, pushing the feature branch for its pull request. | Merging to the primary branch, unless the project setting says so. By default you merge. |
| **Docs sync** | A scheduled job (daily, plus on any CLI version change). | Mirroring each CLI's docs, recording versions, running adapter conformance tests. | Changing a profile by itself. It opens a task when something drifts. |
| **Platform layer** | 1 interface inside horch with 1 module per OS (for example `platform/mac.rs`, `platform/linux.rs`, `platform/windows.rs`). | Service install and restart, hang detection, process facts (alive, start time, children, process group), stop and kill of a process group, file watching, sandboxing, sleep and wake, the transport of the fleetd command channel (a Unix socket, or a named pipe on Windows), and the machine probe (free disk, load). | Failing silently. Where an OS has no equivalent, the module returns an explicit "not supported" value, and core code decides what to do. |
| **herdr** | The pane runtime, in a dedicated session `herdr --session fleet`, also under the service manager of the OS. | Terminals, agent names, screen-based status, the sidebar you watch. | Holding any state fleetd depends on. If herdr restarts, the reconciler rebuilds from the ledger and files. |

**Done for each kind of task.** Each task declares a `kind`. Code kinds are `feature`, `quality`, `refactor` and `security`. Judgment kinds are `research`, `design`, `docs`, `report` and `creative`.

- **Code kind.** The task's check runs. Then today's review scripts run unchanged: the code-sweep rounds, the findings F<n>, the answers file, the waivers and the verification profiles. Their limits stay: at most 2 requeues, and review rounds of 1 (light) and 3 (full) (`preservation-05-contracts-and-values.md`, row V-14).
- **Judgment kind.** The verifier runs a structural check: the file exists, and the schema or the required sections are valid. Plan validation refuses a plan in which a judgment task has no paired judge task. The judge runs in a different session from the author, on a different provider when the roster has one. It ranks each miss:

| Rank | Effect |
| --- | --- |
| CRITICAL | Blocks. A fresh session, not the author and not the judge, fixes it. Then a fresh judge checks only the earlier CRITICAL items and the change. |
| HIGH | Does not block alone. The sub-orchestrator of the feature fixes it or dismisses it with a written reason. |
| MEDIUM, LOW | Never block. |

"Good enough" passes. The second judge cannot raise new MEDIUM or LOW items; this is the guard against gold-plating. After 2 fix rounds, a result that still has a CRITICAL item goes to you.

**Supervision tree and restart policy (OTP style)**

- The service manager of the OS supervises fleetd and the herdr server: restart always. On each pass, fleetd writes a heartbeat: the time of its last full pass. On Linux the systemd watchdog (`WatchdogSec`) restarts a hung fleetd. On macOS a launchd timer job runs `horch fleetd check` every 30 s, and on Windows a scheduled task does the same. `horch fleetd check` restarts the service when the heartbeat is older than 30 s of awake time. Sleep time does not count. Each `horch` CLI command also makes this check before it sends its request. A hung fleetd is killed and restarted on all 3 OSes, and recovers from the ledger (RR-7).
- fleetd supervises attempts: one-for-one, so a crashed worker never affects its siblings. Each profile sets `max_attempts` and backoff. After the last attempt the task goes back to its idea's sub-orchestrator to replan once, then to you.
- The wrapper supervises its CLI process group: on a wall-clock breach it stops the whole group through the platform layer, so no grandchild processes linger. On Linux the module sends SIGINT, waits 20 s, then SIGKILLs the group.

**The command channel.** Each `horch` command that changes state sends a request to fleetd over its channel (a Unix socket, or a named pipe on Windows, through the platform layer). When fleetd is down, the command writes a request file, and fleetd reads it at start. fleetd stays the only ledger writer.

**One platform layer for three OSes.** Core fleetd code has no `cfg(target_os)`, `cfg(unix)` or `cfg(windows)`. It calls the platform interface. The Linux details in this document (`systemd`, `Restart=always`, `WatchdogSec=30`, `/proc`, `inotify`, bubblewrap, `SIGINT` then `SIGKILL`) are the Linux module's implementation. The layer is for new fleetd code. Migration of the 178 existing OS-specific sites in production code is a separate decision (`docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6).

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

Every hand-off is a file under `/fleet`, written atomically, never edited after it lands, and named so that its owner and order are obvious from the path.

**Directory layout**

```
/fleet/
  fleet.toml                  global config (timers, health floors, paths)
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
  run/                        pid files, lock, mux handles
```

**Knowledge across ideas.** Project facts (today `horch note --fact`) go to every sub-orchestrator. Each feature keeps its own decisions file.

**Plan and ticket formats.** `plan.yaml` replaces today's `MAP.md`. A ticket stays a Markdown file in today's format C1 (`docs/specs/ticket-format.md`), and `plan.yaml` names it.

**Protocol rules**

1. **Atomic writes.** A writer writes `name.tmp`, fsyncs, then renames to `name`. Readers ignore `*.tmp`. A half-written file can never be read.
2. **Immutable once landed.** `idea.md`, `task.yaml` and `result.json` are never rewritten. A change is a new attempt directory or a new sequence number.
3. **One writer per file.** The path says who owns it: fleetd owns `task.yaml`, `inbox/` and `verdict.json`; the wrapper owns `heartbeat.json`; the agent owns `outbox/`.
4. **Schemas.** Every file type has a JSON Schema in `/fleet/schemas/`. fleetd rejects a file that fails it and records why.
5. **Agents use the `fleet` helper.** `fleet progress`, `fleet result` and `fleet ask`, plus `horch idea note` for sub-orchestrators, validate and write files for the agent, so a model never hand-assembles JSON. Raw writes still work if they validate.
6. **The only terminal input is a constant.** When a pane must be woken, fleetd's code sends the fixed line `Read inbox/START.md in your task folder and continue.` Everything that changes lives in files.

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
    after: [backoff]
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
    kind: report
    judges: retry-doc       # plan validation refuses a judgment task with no judge
    profile: judge-codex    # a different session, and a different provider from the author
    after: [retry-doc]
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
lease: {expires: 2026-11-10T15:31:40-07:00, renew_every_s: 10}
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

The ledger is an event log plus derived tables in one SQLite file, written by fleetd alone, and every state change goes through one function that checks the transition is legal.

Today your agents write the ledger directly. That is the root of "orchestrator forgot a task" and "worker took a task and sat on it": the truth depends on a model running the right SQL at the right time. In this design agents only write files; fleetd turns validated files into ledger rows. Every other process, including `horch status`, `horch ideas` and every agent, opens the database read-only.

**Schema**

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE features (                    -- 1 sub-orchestrator, 1 feature branch, 1 pull request
  id          TEXT PRIMARY KEY,            -- F-0003
  name        TEXT UNIQUE,                 -- kebab-case, from the sub-orchestrator; checked by fleetd
  branch      TEXT UNIQUE,                 -- created when the sub-orchestrator first writes its assumptions
  primary_branch TEXT NOT NULL,            -- the project's base branch
  stacked_on  TEXT REFERENCES features(id), -- the older feature, after a trial merge conflicted
  state       TEXT NOT NULL CHECK (state IN ('open','ready','merged','abandoned')),
  pr          TEXT,                        -- pull request or compare URL
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
  started_at TEXT, ended_at TEXT, exit_code INTEGER,
  outcome TEXT                             -- verified, rejected, crashed, timed_out, stalled, killed
);

CREATE TABLE leases (
  task_id    TEXT PRIMARY KEY REFERENCES tasks(id),
  attempt_id TEXT NOT NULL REFERENCES attempts(id),
  expires_at TEXT NOT NULL,
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
  due_at      TEXT NOT NULL,               -- past it, the row escalates
  queue_seq   INTEGER,                     -- bundle order
  rescue_ref  TEXT,                        -- written before each rebase or forced update
  orphan      INTEGER NOT NULL DEFAULT 0   -- found in git with no row, adopted and reported
);

CREATE TABLE gate_runs (
  id          INTEGER PRIMARY KEY,
  feature_id  TEXT NOT NULL REFERENCES features(id),
  gate        TEXT NOT NULL CHECK (gate IN ('fast','full')),
  tip         TEXT NOT NULL,               -- the candidate or feature tip it judged
  base_tip    TEXT NOT NULL,               -- the feature branch or primary branch tip it judged
  result      TEXT NOT NULL CHECK (result IN ('pass','fail')),  -- a pass is stale once either tip moves
  at          TEXT NOT NULL
);

CREATE TABLE trial_merges (               -- facts from git merge-tree --write-tree
  left_ref TEXT, right_ref TEXT, left_tip TEXT, right_tip TEXT,
  clean INTEGER NOT NULL, conflict_files TEXT, at TEXT NOT NULL,
  PRIMARY KEY (left_tip, right_tip)
);

CREATE TABLE questions (                   -- 1 row per worker question
  id          TEXT PRIMARY KEY,            -- T-0007-02/A1/Q1
  task_id     TEXT NOT NULL REFERENCES tasks(id),
  feature_id  TEXT NOT NULL REFERENCES features(id),
  owner       TEXT NOT NULL,               -- the sub-orchestrator of the feature
  state       TEXT NOT NULL CHECK (state IN ('open','answered','escalated')),
  answer_case TEXT CHECK (answer_case IN ('covered','revise','replan','own_decision')),
  citation    TEXT,                        -- checked by fleetd for a covered answer
  due_at      TEXT NOT NULL,               -- past it, escalates to needs-you
  asked_at    TEXT NOT NULL, answered_at TEXT
);

CREATE TABLE events (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  at TEXT NOT NULL, entity TEXT NOT NULL, entity_id TEXT NOT NULL,
  kind TEXT NOT NULL, from_state TEXT, to_state TEXT,
  cause TEXT NOT NULL,                     -- e.g. lease_expired, approved_by_user, worktree_rescued
  data_json TEXT
);

CREATE TABLE ingested (                    -- makes file ingestion idempotent
  path TEXT PRIMARY KEY, sha256 TEXT NOT NULL, at TEXT NOT NULL
);
```

**Three rules that make the ledger trustworthy**

1. **One door.** All writes go through `transition(entity, id, to_state, cause)`, which looks up the allowed-transition table, writes the row and the event in one transaction, and raises on anything illegal. There is no other `UPDATE`.
2. **Leases, not claims.** A task in `leased` or `running` always has a lease row with an expiry. Leases are renewed by fleetd from outside signals (heartbeat file, process alive, output or hook activity), never by the model. When `expires_at` passes, the reconciler moves the task to `stalled` in the same pass that notices it. A task in `waiting_quota` has its lease paused: the lease does not expire, and the task leaves the state when its pool resets.
3. **Invariants checked every pass.** The reconciler asserts these and repairs or escalates on any breach:
   - I1: every `leased`/`running` task has exactly one unexpired lease and one attempt with a live process or pane.
   - I2: no task is `ready` while any dependency is not `done`.
   - I3: every requirement of an idea in `in_progress` or later is covered by at least one non-cancelled task.
   - I4: no worker runs more than one attempt at a time.
   - I5: every live pane and process under the fleet belongs to a live attempt; anything else is an orphan and is stopped.
   - I6: every worktree that `git worktree list` or `/fleet/work/` shows has a `worktrees` row, every row that is not `removed` has its directory, and no worktree is removed unless its branch landed or its rescue ref was verified.
   - I7: no idea is past `awaiting_approval` unless `approved_sha256` matches the current `assumptions.md`, and every idea from `clarifying` to `integrating` has exactly one live sub-orchestrator attempt or is waiting on you.
   - I8: a task is `landed`, and a feature is `merged`, only when its changes are in the target branch by content. This holds for a squash merge.
   - I9: a feature branch and the primary branch move only by fast-forward to a candidate that passed its gate.
   - I10: before each rebase or forced update of a branch, a rescue ref of the old tip exists.
   - I11: every fleet branch in git has a `branches` row; one without is adopted as an orphan row and reported, never deleted.
   - I12: a branch is deleted only when it is landed, merged or rescued, and no `git stash` entry names it.
   - I13: a sub-orchestrator exists for each feature that is not merged and not abandoned.

The task state machine is drawn under Supervision, liveness and reconciliation.

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

**Every worker persona names a fallback persona.** The roster refuses to load a persona without one. fleetd uses it when the persona's pool shows a usage limit or a provider outage (see the stall ladder).

**No budget, no cap, and the model guide.** There is no spend budget: a profile has no token limit. `max_minutes` stays as a hang guard, and the 256k context limit (G5) stays as a quality rule. There is no cap on features, sub-orchestrators or workers. Health guards are not caps: fleetd pauses new dispatch only while free disk or machine load is past a safety floor, and resumes by itself. On Windows the machine probe returns unknown today (`preservation-06-wiring-and-platform.md`, row S12). Pacing: fleetd reads the usage window of each pool and dispatches so that the usage is consumed by the reset. Plan validation checks the model guide:

- Opus 5.5 at medium effort is the default.
- Higher effort needs a stated reason.
- A Sonnet task must be marked as "no decisions".
- Fable needs your flag.

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
| `result_gate` | `Stop` hook: exit 2 with "run `horch result` first" while `result.json` is missing | `Stop` hook (managed config, since user hooks need trust) | `Stop` hook; plus the wrapper's repair pass in every case |
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

**Everything is an idea, with a fast lane.** There is 1 state machine and 1 way in. There is no task without an idea. `horch idea add --direct <ticket file>` takes a ticket that you wrote. It skips research and the interview, and makes a plan with 1 task. The idea goes from `captured` to `ticketing`; the ticket you wrote stands for your approval, and `horch idea add --direct` refuses to run inside a fleet attempt, like `horch idea approve`. The plan still passes plan validation, and the task still needs a check. When plan validation refuses the `--direct` plan, the idea goes to `needs-you/` with the reason. A `--direct` idea belongs to a feature (see Features, landing and integration): with `--to <feature>` it joins that feature, and with no `--to` it starts a new feature whose branch name fleetd makes from the ticket title. The feature of a `--direct` idea still has a sub-orchestrator, which owns its questions, conflicts and gate failures.

**The idea state machine**

| State | Who acts | Leaves when |
| --- | --- | --- |
| `captured` | Dispatcher | On the next pass it opens pane `idea-12` and moves to `clarifying`. An idea added with `--direct` moves to `ticketing`. |
| `clarifying` | Sub-orchestrator, with you in its pane | It has researched the repo and docs, interviewed you, and written `assumptions.md` with no item left open. |
| `awaiting_approval` | You | You run `horch idea approve I-0012`, or `horch idea revise I-0012 "..."` to send it back to clarifying. |
| `specifying` | Sub-orchestrator | `spec.md` and `requirements.yaml` validate, and every requirement cites an approved assumption or one of your answers. |
| `ticketing` | Sub-orchestrator | Tickets and `plan.yaml` pass the plan validator and commit in one transaction. |
| `in_progress` | Workers; the sub-orchestrator answers their questions and replans failed tasks | Every task is done. |
| `integrating` | fleetd | Every task of the idea is `landed` on its feature branch. The idea waits here until its feature's changes are in the primary branch. |
| `merged` | Nobody | Terminal. Reached only when git shows its feature's changes in the primary branch and every worktree of the idea is removed. |
| `parked`, `cancelled` | You | `horch idea park` or `cancel`; escalations also park. Worktrees are rescued first. |

&#91;embedded content: idea state machine · 8 states and 1 approval gate\]

Every idea belongs to exactly 1 feature, and every feature that is not merged and not abandoned has 1 sub-orchestrator. Merged also requires that every worktree of the idea is gone.

**How many sub-orchestrators.** One for each feature that is not merged and not abandoned. There is no cap on features, sub-orchestrators or workers, so no idea waits in `captured` for a slot. The count always matches the ledger, because the reconciler starts a missing pane or stops an extra one on every pass.

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

**Waiting on you is not a stall.** While an idea is in `clarifying` with a question open, or in `awaiting_approval`, its lease is paused and fleetd sends one reminder an hour during your working hours. After 30 minutes it stops the idle sub-orchestrator process to free memory; an open question or your approval starts it again from the folder. After 7 days with no answer the idea moves to `parked`, with a note in `needs-you/`, so the queue only counts ideas that can move.

**A worker that waits holds no process.** An exec worker that asks a question exits. Its task parks with no process and no time limit. The 1 h cap on a declared wait (`max_wait_s`) applies only to an interactive persona; at the cap the process exits and the task parks in the same way.

**How a worker's question is resolved.**

1. The worker writes 1 question file, commits its work in progress, and exits. The task parks. The other tasks of the feature go on.
2. The question is a ledger row with an owner (the sub-orchestrator of the feature), a state and a time limit.
3. On each pass, fleetd starts the owner if it is not running. Delivery repeats until an answer is recorded.
4. The answer is 1 of 4 cases:

| Case | What occurs |
| --- | --- |
| The approved assumptions or the spec cover it | The sub-orchestrator answers and cites the line. fleetd checks that the citation exists. |
| The answer changes an approved assumption | It goes to you as a `revise`. Only the affected tasks pause. |
| The question shows a flaw in the plan | The sub-orchestrator replans the task. |
| Nothing approved covers it, and it changes no assumption | The sub-orchestrator decides when the code or the spec makes the answer clear. It records the answer as a new assumption marked as its own decision. You can reverse it. Only product direction, scope or a trade-off that you must own goes to you. |

5. fleetd resumes the worker's session with the answer. If the resume fails, a fresh worker starts from the question, the answer and the committed work.
6. The answer goes to the decision log of the feature and to every later task of the feature.
7. Past its time limit, a question escalates to you in `needs-you/`, with a herdr notification.

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
| `landing` | Its branch is in the landing state machine (see Features, landing and integration). | Its branch is `landed` on the feature branch → `landed`. |
| `landed` | Nothing in it is unique any more. | fleetd removes it and deletes the branch only if it is merged → `removed`. |
| `orphaned` | On disk with no live owner, or on disk with no row at all. | Rescued and verified → `rescued`. |
| `rescued` | Its full state is saved on a rescue ref and checked. | Adopted by the next attempt of its task, or removed when its idea is merged or cancelled. |
| `removed` | Gone from disk and from `git worktree list`. | Terminal. |

**Only fleetd creates or removes worktrees.** Workers start inside a tree fleetd made. Their profiles deny `git worktree` through each CLI's permission rules, and a new `arch_scan` rule keeps worktree calls out of every horch code path except fleetd. A worker that makes one anyway is found by the reconciler as an unowned tree and rescued like any orphan.

**The reconciler compares three views.** On every pass it reads the `worktrees` rows and the directories under `/fleet/work/`; every 60 s it also reads `git worktree list --porcelain` for each repo the fleet uses, including prunable and locked entries. Then:

- A directory with no row is adopted as `orphaned`, linked to the task its branch name points to, if any.
- A row whose owner attempt is dead becomes `orphaned` in the same pass that sees the death.
- A `reserved` row older than a minute with no directory is dropped.
- A row whose directory vanished without fleetd removing it is escalated, because something outside the fleet deleted work.

**Rescue before any cleanup.** Rescue never touches the worktree's own branch or index, so it is safe to run on a tree a dying process was still writing:

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

fleetd then writes `/fleet/rescue/T-0007-03-1.json` with the ref, the old HEAD, the file count, the size and the attempt. Only after that may a tree be removed. A rescue that fails verification leaves the tree in place and escalates. Ignored files such as build output are not saved, which keeps rescue refs small; a profile can list ignored paths worth keeping in `rescue_include`.

**Reuse, don't redo.** The next attempt of the same task gets the same worktree, with the rescue ref as a safety copy, and its `START.md` names the rescue commit and the last attempt's progress notes. Work you paid for becomes the starting point of the retry.

**Disk.** `horch status` shows the total size of all trees. Above `worktree_disk_budget` (default 20 GB) fleetd removes `landed` and `rescued` trees of merged or cancelled ideas, oldest first. It never removes an `active` tree for space; it pauses new dispatch and tells you instead.

**Ideas finish clean.** An idea reaches `merged` only when every one of its worktrees is `removed`, and its closing note lists any rescue ref that never landed. `horch worktrees` lists every tree with its state, task, owner, age and size, and `horch worktree restore T-0007-03` turns a rescue ref back into a branch `rescue/T-0007-03-1` and a working tree.

**Built on what horch has.** The existing `worktrees` table gains the state, owner and rescue columns, triage reads them, and `refs/horch/rescue/` sits beside your archive and trash refs under the same retention rules.

## Features, landing and integration

Work lands in 2 steps, and the ledger tracks each branch through both: a task branch lands on its feature branch automatically, and a feature branch reaches the primary branch through 1 pull request.

The primary branch is the base branch of a project. It is `main` in this repository, but it is not `main` in every project fleetd runs.

### Features

- A feature is 1 sub-orchestrator, 1 feature branch and 1 pull request to the primary branch. It holds 1 main idea and its follow-on ideas.
- fleetd creates the feature branch when the sub-orchestrator first writes its assumptions. The sub-orchestrator gives the name. fleetd checks that it is kebab-case and unique. fleetd writes the ledger row before it creates the branch.
- `horch idea add --to <feature>` adds an idea to an open feature. Its tasks land on that feature branch. With no `--to`, the idea starts a new feature. An idea belongs to exactly 1 feature, and the ledger records it.
- A feature is an intake funnel until its pull request merges. After the merge, a follow-up idea starts a new feature.
- The sub-orchestrator of a feature plans 1 idea at a time, with a fresh process for each phase. Tasks of several ideas of 1 feature run in parallel.

### Landing a task branch on its feature branch

Every branch is a ledger row before it exists. Each row has a landing state, 1 owner and a time limit. Past the limit it escalates. No state is silent.

| State | Owner | Leaves when |
| --- | --- | --- |
| `building` | worker | the worker writes its result |
| `verifying` | fleetd | the check, and for a code kind the review, pass |
| `ready` | fleetd | fleetd puts it in the next bundle |
| `in_bundle` | fleetd | the bundle passes the fast gate and the feature branch moves forward |
| `landed` | nobody | terminal |
| `conflicted` | the same worker | its rebase conflict is resolved (today's behavior) |
| `gate_failed` | sub-orchestrator | its triage task lands |
| `abandoned` | operator only | terminal; a rescue ref is kept |

**The bundle.** When several task branches of 1 feature are `ready` together, fleetd builds 1 candidate from the feature branch tip plus each ready task branch in queue order. Each task's commits are kept. The fast gate runs once on the candidate. On a pass, the feature branch moves forward to it. On a failure, fleetd bisects the bundle, sends the failing task to `gate_failed`, and the others go on. No ready branch is forgotten, because each one is a row that fleetd reads on every pass.

No human approves a landing on a feature branch.

fleetd keeps these behaviors of today's land path (`preservation-04-behavior.md`):

- F9: under the land lock, just before the fast-forward, fleetd checks that the task is still the worker's. A task cancelled earlier lands nothing.
- F7: a red land resets the branch to its tip from before the rebase.
- F5: a passed step stands only for the tree it judged. New commits reopen the land and every later step.
- F11: every git network call has a time limit (push 10 min, others 3 min).
- L6: 2 running tasks of 1 feature never own overlapping paths.

### No work is lost

fleetd checks these on every pass against git. They are invariants I8 to I13 in the Ledger section.

1. A task is `landed`, and a feature is `merged`, only when its changes are in the target branch by content. This holds for a squash merge.
2. A feature branch and the primary branch move only by fast-forward to a candidate that passed its gate. A landing cannot overwrite another.
3. Before each rebase or forced update of a branch, fleetd writes a rescue ref of the old tip.
4. A fleet branch in git that has no ledger row is adopted as an orphan row and reported. It is never deleted.
5. A branch is deleted only when it is landed, merged or rescued. A `git stash` entry that names the branch blocks the deletion (`preservation-04-behavior.md`, row W6).
6. A sub-orchestrator exists for each feature that is not merged and not abandoned.

### Between features

- **Parallel is the default.** Any number of features can change the same file. A shared file name never makes a feature wait.
- **Early detection.** When a feature branch tip or the primary branch moves, fleetd makes a trial merge of each in-flight feature against the primary branch and against each other in-flight feature. It uses `git merge-tree --write-tree`, with no checkout. The result is a fact in the ledger.
- **Stack on evidence.** When the trial merge of 2 features conflicts, fleetd puts the younger feature on top of the older one. The conflict becomes 1 resolver task for a fresh agent. The agent gets the conflict in `zdiff3` form, the requirements of both features for those files, and the unit tests of both. It makes 1 commit for each requirement. The task lands on the younger feature's branch.
- **Continuous rebase.** When the primary branch moves, fleetd rebases each feature branch, the bottom of a stack first. After a clean rebase the fast gate runs again. A conflict becomes a resolver task. `rerere` is on.
- **Build on what is in flight.** At plan time the sub-orchestrator gets the diffs of the files that in-flight features already changed. It can propose to build on one of them. The proposal is in the assumptions that you approve.
- **The full gate.** The full gate runs once for a feature, when its tasks are landed, on its tip over the current primary branch. Failures become triage tasks of the feature. If either tip moves after a pass, the pass is stale and the gate runs again before the feature is marked ready.
- **Two declared gates.** Each project declares its fast gate (its unit tests) and its full gate (the fast gate plus the end-to-end tests). Plan validation refuses a plan for a project that did not declare both. This repository has no unit-only command today (`research-03-this-repo.md`, F10).
- **The pull request.** A project setting `pr` is `gh` or `manual`. `manual` pushes the feature branch and shows the compare URL. A stacked feature's pull request has the lower feature's branch as its base. A project setting says who merges; the default is you.
- **Merged.** A feature is `merged` when its changes are in the primary branch. fleetd reads this from git, not from the GitHub API.
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

fleetd derives one agent state per attempt: `starting`, `working`, `waiting_approval`, `idle_no_result`, `hung` (process alive, no activity for `stall_after`, default 5 min), or `exited`.

**Lease renewal is earned, not claimed.** fleetd renews a lease only when the process is alive, the heartbeat is under 30 s old and there was activity within `stall_after`. A worker that holds a task and does nothing stops earning renewals, and its lease expires within 120 s of the stall threshold. That is how "took the task and sat on it" becomes impossible to hide.

**A sleep does not expire a lease.** Lease time counts only while the machine is awake. The platform layer reports sleep and wake, and fleetd does not count the time the machine slept against a lease or a stall timer.

**Stall ladder.** Every rung is code with a fixed timer; none depends on a model reading anything.

A usage limit or a provider outage is not a stall. A worker that shows one is not stuck, is never reaped as stuck, and does not burn an attempt. If the persona's fallback is on another provider, fleetd restarts the task at once on a new worker of the fallback persona, on the same worktree. If not, the task goes to `waiting_quota`, its lease is paused, and it leaves the state when the pool resets. If both pools are limited, the task waits; fleetd does not go to a third persona. Today no code enforces "never reap a usage-limited worker": the rule is only in the orchestrator briefing (`preservation-04-behavior.md`, row S6). fleetd enforces it in code.

1. `idle_no_result` in an interactive pane: send the constant doorbell once. For Claude Code a Stop hook already refuses to let the turn end until `result.json` exists (see Worker profiles), so this rung is mostly for other CLIs.
2. Still idle 2 minutes later: doorbell again and record it.
3. Still idle, or `hung` at any point: end the attempt. Stop the process group through the platform layer (on Linux: SIGINT, wait 20 s, then SIGKILL). Record outcome `stalled`.
4. `waiting_approval`: this is a profile defect, because profiles pre-approve everything a task needs. fleetd ends the attempt, writes the screen excerpt to `needs-you/`, and marks the profile unhealthy until a conformance test passes.
5. Process exited without a valid result: outcome `no_result`.
6. Any non-verified outcome: if attempts remain, a new attempt starts after backoff (30 s, 2 min, 8 min) in the same worktree, with a handoff file built from the old attempt's `progress.jsonl`, `git diff` and transcript tail. Otherwise the task goes to its idea's sub-orchestrator to replan once, then to `needs-you/`.

**Finishing without reporting is closed at both ends.** Inside the agent, a stop hook (or for exec mode, the wrapper on exit) checks for `result.json`. If a process exits with commits but no result, the wrapper runs one short resume of the same session with a fixed instruction to write the result, then exits. Outside the agent, "done" is defined only by the verifier running the acceptance commands, so a result that claims success without passing checks goes back to `ready`.

**The reconcile pass**

```python
def reconcile():
    snap  = mux.snapshot()          # herdr session.snapshot: panes, agents, status
    procs = scan_processes()        # pids and pgids recorded in attempts
    files = scan_outboxes()         # full scan; file watching only makes it run sooner
    trees = scan_worktrees()        # /fleet/work every pass, git worktree list every 60 s
    with ledger.tx() as tx:
        ingest(tx, files)                     # idempotent by (path, sha256)
        advance_ideas(tx, files)              # idea state machine: files valid -> next state
        keep_sub_orchestrators(tx, snap)      # one per feature that is not merged or abandoned
        promote_pending(tx)                   # deps all done -> ready
        dispatch(tx, health(), pacing(tx))    # lease worktree, write task.yaml + START.md, launch; no cap
        for a in live_attempts(tx):
            state = liveness(a, snap, procs, files)
            renew_or_expire_lease(tx, a, state)
            apply_stall_ladder(tx, a, state)
        finish_exited(tx, procs)              # result -> verifying, else retry/escalate
        run_due_verifications(tx)             # async jobs, verdict.json on completion
        reconcile_worktrees(tx, trees)        # adopt, mark orphaned, rescue, remove landed
        stop_orphans(snap, procs, tx)         # panes or pids not owned by a live attempt
        land_bundles(tx)                      # ready task branches -> 1 bundle per feature, fast gate, fast-forward
        integrate_features(tx)                # trial merges, stacks, rebases, resolver tasks, full gate
        check_invariants(tx)                  # I1..I13, repair or escalate
        roll_up_ideas(tx)                     # all tasks landed on the feature branch -> integrating; merged once git shows the feature in the primary branch and trees removed
    write_heartbeat(now)            # time of the last full pass; `horch fleetd check` reads it
    platform.watchdog_ping()        # systemd_notify("WATCHDOG=1") on Linux; macOS and Windows use `horch fleetd check`
```

Dispatch order matters for "workers don't find work": fleetd writes `task.yaml` and `inbox/START.md` before it launches the process, and passes the START.md path as the launch argument. The worker never searches; the task is the first thing it is given. If the process starts but never touches its files, the start timeout (90 s with no first heartbeat or activity) ends the attempt.

&#91;embedded content: task state machine · 9 states\]

Only the verifier moves a task to done. A stall, a crash, a missing result and a failed check all land in the same attempt-failed state, which either retries or escalates, so no path leaves a task held and forgotten.

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
4. If any check fails, mark every profile using that CLI as `unhealthy` in fleetd, which stops dispatching to it, and queue an idea "Update the \<tool> adapter for \<version>" with the diff and failing checks attached. You see it in `needs-you/`.

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
| 05 E-10, 06 S1 | `ANTHROPIC_API_KEY` is removed from every child process, every pane command line, every detached child, every updater and every probe, and the roster rejects a teammate file that sets it. | `runtime/process.rs:190`, `:277`, `workspace/paneshell.rs:11`, `harness/update.rs:507`, `quota_probe.rs:110`, `roster/validation.rs:481`, `:1681` | Absolute operator rule: every `claude` run uses the subscription login. | API billing instead of the subscription; a broken operator rule. |
| 06 S2 | herdr's own pane resume uses an environment that horch did not scrub. horch warns when `resume_agents_on_restore` is not false. | `cmd/restore.rs:62` | The forbidden key can reach a resumed agent. | A herdr-resumed agent bills the API key. |
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
| 06 S15 | The OpenCode, pi and Prime adapters carry over. | `routing/quota.rs:46` | They give the `opencode-zen` and `local` pools. | Those pools are lost. |

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
| A stalled git network call holds the land lock (04 F11) | High: seen on 2026-11-08 | Stuck fleet | Wall-clock bounds on every git network call. |
| A restart in the middle of a reap loses the requeue (04 S7) | Low | Stuck task | Requeue in 1 transaction before any signal. |
| An escalated task can never run again (04 L8) | Medium | Stuck task | Attempt reset on your requeue. |
| 2 tasks of 1 feature edit 1 file (04 L6) | Medium | Conflicting commits | Own exclusion inside 1 feature. |
| Long suites fail, or 2 lands race (05 V-10, V-11) | Medium | Stuck fleet | Keep today's land lock and check timeout values. |
| A newer schema is opened by an older binary (05 L-3) | Medium with automatic updates | Corrupt or locked ledger | Newer schema refused; a CLI restarts an older daemon (OR-6). |
| A failed migration loses history (05 L-4) | Low | Lost ledger history | Backup before each migration, 3 kept. |
| A development build migrates the shared ledger (05 L-5) | Medium | Installed horch refuses the ledger | Dev-build guard (`HORCH_ALLOW_LEDGER_MIGRATE`). |
| A test or scratch run writes the shared ledger (05 E-1, E-2) | Medium | Corrupt shared state | `HORCH_LEDGER_DB` and `HORCH_STATE_DIR` isolation stay beside `FLEET_ROOT`. |
| `ANTHROPIC_API_KEY` reaches a child (05 E-10, 06 S1, S2) | Medium | API billing; broken operator rule | Scrub on every child, pane line, detached child, updater and probe; roster check; the herdr resume warning. |
| Google keys reach `agy` (06 S3) | Low | API billing | Scrub and roster check. |
| Another local user plants instructions (04 M5) | Low | Agent acts on planted lines | Mode 0700 and the owner check on every mail and outbox directory. |
| A sandboxed persona runs unsandboxed (06 S9) | Low | Writes outside allowed paths | Refuse to start on a host that cannot sandbox it. |
| `stop` signals a PID of another host (04 S8) | Low | An unrelated process is killed | Refuse a worker of another host. |

## Failure modes and effects

Every failure you listed, plus twenty-six more found while reading the code, the CLI docs and your new requirements, is either prevented by construction or caught by a timer in code within minutes, and each traces to a requirement in the last column.

"Caught within" assumes the default timers (reconcile every 5 s, git scan every 60 s, lease 120 s, stall after 5 min). Rows FM-1 to FM-8 and FM-25 are the failures you reported.

| ID | Failure mode | Effect | Mechanism that removes or catches it | Caught within | Req |
| --- | --- | --- | --- | --- | --- |
| FM-1 | Orchestrator forgets to create a task | Part of an idea is never built, silently | No AI creates tasks: they come only from a validated `plan.yaml`, committed in one transaction after your approval; the coverage check fails the plan if any requirement has no task | Prevented | FR-3, FR-4, FR-10 |
| FM-2 | Worker starts but doesn't find its work | Idle pane, task never runs | Push dispatch: fleetd binds the task to the attempt and writes `task.yaml` before launch; no claim, persona match or run lookup | Prevented; a launch that never starts fails at 90 s | FR-5 |
| FM-3 | Worker holds a task and sits idle | Task blocked for hours or days | Leases renewed only on earned activity; an exec worker that asks a question exits and its task parks with no process; an interactive persona's waits are capped by `max_wait_s` and need a live question or review row behind them | About 7 min | RR-2 |
| FM-4 | Agent process dies silently | Task looks running | PID and start time checked every 5 s pass; wrapper heartbeat stale at 30 s | 5 to 30 s | RR-1 |
| FM-5 | Agent alive but stuck at a login, rate-limit, trust or error prompt | Looks alive forever | A usage limit or a provider outage is not a stall: no attempt is burned, the task restarts at once on the fallback persona when it is on another provider, else waits in `waiting_quota` with its lease paused until the pool resets. A login, trust or error prompt: herdr `blocked` state, `Notification` and `PermissionRequest` hooks, no activity past `stall_after`; profiles pre-approve so any dialog is a defect | Immediately for dialogs and usage limits, 5 min otherwise | RR-1, RR-11, FR-8 |
| FM-6 | Worker finishes but never reports | Work lost or redone | Stop gate refuses to end the turn without `result.json`; exec mode exit without result triggers one repair resume; the next attempt reuses the same worktree | Prevented; worst case one retry that starts from the work | RR-3 |
| FM-7 | Watchdog misses updates | Any of the above lingers | One supervised daemon, level-triggered pass every 5 s, full scans instead of trusting events, actions in code instead of mail to an LLM | 5 s | RR-4, RR-5 |
| FM-8 | Stacked ideas get merged | Wrong plan, tangled branches | Each idea is its own file, ledger row and sub-orchestrator; a fresh process per phase whose only memory is that idea's folder; 1 process never plans 2 ideas at the same time | Prevented | FR-1, FR-2 |
| FM-9 | Worker claims done but work is broken | Bad code lands | Only the verifier moves a task to done: the acceptance commands, then for a code kind today's review scripts, and for a judgment kind the structural check and the paired judge | At verification | RR-3 |
| FM-10 | Plan has a cycle, missing profile or uncovered requirement | Deadlock or gap | Plan validator rejects before commit; the sub-orchestrator reruns ticketing with the errors | Prevented | FR-3 |
| FM-11 | fleetd crashes mid-pass | Half-applied state | Every pass is one SQLite transaction; ingestion is idempotent; the service manager of the OS restarts it; the next pass rebuilds from ledger and files | Under 30 s | RR-7, RR-8 |
| FM-12 | fleetd hangs | Nothing moves | On Linux the systemd watchdog (`WatchdogSec=30`) kills and restarts it when the pass stops pinging. On macOS a launchd timer job, and on Windows a scheduled task, runs `horch fleetd check` every 30 s, and it restarts the service when the heartbeat is older than 30 s of awake time. Each `horch` CLI command makes the same check | 30 s to 60 s of awake time on every OS | RR-7, PL-5 |
| FM-13 | herdr server restarts | Every agent process is gone | Reconciler sees dead PIDs and missing panes, fails those attempts, rescues their worktrees and retries in them; sub-orchestrators restart from their folders | 5 to 30 s | RR-4 |
| FM-14 | Two workers on one task | Conflicting commits | Lease row is unique per task and a worktree has one owner; fleetd stops the old attempt by recorded PID before starting the next (your ADR 0002) | Prevented | RR-2 |
| FM-15 | Typed prompt lost or delivered twice | Missed or duplicated instruction | Nothing meaningful is typed. The doorbell is constant and idempotent; content lives in files | Prevented | FR-7 |
| FM-16 | One AI conversation buried in ideas, worker mail and wakes | Context churn, ideas blur | There is no long-lived orchestrator conversation. Reports go to files and the ledger; a sub-orchestrator sees only its own feature's questions; you read `horch ideas` and `needs-you/` | Prevented | OR-3, FR-2 |
| FM-17 | Codex sandbox blocks writes to the task folder | No result, looks like a stall | Task folder in `writable_roots`; conformance test proves the write; the old `horch ledger` sandbox escape is no longer needed | Before dispatch | SR-1, FR-8 |
| FM-18 | Trust or approval prompt on first launch | Pane frozen | Trust pre-seeded in each private config home; conformance check "no trust prompt" | Before dispatch | FR-8 |
| FM-19 | CLI update changes a flag or default | Every worker of that CLI fails | Docs sync plus conformance on version change; failing CLI marked unhealthy | Before dispatch | FR-8 |
| FM-20 | Context drift past 256k, or a CLI with no compaction setting | Quality drops, loops | CLI compaction settings; wrapper ends the attempt with a handoff at the 256k context limit | At the limit | FR-9 |
| FM-21 | Retry loop burns quota | Cost with no progress | Bounded attempts and backoff, one replan, then escalation; a per-attempt wall-clock limit (`max_minutes`) as a hang guard | Bounded | RR-6 |
| FM-22 | Orphan processes or panes after a reap | Leaked CPU, stray edits | Process-group kill plus exact pane close; invariant I5 stops anything not owned by a live attempt | 5 s | RR-4 |
| FM-23 | Half-written file read | Corrupt result | Temp file, fsync, rename; readers skip `*.tmp`; schema validation | Prevented | RR-8 |
| FM-24 | Ledger write contention or a stray writer | Busy errors, lost updates | Single writer; everything else opens read-only | Prevented | RR-5 |
| FM-25 | Worker dies and leaves its worktree on disk | Paid-for work invisible, later deleted or redone | The worktree was leased before it existed, so owner death marks it `orphaned` in the same pass; a verified rescue ref is written before anything else; the next attempt reuses the tree | 5 to 30 s | FR-11, RR-9 |
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

## Migration plan

Migrate in eight phases, each a small set of horch changes with a fault-injection test as its exit gate, starting with the supervised loop, leases and worktree rescue because they stop the daily stalls and the lost work with the least code.

Each phase keeps the fleet usable. A phase is released when its fault-injection tests pass, and it is on for every user at once. The tests are the only gate.

**Rollout is automatic and immediate.** fleetd is a subcommand of the single `horch` binary, so the CLI and the daemon cannot have different versions. `horch self-update` installs a new release and restarts the running daemon on the new binary at once. This is safe because all state is in the ledger and in files, and the loop derives everything again on its next pass. Today the watchdog already restarts itself when its binary changes (`preservation-04-behavior.md`, row S10). A CLI that finds an older daemon restarts it. Harness updates keep today's rule: a release must be more than a week old (`update_cooldown_days`).

**Schema migration is forward-only and runs at daemon start.** Today's safety stays (`preservation-05-contracts-and-values.md`, rows L-3, L-4, L-5): a newer schema is refused, a backup file is written before each migration, and a development build does not migrate the shared ledger.

1. **Phase 0: reproduce every failure in tests.** Add scenarios to `crates/horch-e2e` using the fake harness and the existing fault points: an agent that never claims, one that claims and stops, one that exits without `done`, a killed watchdog, a herdr server restart, two ideas sent back to back, a worker killed with dirty and untracked files in its worktree. Add red tests for: several task branches ready at the same time, and none is forgotten; 2 features that conflict; a usage-limited worker that must not be reaped; a machine sleep that must not expire a lease; a daemon restart on a new binary in the middle of a run; a hung fleetd that is restarted, on each OS. Today most should fail. *Gate:* FM-1 to FM-8 and FM-25 each have a red test, and each new scenario above has a red test.
2. **Phase 1: `horch fleetd`, supervised and level-triggered.** Move the watchdog's pass into a long-lived daemon under the service manager of each OS through the platform layer (on Linux a systemd user unit with `WatchdogSec`; on macOS a launchd timer job and on Windows a scheduled task that run `horch fleetd check` every 30 s), a 5 s pass that writes a heartbeat, and invariants I1 to I5. The platform layer lands in this phase, with 1 module for each of macOS, Linux and Windows. CI runs the test suite on Windows; today the Windows job only builds (`.github/workflows/ci.yml`, job `build-windows`). It acts in code (stop by recorded PID, requeue, respawn) instead of only mailing the orchestrator. Remove the lazy `ensure` start. *Gate:* FM-4, FM-7, FM-12, FM-13 green.
3. **Phase 2: leases and capped waits.** Schema v8 adds a `leases` table. Renewal comes only from earned activity. `worker_waits` rows of an interactive persona get `max_wait_s` (default 1 h) and must point at a live question or review; an exec worker that asks a question exits and its task parks. Replace "3 checks of 600 s" with the stall ladder. Add the `waiting_quota` state, the required `fallback` persona in the roster, and the restart on a fallback of another provider. *Gate:* FM-3, FM-5 green; a usage-limited worker is not reaped and burns no attempt; the idle-claim scenario is caught in under 10 min.
4. **Phase 3: worktree leases and rescue.** Extend the existing `worktrees` table with state, owner, base and rescue columns, and make fleetd (through `horch spawn` until Phase 4) the only code that runs `git worktree add` or `remove`, writing the row first. Add the worktree reconciler, the verified rescue to `refs/horch/rescue/`, reuse by the next attempt, invariant I6, `horch worktrees` and `horch worktree restore`. Run it once in report-only mode to list the orphans already on your disk before it acts. *Gate:* FM-25 to FM-28 green; a worker killed with dirty and untracked files loses nothing.
5. **Phase 4: push dispatch and the result gate.** `horch spawn` binds a task before the pane exists and writes `task.yaml` and `START.md`; `CLAIM_TASK` goes away. Add Stop hooks for Claude Code, Codex and Antigravity and a `horch result` command that writes `result.json`. fleetd runs the finish steps (check, sweeps, land) as the verifier instead of the worker detaching them and waiting for a typed NOTE. For a code kind the review scripts carry over unchanged: code-sweep rounds, findings F<n>, the answers file, waivers and verification profiles, with their limits (V-14). *Gate:* FM-2, FM-6, FM-9 green.
6. **Phase 5: one ledger writer.** Worker commands (`progress`, `result`, `ask`) write files into the attempt's outbox; fleetd ingests them. Moving worker reports to files removes the Unix-socket need of the worker channel on Windows (`preservation-06-wiring-and-platform.md`, row S11). The fleetd command channel still needs a Windows transport. Every other process opens the ledger read-only, enforced by a new `arch_scan` rule. Remove Codex's `broad_ledger_rule` sandbox escape. Each `horch` command that changes state sends a request to fleetd over its command channel, or writes a request file when fleetd is down. *Gate:* FM-17, FM-24 green; arch test passes.
7. **Phase 6: the idea dispatcher and sub-orchestrators.** Schema v9 gives `ideas` the state machine above and adds `horch idea add` (with the `--direct` fast lane), `ideas`, `show`, `note`, `approve`, `revise` and `park`. Add the dispatcher loop to fleetd, invariant I7, and the `idea-lead` persona, which runs today's brief, spec-writer and ticket-writer steps for one idea in its own interactive pane, a fresh process per phase. `plan.yaml` replaces `MAP.md`; tickets stay Markdown in format C1 and `plan.yaml` names them. Extend `add-map --dry-run` and `scripts/check-req-coverage.sh` into the plan validator (schema, DAG, coverage, profiles, citations to approved assumptions). Retire the long-lived orchestrator conversation, and allow one feature branch per feature with several at once in place of one non-solo run per workspace. Add the `features`, `branches`, `gate_runs` and `trial_merges` tables and `horch idea add --to`. *Gate:* FM-1, FM-8, FM-10, FM-16, FM-29 to FM-32 green.
8. **Phase 7: docs sync and conformance, then exec mode.** Widen `herdr-docs-sync` to every CLI with the conformance suite and unhealthy marking. Then make exec mode the default worker mode and set `context-windows.md` to 256000 for every seat (today it is 300000 for the orchestrator and 200000 for opus). *Gate:* FM-18 to FM-20 green.

**New work for each phase**

| Phase | New work |
| --- | --- |
| Phase 1 | The platform layer, the health guards, pacing. |
| Phase 2 | `waiting_quota`. |
| Phase 4 | The 2 declared gates, the landing state machine. |
| Phase 6 | The feature level, the fast lane, judge tasks, the model guide in plan validation, the question flow. |
| Each phase that changes the schema | The backup before the migration and the newer-schema refusal. |

Phases 1 to 4 remove most of the stalls you see day to day and stop worktrees from being lost. Phase 6 is the largest change and depends on Phase 5, because a plan has to land through the single writer to be all-or-nothing, and only fleetd may record your approvals.

## Open questions

Your 5 choices are answered below. Eight facts need a live test; none blocks Phases 0 to 5.

**Your call (answered)**

| Question | Answer | Decision |
| --- | --- | --- |
| Exec mode as the default for workers? | Yes. A worker runs headless, does its task and exits. A persona can be marked interactive; sub-orchestrators are interactive, and an interactive persona is the way to pair with a worker in a pane. | D7 |
| One run, one branch and one PR per idea, with several ideas running at once? | No: 1 feature branch and 1 pull request per feature, which holds 1 main idea and its follow-on ideas. Several features run at once, in parallel by default. See Features, landing and integration. | D5, D10, D11, D12 |
| How many sub-orchestrator panes at once? | No cap. 1 sub-orchestrator per feature; an idle one stops after 30 min to free memory. Only health guards pause new dispatch. | D8 |
| Which host runs the fleet day to day, macOS (launchd) or Linux (systemd)? | All 3: macOS, Linux and Windows, behind 1 platform layer. | D1 |
| Is 1 hour the right cap on a worker's declared wait? | An exec worker that asks a question exits, and its task parks with no process and no time limit. The 1 h cap applies only to an interactive persona. | Settled by the orchestrator with D8; question flow D13 |

**To confirm by test**

- [ ] How to pre-trust Codex hooks for unattended workers (managed or requirements config). The docs say user hooks need trust through `/hooks`.
- [ ] Whether a private `HOME` fully isolates `agy` config, skills and MCP, and keeps its login.
- [ ] Whether herdr's `pane report-agent` state from the wrapper overrides its screen-based detection in the sidebar, or sits beside it.
- [ ] Which fields herdr's custom key commands take, so one key can run `horch idea approve` for the focused idea pane.
- [ ] herdr preview 0.9.4 against 0.9.5: whether the facts in the herdr table hold for the installed preview.
- [ ] A Windows transport for the fleetd command channel.
- [ ] The size limit of a resolver task (starting value: 100 changed lines or 2 files).
- [ ] The accuracy of a structural merge tool on this repository's recorded conflicts.
