# Spec: the task ledger

Status: draft for operator review. Nothing here is built yet.

## 1. Why

Today horch has no task model. A task is 1 free-text string on a worker's
session record. `horch assign` overwrites it. Dependencies, waves,
parallelism and "is all the work done?" live only in prompts and markdown
(`MAP.md`, `STATUS.md`). Workers have no heartbeat, the ledger has no PID,
and the orchestrator pushes every task into a pane by typing.

The task ledger changes the direction of work:

- The orchestrator only **queues** tasks into the ledger and spawns workers.
- A worker **claims** a task from the ledger when it starts.
- Every worker reports through the ledger: claim, progress, land, done.
- `horch tell` stays for questions and answers. It is not how tasks are
  created, assigned, reported or counted.

Orchestrators and workers both use 1 CLI: `horch ledger ...` (section 6).

## 2. Where the database lives

One SQLite file for every project on the machine:

| OS | path |
|---|---|
| Linux | `~/.horch/ledger.sqlite3` |
| macOS | `~/.horch/ledger.sqlite3` |
| Windows | `%USERPROFILE%\.horch\ledger.sqlite3` |

- `~` is `$HOME`, or `%USERPROFILE%` on Windows (`Paths::home`).
- `$HORCH_LEDGER_DB` overrides the path. Tests use it.
- The same place on every OS, so a human or an agent can find it without
  knowing XDG rules.
- Opened in WAL mode with `busy_timeout = 5000`. Every write that reads
  first (claim, reap, land) uses `BEGIN IMMEDIATE`, so 2 workers can never
  claim the same task.
- The file must be on a local disk. SQLite locking is not safe on a
  network share.
- `PRAGMA user_version` holds the schema version. horch migrates forward
  on open.

All timestamps are RFC 3339 UTC text, taken from the writer's clock at the
moment of the write ("now"). All ids are UUID text unless stated.

## 3. Tables at a glance

```
projects ─< runs ─< tasks ─< task_deps
                      │  └─< task_files
                      ├─< workers ─< checks
                      │      └─< stops
                      ├─< worktrees
                      └─< events
```

| table | 1 row is | written by |
|---|---|---|
| `projects` | 1 repository on this machine | horch |
| `runs` | 1 fleet session (1 orchestrator) | `horch fleet` |
| `tasks` | 1 unit of work (1 ticket file) | orchestrator |
| `task_deps` | "task A waits for task B" | orchestrator |
| `task_files` | 1 file a task owns or must not touch | orchestrator |
| `workers` | 1 worker process life: 1 PID, 1 session, at most 1 task | spawn, `horch worker`, the agent |
| `checks` | 1 orchestrator progress check of 1 worker | the watchdog (section 5.3) |
| `stops` | 1 signal sent to 1 exact PID | `horch ledger stop` |
| `worktrees` | 1 git worktree, from create to deleted from disk | `horch ledger worktree ...` |
| `events` | 1 line of history (note, question, claim, land ...) | everyone |

A worker never stays alive after its task. So 1 `workers` row is also
"1 attempt at 1 task". When a worker dies and the task is retried, the old
row ends `incomplete` and a **new** row (new PID, new session) points at
the same task. That is the "new ledger entry for the new worker".

## 4. The tables, field by field

### 4.1 `projects`

| field | type | used for |
|---|---|---|
| `project_id` | TEXT PK | key for everything below |
| `root` | TEXT UNIQUE | absolute repo path. `horch` finds the project from the current directory by this. |
| `created_at` | TEXT | audit |

### 4.2 `runs`

1 row per `horch fleet` session. Groups tasks so "is this run done?" has an
answer.

