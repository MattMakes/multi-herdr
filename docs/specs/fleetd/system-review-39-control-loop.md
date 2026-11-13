# System review 39: control loop and crash safety

Scope: `docs/specs/fleetd/design.md` at `043cc279b99c951bdadf6adf5211df96eca6f156`, with the earlier design reviews, amendments, and preservation rows used only to test this lens.

## Verdict

NOT SOUND

The design puts gate, git, process, and file side effects inside a transaction-shaped pass but does not define a crash-consistent effect protocol. It also lacks a daemon singleton and a durable command-request protocol, so the stated restart and sole-writer guarantees do not hold. The level-triggered scans and worktree rescue rules are strong, but they do not close these control-plane gaps.

## Answers

### 1. Long work inside one pass

Yes. The verifier is explicitly an async job at lines 1003-1003, but `land_bundles` and `integrate_features` run inside the transaction at lines 1006-1007, while the fast gate runs on the candidate at lines 901-901 and the full gate runs before feature readiness at lines 931-931. The same design preserves checks up to 24 h at lines 1139-1139 and git network calls up to 10 min at lines 1142-1142, while the heartbeat is written only after the full pass at lines 1010-1011 and PL-5 restarts after 30 s of awake time at lines 180-180. Worktree creation and rescue also run git and filesystem operations at lines 830-830 and 850-861; git fetch has bounds at lines 1097-1097 and credentials at lines 1177-1177, but its scheduling is silent.

### 2. Side effects and crash windows

The pass can start or stop a process, write `task.yaml`, `START.md`, and verdict files, create, rescue, or remove a worktree, create or move branches, merge or rebase, run gates, and write the final heartbeat at lines 986-1011. A crash after an effect but before the SQLite commit leaves a process, file, directory, or git ref that the rolled-back ledger does not own; process, file, worktree, and branch scans can detect parts of this at lines 647-660, 841-846, 915-922, and 987-1008, but the design does not define repair for immutable attempt files, duplicate launch, or a ref moved under rolled-back landing state. A commit-before-effect implementation has the opposite gap because there is no durable effect outbox or intent state, so the ledger can claim an action that never occurred. The reserved worktree case is the exception: lines 830-846 explicitly detect a row without a directory and a directory without a row.

### 3. More than one fleetd

The service manager, `horch fleetd check`, every `horch` command, and `horch self-update` can all cause a start or restart at lines 260-260, 288-288, and 1255-1255. The platform layer supports file locks and `/fleet/run/` contains an unnamed lock at lines 268-268 and 349-349, but the design never says fleetd acquires it, which identity owns it, or how takeover works. Thus the singleton is only a convention as written; 2 daemons can both scan the same ready state and perform external effects, while SQLite only serializes or rejects their ledger writes.

### 4. Command requests

When fleetd is down, the command writes a request file that fleetd reads at start, as stated at lines 292-292. The design is silent on acknowledgment, deletion, claiming, result delivery, and the crash point after read but before commit. It is also silent on request identity and duplicate submission; the generic `ingested(path, sha256)` table at lines 638-640 is not connected to command requests, so it does not specify same-request deduplication or response replay.

### 5. Level-triggered facts

Processes and panes are resampled each pass at lines 987-989, files get a full scan at lines 990-990, and file or herdr events only accelerate that work at lines 210-210 and 304-304. Worktree directories are scanned each pass, `git worktree list` every 60 s, and git invariants each pass at lines 841-846, 915-922, and 991-1008; leases are recomputed from the ledger, process state, heartbeat files, and persisted activity at lines 958-969. Two facts remain event-only or silent: sleep and wake are only said to be reported by the platform at line 969, with no restart reconstruction, and remote git changes have no fetch cadence even though feature merge is read from git at lines 927-934.

### 6. Clock and sleep