| field | type | used for |
|---|---|---|
| `run_id` | TEXT PK | key |
| `project_id` | TEXT FK | which repo |
| `slug` | TEXT | human name, e.g. `design-skills`; matches `ai_docs/plans/<slug>/` |
| `orchestrator_worker_id` | TEXT FK workers | the orchestrator's own process row (it has a PID and session too) |
| `workspace_id` | TEXT | the herdr workspace; scopes panes |
| `base_branch` | TEXT | default branch tasks land on |
| `check_every_s` | INTEGER | progress check interval. Default 600 (10 min). |
| `stall_rounds` | INTEGER | checks in a row without progress before a worker is declared dead. Default 3. |
| `heartbeat_every_s` | INTEGER | how often `horch worker` writes `last_checkin_at`. Default 30. |
| `heartbeat_stale_s` | INTEGER | a checkin older than this is "missed". Default 120. |
| `timer_tolerance_s` | INTEGER | slack when comparing a screen timer with the clock. Default 60. |
| `status` | TEXT | `active`, `done`, `abandoned` |
| `started_at`, `ended_at` | TEXT | audit |

The thresholds sit on the run, not in code, so every verdict in `checks`
can be explained from the row it was made with.

### 4.3 `tasks`

Written by the orchestrator with `horch ledger add`. The ticket file is the
contract; the row is the routing and state around it.

| field | type | used for |
|---|---|---|
| `task_id` | TEXT PK | key. Short form `T-<n>` per run for humans (`seq`). |
| `run_id` | TEXT FK | which run |
| `seq` | INTEGER | `T-1`, `T-2` ... in the order added |
| `title` | TEXT | 1 line, for `ledger status` |
| `task_file` | TEXT | path of the ticket the spec/ticket writer made, e.g. `ai_docs/tickets/x/03-parser.md`. The claiming worker reads this file and nothing else for its instructions. |
| `primary_persona` | TEXT | teammate name, e.g. `sonnet-feature`. **Only a worker of this persona may claim the task.** |
| `fallback_persona` | TEXT NULL | the teammate to use when the primary cannot run (quota refused, harness missing). |
| `claimable_by` | TEXT | the persona allowed to claim **now**. Starts as `primary_persona`. The orchestrator switches it to `fallback_persona` with `ledger fallback <task> "<reason>"`; nobody else changes it. |
| `fallback_reason` | TEXT NULL | why `claimable_by` moved to the fallback |
| `phase` | TEXT NULL | `research`, `plan`, `implementation`, `validation`; passed to the spawn |
| `parallel` | INTEGER 0/1 | 1: may run at the same time as other tasks. 0: runs alone - no other task of the run may be `running` while it runs. |
| `priority` | INTEGER | lower runs first among claimable tasks. Default 100. A requeued task gets its old priority minus 1000, so a retry goes first. |
| `wave` | INTEGER NULL | copied from `MAP.md`, for display only. Order comes from `task_deps`. |
| `worktree_mode` | TEXT | `shared` (single-branch mode) or `worktree` (own branch and worktree) |
| `land_check` | TEXT NULL | the gate command `ledger land` runs before it fast-forwards, e.g. `cargo test -p horch-core` |
| `status` | TEXT | see below |
| `attempts` | INTEGER | how many `workers` rows have claimed it. Starts 0. |
| `current_worker_id` | TEXT NULL FK | the worker that holds it now |
| `summary` | TEXT NULL | the final `done` summary of the attempt that finished it |
| `created_at`, `updated_at`, `finished_at` | TEXT | audit |

`tasks.status`:

| value | meaning | set by |
|---|---|---|
| `queued` | waiting for a worker | `ledger add`, reap |
| `running` | a worker claimed it | `ledger claim` |
| `landing` | the worker ran `ledger land`; the gate is running | `ledger land` |
| `done` | landed (or no worktree), worktree cleaned, summary in | `ledger done` |
| `cancelled` | the orchestrator dropped it | `ledger cancel` |

"Blocked" is not stored. A task is **claimable** when all of these hold:
`status = queued`, every `task_deps.blocked_by` task is `done`, it is not
`parallel = 0` with another task running, no running task is `parallel = 0`,
and no running task owns a file this task owns (`task_files`).

### 4.4 `task_deps`

| field | type | used for |
|---|---|---|
| `task_id` | TEXT FK | the task that waits |
| `blocked_by` | TEXT FK | the task it waits for |

PK (`task_id`, `blocked_by`). `ledger add` refuses a cycle.

### 4.5 `task_files`

| field | type | used for |
|---|---|---|
| `task_id` | TEXT FK | the task |
| `path` | TEXT | repo-relative file or directory |
| `mode` | TEXT | `own` or `do_not_touch` |

Used 2 ways: the claim refuses a task whose `own` files a running task
already owns (so 2 workers never edit 1 file), and `ledger plan-workers`
counts how many tasks can run at once.

### 4.6 `workers`

1 row per worker process life. Also used for the orchestrator itself.

**Identity (written at spawn and start, never changed after):**

| field | type | used for |
|---|---|---|
| `worker_id` | TEXT PK | key; also passed to the agent as `HORCH_WORKER_ID` |
| `run_id` | TEXT FK | which run |
| `kind` | TEXT | `worker` or `orchestrator` |
| `persona` | TEXT | teammate name. The claim only matches tasks whose `claimable_by` equals this. |
| `role` | TEXT | herdr role, e.g. `sonnet-feature-3`; the `horch tell` address |
| `harness`, `model`, `effort` | TEXT | what actually ran (after routing) |
| `host` | TEXT | hostname. A PID only means something on this host; `stop` refuses on another host. |
| `workspace_id`, `pane_id` | TEXT | the herdr pane (public id, e.g. `w19:p3`). Used only to close that 1 pane. |
| `supervisor_pid` | INTEGER | PID of `horch worker`, the process the pane runs. Written by `horch worker` at start. |
| `supervisor_started` | INTEGER | its OS start time (`procid::start_time`). With the PID, names exactly 1 process. |
| `agent_pid` | INTEGER NULL | PID of the agent CLI child (`claude`, `codex` ...). Written by `horch worker` right after it starts the child. |
| `agent_started` | INTEGER NULL | the agent's start time |
| `previous_worker_id` | TEXT NULL FK | the dead worker this one replaces (same task). The claim uses it to hand over transcript and worktree. |
| `resumed_from_session` | TEXT NULL | set when spawned with `--resume`; the session it continues |

**Session (written by the worker):**

| field | type | used for |
|---|---|---|
| `session_id` | TEXT NULL | the agent's session id. Written by `horch worker` as soon as it is known (minted for Claude and pi, discovered for Codex and others). Used for `horch spawn --resume`. |
| `transcript_path` | TEXT NULL | the session's transcript file (e.g. `~/.claude/projects/.../<session>.jsonl`, a Codex rollout). Read by the watchdog for growth, and handed to a replacement worker. |

**Liveness (written all the time):**

| field | type | used for |
|---|---|---|
| `last_checkin_at` | TEXT | **heartbeat.** `horch worker` writes "now" every `heartbeat_every_s` while its agent child runs. It proves the process lives. It does not prove progress. |
| `last_progress_at` | TEXT NULL | the last time progress was seen: the agent's own `ledger progress`/`note` call, or a proxy checkin the watchdog wrote for it (section 5.3). |
| `last_progress_source` | TEXT NULL | `agent`, `transcript`, `herdr_state`, `screen_timer` - why `last_progress_at` moved |
| `stall_rounds` | INTEGER | checks in a row with no progress. Reset to 0 on any progress. At `runs.stall_rounds` the worker is dead. |

**Task and outcome:**

| field | type | used for |
|---|---|---|
| `task_id` | TEXT NULL FK | the task this worker claimed. NULL until claim, and NULL for a worker that found none. |
| `attempt` | INTEGER NULL | `tasks.attempts` at claim time: 1 for the first try, 2 for the first replacement ... |
| `status` | TEXT | see below |
| `exit_code` | INTEGER NULL | the agent's exit code |
| `end_reason` | TEXT NULL | 1 line: `done`, `no task for persona sonnet-feature`, `dead: 3 rounds without progress`, `stopped by orchestrator: <reason>` ... |
| `summary` | TEXT NULL | the `ledger done` summary, or the reaper's note for an incomplete attempt |
| `spawned_at`, `started_at`, `claimed_at`, `ended_at` | TEXT | audit, and the watchdog's grace period after spawn |

`workers.status`:

| value | meaning |
|---|---|
| `spawning` | spawn wrote the row; the pane is opening |
| `starting` | `horch worker` runs and wrote its PID; the agent is starting |
| `claiming` | the agent is up and looks for a task |
| `working` | it holds a task |
| `done` | finished its task, ran `ledger done`, exited |
| `no_task` | found nothing to claim, told the orchestrator, exited |
| `incomplete` | died or was declared dead while it held a task; the task was requeued |
| `stopped` | the orchestrator stopped it on purpose (`ledger stop`) |
| `launch_failed` | never got as far as `starting` |