Lease and stall time explicitly exclude machine sleep at lines 969-969, and the hang check uses awake time at lines 180-180 and 288-288. The 2 min stall-ladder delay is silent at lines 975-980, question `due_at` is silent at lines 618-627, and branch `due_at` is silent at lines 588-599. The 90 s start timeout, retry backoffs, 1 h interactive wait, 30 min idle stop, 1 h reminders, 7 day park, and `max_minutes` wall-clock guard also do not define sleep behavior at lines 695-710 and 796-798.

### 7. Back-pressure

No mechanism limits pass work. Dispatch has no cap, all live attempts are visited, outboxes and worktree directories are fully scanned, and every invariant runs at lines 987-1009; a moved tip also requests trial merges against the primary branch and every other in-flight feature at lines 926-931. With 200 tasks and 200 worktrees, pass cost grows with all of them, and 200 in-flight features can require 19,900 pairwise trial merges after a relevant tip move. Health floors pause new dispatch at lines 716-716, but there is no per-pass deadline, durable cursor, batch size, or worker-job queue that protects the 30 s heartbeat.

## Findings

### S39-1 - CRITICAL - Long gates can make the watchdog restart a healthy daemon forever

Lines: 180, 901, 931-932, 1006-1011, 1139, 1142.

> `land_bundles(tx)                      # ready task branches -> 1 bundle per feature, fast gate, fast-forward`
> `integrate_features(tx)                # trial merges, stacks, rebases, resolver tasks, full gate`

Failure scenario: A fast or full gate takes more than 30 s. The full gate includes end-to-end tests, the design gives no 30 s gate bound, and task checks can run for 20 min by default or up to 24 h. The heartbeat and watchdog ping do not occur until the pass finishes. The service manager kills fleetd, the restart sees the same due gate, and the fleet repeats the same restart without making progress.

Smallest change: Move every potentially blocking effect to a bounded asynchronous job with a durable job row. Keep the 5 s pass to enqueue, observe, and commit job results, and make the watchdog heartbeat independent of job duration.

### S39-2 - CRITICAL - One SQLite transaction cannot make the pass side effects crash-atomic

Lines: 986-1011, 1223-1223.

> `with ledger.tx() as tx:`
> `dispatch(tx, health(), pacing(tx))    # lease worktree, write task.yaml + START.md, launch; no cap`

Failure scenario: fleetd writes immutable attempt files and launches a worker, then crashes before the ledger commit. SQLite rolls back the attempt, but the files and process remain. On restart, dispatch runs before `stop_orphans`, can reuse the rolled-back attempt identity, and can launch another process into the same worktree before the orphan is stopped; the workers can race on files and corrupt paid-for work. Committing first only reverses the gap: a crash can leave ledger state that claims an effect that never occurred.

Smallest change: Add a durable intent and effect state machine for every external action. Commit an idempotency key and intended action first, execute outside the transaction, then commit observed completion; reconcile incomplete intents before new dispatch.

### S39-3 - HIGH - The sole-writer rule has no daemon singleton mechanism

Lines: 78, 260, 268, 288, 349, 1255.

> `| Ledger | SQLite in WAL mode, one writing process | Simple, durable, inspectable with any SQLite tool. |`

Failure scenario: `horch fleetd check` starts a replacement while the old process is slow but still alive, or `horch self-update` starts the new binary before the old one exits. Both daemons observe ready work and perform process or git effects. One ledger transaction then blocks or loses its race, but both external effects remain, which can produce duplicate workers, competing branch moves, or repeated cleanup.

Smallest change: Specify one cross-process singleton lease through the platform layer, including owner identity, fencing generation, atomic acquisition, stale-owner proof, and handoff during update. Require every mutating effect to carry and verify the current fencing generation.

### S39-4 - HIGH - Command requests have no durable claim, acknowledgment, or deduplication contract

Lines: 292, 638-640, 1265.

> `When fleetd is down, the command writes a request file, and fleetd reads it at start.`