A partial unique index allows at most 1 `working` row per `task_id`.

### 4.7 `checks`

1 row per progress check of 1 working worker, every `check_every_s`.
This is the evidence trail behind "this worker is dead".

| field | type | used for |
|---|---|---|
| `check_id` | INTEGER PK | key, in order |
| `worker_id` | TEXT FK | who was checked |
| `at` | TEXT | when |
| `elapsed_s` | INTEGER | seconds since this worker's previous check |
| `checkin_age_s` | INTEGER | now minus `last_checkin_at` |
| `pid_alive` | INTEGER 0/1 | `procid::alive(agent_pid, agent_started)` |
| `herdr_status` | TEXT | herdr's agent status: `working`, `idle`, `blocked`, `done`, `unknown` |
| `herdr_seq` | INTEGER NULL | herdr's `state_change_seq` for the pane. A change since the last check is progress. |
| `transcript_bytes` | INTEGER NULL | size of `transcript_path` |
| `transcript_mtime` | TEXT NULL | its last write |
| `screen_timer_s` | INTEGER NULL | the "thinking / working for Nm" timer read from the pane, in seconds, if the harness shows one |
| `timer_delta_s` | INTEGER NULL | `screen_timer_s` minus the previous check's value |
| `verdict` | TEXT | `progress`, `proxy_checkin`, `suspect`, `no_progress`, `dead` |
| `round` | INTEGER | `workers.stall_rounds` after this check |
| `note` | TEXT | 1 line: which signal decided |

### 4.8 `stops`

Every signal and pane close goes through `horch ledger stop`, and every one
leaves a row. There is no other kill path (section 7).

| field | type | used for |
|---|---|---|
| `stop_id` | INTEGER PK | key |
| `worker_id` | TEXT FK | the worker stopped |
| `requested_by` | TEXT | `orchestrator`, `watchdog`, `self` (a `no_task` worker) |
| `reason` | TEXT | why |
| `target` | TEXT | `agent`, `supervisor` or `pane` |
| `pid` | INTEGER NULL | the exact PID signalled (from the `workers` row, never typed in) |
| `pid_started` | INTEGER NULL | the start time it had to match |
| `signal` | TEXT NULL | `TERM` or `KILL`; NULL for a pane close |
| `pane_id` | TEXT NULL | the exact pane closed |
| `result` | TEXT | `sent`, `already_gone`, `not_same_process` (the PID now belongs to another program: nothing sent), `refused_other_host`, `pane_closed`, `error` |
| `at` | TEXT | when |

### 4.9 `worktrees`

Replaces the JSON claim registry (`<state>/worktrees/<repo-hash>.json`).
A row lives from `git worktree add` to the moment the folder is gone from
disk.

| field | type | used for |
|---|---|---|
| `worktree_id` | TEXT PK | key |
| `task_id` | TEXT FK | the task it serves. Survives a worker's death. |
| `worker_id` | TEXT FK | the worker that holds it now. A replacement worker takes it over. |
| `repo_root` | TEXT | main checkout |
| `path` | TEXT UNIQUE | `<repo>/.worktrees/<task-seq>-<name>`. The only path cleanup will ever delete. |
| `branch` | TEXT | the unit branch |
| `base` | TEXT | the branch it lands on |
| `base_commit` | TEXT | where it started |
| `status` | TEXT | `open`, `landing`, `landed`, `cleanup_failed`, `cleaned`, `unlanded`, `abandoned` |
| `landed_commit` | TEXT NULL | the base tip after the fast-forward |
| `check_output_path` | TEXT NULL | the gate's log, for a red gate |
| `bytes_on_disk` | INTEGER NULL | measured just before removal |
| `created_at`, `landed_at`, `cleaned_at` | TEXT | audit |
| `reason` | TEXT NULL | why `unlanded`, `cleanup_failed` or `abandoned` |

### 4.10 `events`

Append-only history: the readable story of a run. Messages still travel
by `horch tell`, but a question, a block and an answer are also written
here, so `ledger status` shows them.

| field | type | used for |
|---|---|---|
| `event_id` | INTEGER PK | order |
| `at` | TEXT | when |
| `run_id`, `task_id`, `worker_id` | TEXT NULL | what it is about |
| `actor` | TEXT | `orchestrator`, `worker`, `supervisor`, `watchdog`, `horch` |
| `kind` | TEXT | `added`, `claimed`, `no_task`, `progress`, `note`, `question`, `blocked`, `answered`, `fallback`, `land_started`, `land_red`, `landed`, `cleaned`, `cleanup_failed`, `done`, `reaped`, `requeued`, `stopped`, `session`, `cancelled` |
| `text` | TEXT | 1 STE sentence or more |

## 5. How the fields are used: the flows

### 5.1 Queue and spawn (orchestrator)

1. The orchestrator runs `horch ledger add` once per ticket: `task_file`,
   `primary_persona`, `fallback_persona`, `parallel`, `blocked_by`,
   `own`/`do_not_touch` files, `land_check`.
2. It runs `horch ledger plan-workers`. For each persona that prints the
   number of tasks claimable now, minus the live workers of that persona in
   `spawning`, `starting` or `claiming`. That is how many to spawn.
3. It spawns that many: `horch spawn <persona>` with no task text. The task
   text the worker gets is fixed: "Run horch ledger claim."
4. Quota: `horch quota` and a REFUSED/NOTE line from spawn are warnings.
   The orchestrator tells the operator that a pool is low, then goes on
   unless the operator says stop. If the primary truly cannot start (the
   harness is missing, spawn refuses), the orchestrator runs
   `horch ledger fallback <task> "<reason>"` and spawns the fallback.

### 5.2 Claim (worker)

1. `horch worker` writes `supervisor_pid`, `supervisor_started`, status
   `starting`, then starts the agent and writes `agent_pid`,
   `agent_started`. It starts the heartbeat thread (`last_checkin_at`).
2. As soon as the session id is known, it writes `session_id` and
   `transcript_path`.
3. The agent runs `horch ledger claim`. In 1 `BEGIN IMMEDIATE` transaction
   it picks the claimable task with the lowest `priority`, then the lowest
   `seq`, where `claimable_by = workers.persona`. It sets `tasks.status =
   running`, `current_worker_id`, `attempts + 1`, and on the worker
   `task_id`, `attempt`, `status = working`, `claimed_at`.
4. The claim prints: the `task_file` to read, the `own` and `do_not_touch`
   files, the worktree to use, and for a retry the previous attempts:
   their `summary`, `transcript_path` and the worktree they left.
5. **No task:** the claim sets `status = no_task` and prints the 1 line to
   send. The agent sends
   `horch tell orchestrator "[<role>] NOTE: No task for <persona>. I exit. PID <supervisor_pid>."`
   and ends its turn. `horch worker` sees the agent exit and closes its own
   pane by its exact `pane_id`. A `stops` row with `requested_by = self`
   records it.

### 5.3 Heartbeat and the progress check

2 different signals:

- `last_checkin_at` - written by `horch worker` every 30 s. The agent cannot
  write it: an agent in a long thinking turn runs no command. So it proves
  the process lives, not that it makes progress.
- `last_progress_at` - moved by the agent's own `ledger progress` and
  `ledger note`, or by the watchdog when it sees progress for the agent.

A horch watchdog process (1 per run, single-instance by lock, like the mail
courier) checks each `working` worker every `check_every_s` and writes 1
`checks` row. It decides like this:

1. **Process gone.** `pid_alive = 0`, or `checkin_age_s > heartbeat_stale_s`
   and the supervisor PID is gone: verdict `dead`. Go to 5.4 at once - a
   process that does not exist cannot resume.
2. **Agent progress.** `last_progress_at` moved since the last check:
   verdict `progress`, `stall_rounds = 0`.