Failure scenario: fleetd reads a queued `cancel`, `approve`, or `requeue` request and crashes before its ledger commit. If it removed or marked the file first, the operator's command is lost; if it leaves the file, a non-idempotent command can run twice after restart. A client retry can create the same ambiguity even without a crash, and the operator cannot tell whether the command took effect.

Smallest change: Give each request a client-generated request ID, immutable payload, authenticated caller, durable pending, applied, and replied states, and a replayable result. Commit command effect and request completion in one SQLite transaction, and delete or archive the file only after acknowledgment is durable.

### S39-5 - MEDIUM - Remote git state is not level-triggered

Lines: 927-934, 1007-1008, 1177.

> `A feature is merged when its changes are in the primary branch. fleetd reads this from git, not from the GitHub API.`

Failure scenario: The operator merges a pull request remotely while no local command fetches. The local primary ref never moves, so fleetd leaves the feature and its ideas in integration, does not rebase other features onto the real primary, and can present gate evidence against a stale base indefinitely.

Smallest change: Define a bounded periodic fetch job per repository, record the fetched remote tip, and make merge detection and gate freshness depend on that observed tip. A failed fetch must leave visible stale-since state.

### S39-6 - MEDIUM - Several deadlines do not define sleep behavior

Lines: 588-599, 618-627, 969, 975-980.

> `due_at      TEXT NOT NULL,               -- past it, the row escalates`

Failure scenario: A laptop sleeps before a branch or worker-question deadline and wakes after it. The next pass immediately escalates both, even though no actor could make progress while the machine slept; the 2 min stall rung and retry backoff can also fire immediately after wake. The operator sees false escalations and avoidable attempt churn.

Smallest change: Define one awake-time clock abstraction for operational timers and list each timer that deliberately uses civil time. Store remaining awake duration or a monotonic awake-time deadline instead of an unexplained wall-clock `due_at`.

### S39-7 - LOW decision - D8 scale has no per-pass work budget

Lines: 38, 716, 926-931, 987-1009.

> `There is no cap on features, sub-orchestrators or workers.`

Failure scenario: With 200 active features, a primary-tip move makes the design request 19,900 pairwise feature trial merges, in addition to full scans and invariant checks. The pass exceeds 30 s, the hang detector restarts fleetd, and the same backlog is reconstructed. This problem follows the operator's D8 no-cap decision, so the rank is LOW and the decision itself is not challenged.

Smallest change: Keep D8, but add durable cursors, per-pass item and time budgets, fair queues, cached pair results keyed by tips, and a separate bounded job pool. A pass must publish backlog age and finish before the heartbeat deadline.

### S39-8 - HIGH - The command channel does not define local caller or spool authorization

Lines: 292, 314-363, 1103, 1201.

> `Each horch command that changes state sends a request to fleetd over its channel`

Failure scenario: An implementation creates the socket, named pipe, or down-daemon request directory with permissive defaults because the design gives no owner or mode rule for them. Another local user submits a valid `approve`, `cancel`, or direct-intake request. fleetd accepts it as operator input, so unapproved agent work can start or authorized work can be stopped.

Smallest change: Require peer-user verification on sockets and named pipes, mode 0700 for request directories, mode 0600 for files, owner checks before read, and an explicit operator identity in each request. Apply the existing M5 local-injection rule to the command channel.

## Checked and sound

- Lines 358-361 give protocol files atomic temp-file, fsync, rename, schema, and single-writer rules.
- Lines 210-210 and 987-990 make herdr and file notifications accelerators, then resample panes and outboxes.
- Lines 830-846 define both worktree crash directions and reconcile a reserved row or an unowned directory.
- Lines 850-861 preserve dirty and untracked work in a rescue ref and verify it before removal.
- Lines 1003-1003 explicitly make task verification asynchronous, so the acceptance timeout need not block the pass.
- Lines 655-660 and 915-922 compare branch claims with git and preserve rescue refs before destructive branch movement.
- Lines 969-969 and 288-288 correctly exclude machine sleep from leases, stall time, and the daemon hang threshold.