3. **Proxy checkin.** The agent made no ledger call, but 1 of these moved:
   - `transcript_bytes` grew or `transcript_mtime` is newer;
   - `herdr_seq` changed;
   - the screen timer ran the whole interval:
     `timer_delta_s >= elapsed_s - timer_tolerance_s`
     (example: 10m then 20m, 10 min apart).
   Verdict `proxy_checkin`: the watchdog writes `last_progress_at = now`
   for the worker with the source, and `stall_rounds = 0`.
4. **Suspect.** The timer moved less than the clock
   (example: 10m then 15m, 10 min apart: it stopped or restarted about
   5 min ago), or herdr says `idle` while the task is `running`. The
   watchdog verifies at once: PID alive, herdr status, transcript mtime.
   If the transcript was written after the timer stopped, or herdr shows
   `working`, it is `proxy_checkin`. Else verdict `no_progress`.
5. **Nothing moved:** verdict `no_progress`, `stall_rounds + 1`. On the
   first `no_progress` the watchdog tells the orchestrator, and the
   orchestrator asks the worker for an update with `horch tell`.
6. At `stall_rounds = runs.stall_rounds` (3 checks, 30 min with the
   defaults): verdict `dead`. Go to 5.4.

The watchdog tells the orchestrator only when a verdict changes, so a
healthy fleet makes no noise.

### 5.4 Reap and replace a dead worker

`horch ledger reap <worker>` (the watchdog runs it on `dead`; the
orchestrator can run it too). In this order:

1. `ledger stop` on the worker: TERM to `agent_pid`, wait 10 s, KILL if it
   still runs with the same start time; the same for `supervisor_pid`; then
   close `pane_id` in herdr. Each step writes a `stops` row.
2. The worker row: `status = incomplete`, `end_reason`, `ended_at`.
3. The task row: `status = queued`, `current_worker_id = NULL`,
   `priority - 1000`. `claimable_by` stays.
4. Its worktree row stays `open` and keeps the branch and any uncommitted
   files. It is not cleaned.
5. An `events` row `reaped` and `requeued`.
6. The orchestrator spawns 1 new worker of `claimable_by`. The spawn sets
   `previous_worker_id` on the new row. Its claim takes that task first and
   prints the previous `transcript_path`, `summary` and worktree, with the
   instruction: use what you can of the previous work; read the transcript
   to learn what went wrong before you continue. `worktrees.worker_id` moves
   to the new worker.

The new worker gets its own `worker_id`, PIDs, session id and transcript.
The old row is never reused.

### 5.5 Land and clean up (worker)

1. Worktree mode: `horch ledger worktree new` creates the worktree and the
   row (`open`). Shared mode has no row.
2. When the work is complete the worker runs `horch ledger land`. It sets
   `landing`, rebases on `base`, runs `land_check`, fast-forwards `base`,
   writes `landed_commit`, `landed_at`, status `landed`.
   (This is the current `Landing::land`, driven from the ledger.)
3. In the same command: measure `bytes_on_disk`, run
   `git worktree remove <path>` and `git branch -d <branch>`, then confirm
   that `path` no longer exists. Status `cleaned`, `cleaned_at`.
4. If removal fails: status `cleanup_failed` with `reason`. The land is
   still good. The watchdog retries cleanup every check until `cleaned`.
   Today a failure here is silent and the folder stays on disk.
5. A red gate: status back to `open`, `check_output_path` set, an
   `events` row `land_red`. The worker fixes and lands again.
6. `horch ledger done "<summary>"` refuses while the task has a worktree
   that is not `cleaned` - unless `--unlanded "<reason>"`, which sets
   `unlanded` and tells the orchestrator.
7. Cleanup only ever deletes a `path` that is in this table. It never
   globs `.worktrees/*`.

### 5.6 Done and "is the run done?"

- `ledger done` writes `tasks.summary`, `status = done`, `finished_at`;
  `workers.summary`, `status = done`; an `events` row; then tells the
  orchestrator `[<role>] DONE: ...` as today. `horch worker` then closes
  its exact pane.
- `horch ledger status` prints each task with its state, worker and age,
  and exits 0 only when every task is `done` or `cancelled` and every
  worktree is `cleaned` or `unlanded`. The orchestrator uses that, not its
  memory, to decide the run is finished.

## 6. The CLI

All under `horch ledger`. `--json` on every read command.

| command | who | does |
|---|---|---|
| `add --file <ticket> --primary <p> [--fallback <p>] [--parallel\|--exclusive] [--blocked-by T-n ...] [--own <path> ...] [--no-touch <path> ...] [--check "<cmd>"] [--phase <ph>]` | orchestrator | queue 1 task |
| `plan-workers` | orchestrator | per persona: how many workers to spawn now |
| `fallback <task> "<reason>"` | orchestrator | let the fallback persona claim it |
| `cancel <task> "<reason>"` | orchestrator | drop a queued or running task |
| `status [--run <id>]` | anyone | the board; exit 0 only when all work is done |
| `show <task\|worker>` | anyone | 1 row with its events, checks, stops |
| `claim` | worker | take the next matching task, or record `no_task` |
| `progress "<text>"` | worker | a milestone; moves `last_progress_at` |
| `note "<text>"` | worker | same as today's `horch note` |
| `worktree new <name>` | worker | create the worktree and its row |
| `land` | worker | gate, fast-forward, remove the worktree, verify it is gone |
| `done "<summary>" [--unlanded "<reason>"]` | worker | finish the task |
| `checkin` | `horch worker` (internal) | write `last_checkin_at = now` |
| `watch` | the watchdog (internal) | the check loop of 5.3 |
| `reap <worker>` | watchdog, orchestrator | 5.4 |
| `stop <worker> "<reason>"` | orchestrator | stop 1 worker by its recorded PIDs and pane |

`horch spawn`, `horch tell` and `horch inbox` stay. `horch assign` is
retired: assignment is the claim.

## 7. Deterministic stop

- `horch ledger stop` is the only code that sends a signal to a worker or
  closes a worker pane.
- It takes a `worker_id`. It never takes a PID, a name, a pattern or text
  from a transcript. It reads the PID and start time from the `workers` row.
- Each signal goes to 1 PID through `procid::signal_same(pid, started, sig)`.
  On Linux it opens a pidfd, then checks the start time, so a reused PID is
  never hit. A mismatch sends nothing and records `not_same_process`.
- The pane close uses the exact `pane_id` from the row. Closing the pane
  ends any processes the agent started under it.
- A worker on another `host` is refused.
- Briefings forbid `pkill`, `killall`, `kill` with a pattern, and
  `taskkill /IM`. A unit test fails the build if horch's source or a
  briefing contains them.

## 8. What must be built (beyond the tables)

1. The SQLite store (`rusqlite`, bundled), migrations, WAL setup.
2. `horch ledger` commands of section 6.
3. `horch worker`: record both PIDs, the heartbeat thread, the session id
   and transcript path as soon as known.
4. The watchdog process and the progress rules of 5.3.
5. A screen-timer reader per harness (Codex, Claude). No code reads these
   timers today and their text changes between releases, so it is the
   last signal, after transcript growth and `herdr_seq`.
6. Windows process identity: `procid` has no start time on Windows. Use
   the process creation time (`GetProcessTimes`) and `TerminateProcess` on
   a handle opened once, so Windows stops are as exact as Linux.
7. Ledger-driven land and verified cleanup (5.5), retry of
   `cleanup_failed`.
8. The briefings: `fleet-orchestrator.md`, `fleet-worker.md` and the
   `orchestrate` skill move from "assign and tell" to "add, spawn, claim".
9. The ban test of section 7.

## 9. Open questions

1. **Migration.** Keep today's JSON session ledger and worktree registry
   read-only for old records, or import them into SQLite once?
2. **A dead PID.** Section 5.3 step 1 reaps at once when the process is
   gone, without 3 rounds. The 3 rounds apply only to a process that lives
   but makes no progress. Is that what you want?
3. **Grandchildren.** An agent starts child processes (tool commands, MCP
   servers). The plan stops them by closing the exact pane, not by a
   process-group signal. Is a pane close acceptable for these, or do you
   want each child PID recorded too?
4. **The orchestrator's own row.** Should the orchestrator heartbeat and be
   watched too, so a dead orchestrator is visible to the operator?
5. **Transcript size.** A transcript can be many MB. Give the replacement
   worker the path only, or a trimmed copy (last N turns plus every error)?
