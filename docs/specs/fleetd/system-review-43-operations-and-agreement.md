# System review 43: operations, trust, and document agreement

## Verdict

NOT SOUND

The design has 1 CRITICAL contradiction: it requires one live sub-orchestrator attempt for every active idea and also exactly one sub-orchestrator per feature. It also has 1 HIGH trust-boundary gap because worker-controlled files have schemas but no required size, file-type, path-binding, or entry-count checks. The control-loop, worktree-rescue, verifier, event-cause, and branch-custody mechanisms are otherwise strong.

## Answers

### 1. Worker-written files that fleetd reads

The worker owns `attempts/<attempt>/outbox/`, which contains append-only `progress.jsonl`, immutable `result.json`, and one file per question; fleetd scans and ingests that outbox on each pass (lines 334-343, 360-362, 990-993, 1265). Every file type has a schema, but the design gives none of these files a size limit, entry-count limit, regular-file or no-symlink rule, or an explicit check that payload task and attempt identifiers match the attempt directory (lines 361-362, 460, 638-640). A wrong worker can claim a false result, park or redirect a task with a question, and keep a lease active with progress records; the verifier limits the false result because it reruns the committed acceptance commands, but the other control effects occur before verification (lines 646, 962, 967, 980, 1018). A hostile worker can place an oversized file or a symlink at a scanned name, so S43-1 records the resulting control-plane denial of service and cross-attempt ambiguity.

### 2. Acceptance command authority

The sub-orchestrator writes acceptance commands into `plan.yaml`, fleetd commits the validated plan, and fleetd copies the command into immutable `task.yaml` before launch (lines 262, 391-442, 444-461, 759, 769). A worker owns only its worktree and attempt outbox, so it cannot rewrite the command string or the committed plan after approval (lines 167, 264, 359-360, 698). It can change a script or test inside the worktree that the fixed command invokes, but the code-kind review and the feature gates provide the later independent checks (lines 265, 273, 901, 931).

### 3. Credential launch paths

The design preserves the blanket rule that every child, pane command, detached child, updater, and probe removes `ANTHROPIC_API_KEY`, and roster validation rejects it (lines 248, 1104, 1153-1154, 1171, 1199). Worker, sub-orchestrator, and judge launches use the profile adapter and wrapper path, so they are covered by that rule (lines 262-264, 271-284, 725-738). The verifier's acceptance-command launcher, docs-sync `version_cmd`, and real conformance launches are not explicitly connected to the scrubbed launch flow (lines 265, 1035-1053, 1065-1077); native herdr pane resume is explicitly an unscrubbed launch path and gets only a warning (line 1105). The declarative rule is therefore complete, but the executable wiring is not, as recorded in S43-3.

### 4. Bounds with no parallel-worker cap

Disk has a 20 GB worktree budget and a free-disk health floor; fleetd cleans finished trees, pauses dispatch, and reports the condition (lines 191, 584, 716, 865). Machine load has a health floor, while memory is probed but has no dispatch floor; open files, per-repository git mutation pressure, and SQLite write rate have no bound or admission reservation (lines 268, 309, 716, 926-929, 997). The single SQLite writer and git locks prevent corruption, but they do not bound queueing or resource use (lines 154, 495-497, 645, 907-911). Only the disk path explicitly says that status tells the operator why dispatch paused, so S43-4 is a LOW decision gap under D8.

### 5. Observability from status and the ledger

Branch ownership and the last hour of state changes are answerable because branches carry `owner` and events carry time and cause (lines 588-600, 630-635, 188). `horch status` is required to show ideas, tasks, leases, workers, and last signals, but it is not required to show health, pacing, profile-health, gate, or lock reasons that leave a ready task undispatched (lines 187, 716, 865, 997). Operator waits are represented in idea and question rows and in `needs-you/`, but status is not required to render that one list, so the four questions are only partially answerable (lines 525-527, 618-627, 189, 347).

### 6. Lifecycle of the needs-you list

Approvals, direct-plan refusals, parked ideas, escalated questions or tasks, unhealthy profiles, and conformance drift put items into the single `needs-you/` directory (lines 347, 759, 794-796, 816, 978-980, 1061). The design does not define acknowledgement, resolution, replacement, deletion, retention, or a maximum item count; parking an idea after 7 days adds another note but does not remove the old item (line 796). S43-6 records the stale and unbounded-list failure.

### 7. Agreement inside the design

Invariant I7 requires one live sub-orchestrator attempt for every active idea, while I13 and the Ideas section require one per feature even when that feature has several ideas in progress (lines 654, 660, 777-781, 884). The file-protocol introduction says every hand-off file is never edited after landing, while `progress.jsonl` is appended and `heartbeat.json` is rewritten every 10 seconds (lines 314, 338, 478-490, 961). The figure says the idea machine has 8 states although the table and schema name 10 including `parked` and `cancelled`, and G3 says every task has only done or escalated outcomes while the task schema also has terminal `cancelled` (lines 60, 519-544, 763-775).

### 8. Agreement with the DDD model

All 10 searched DDD rules are silent rather than contradicted in the design: request files have no `idempotency_key`; usage-limit input has no optional `reset_at`; question `due_at` has no 15 minute value; supervision is not named as a separate operating phase; task-execution and integration are not named as a partnership; the design names one needs-you place but not Ideas ownership; wrapper ownership by worker-profiles is absent; plan validation is not named as a domain-model module; docs sync is called a scheduled job rather than a `horch` subcommand; and conformance drift queues an unspecified idea rather than a `--direct` idea (lines 292, 347, 626, 676-738, 816, 1020-1065). Other DDD-only rules found are creation of the feature row at idea capture, refused command responses with a reason, the closed result statuses `completed|blocked|failed`, re-enabling a profile after conformance passes, the review-requeue counter rule, the unlimited operator wait after escalation, and an `arch_scan` boundary for the six contexts; the relevant design locations are silent (lines 133, 469, 511-530, 759, 1061, 1100). Design-only rules not carried into the DDD implementation artifacts are the 20 GB worktree budget and the `rescue_include` exception for ignored files (lines 861, 865). S43-8 captures the operationally material DDD drift.

### 9. Simplicity

First, remove `acceptance_self_check` from `result.json`: it is a worker claim and the verifier reruns the committed acceptance commands (lines 473, 982). Second, remove file-watch and herdr edge-event wakeups if implementation simplicity matters more than sub-5-second latency, because the fixed full scan already catches every missed event (lines 153, 210, 950, 990). Third, remove per-CLI Stop result hooks: wrapper exit repair, the interactive idle ladder, and verifier-only completion already close the same failure, although detection becomes slower for an interactive pane (lines 735, 975-982).

## Findings

### S43-1 - HIGH - Worker-controlled files lack resource and attempt-boundary checks

Lines: 338-362, 638-640, 990-993.

> Every file type has a JSON Schema in `/fleet/schemas/`. fleetd rejects a file that fails it and records why.
> Raw writes still work if they validate.

The schema rule does not require a byte limit, line or entry limit, regular-file check, no-follow open, canonical path check, or equality between payload identifiers and the attempt named by the path.

Failure scenario: a hostile worker writes a very large `progress.jsonl` or makes `result.json` a symlink to an unbounded readable source. The full outbox scan reads it with fleetd's authority, fleetd exhausts memory or blocks, the service manager restarts it, and the unchanged file repeats the failure. A second worker can also place schema-valid identifiers for another attempt unless the builder invents a cross-check.

Smallest change: specify per-type byte and entry limits, accept only regular files opened without following links, canonicalize under the exact attempt outbox, require payload task and attempt identifiers to equal the path owner, and quarantine a rejected file without rereading it every pass.

### S43-2 - CRITICAL - Per-idea and per-feature sub-orchestrator invariants cannot both hold

Lines: 654, 660, 777-781, 884.

> I7: ... every idea from `clarifying` to `integrating` has exactly one live sub-orchestrator attempt or is waiting on you.
> I13: a sub-orchestrator exists for each feature that is not merged and not abandoned.

The Ideas section also says the count is exactly one per feature and that tasks from several ideas in that feature can run in parallel.

Failure scenario: feature F has ideas I1 and I2 in progress. I7 requires two live idea attempts, while the per-feature rule permits one. One reconciliation branch starts the missing attempt and another stops the extra attempt on every pass, or an implementation violates one invariant permanently; questions and replans can then lose their owner.

Smallest change: make the live process invariant feature-scoped, define which idea phase that one process owns, and represent other active ideas as queued or worker-supervision state without their own live sub-orchestrator attempt.

### S43-3 - MEDIUM - New Claude-capable launchers are not wired to the environment scrub

Lines: 248, 265, 267, 1035-1077, 1104-1105, 1153-1154.

> `ANTHROPIC_API_KEY` is removed from every child process, every pane command line, every detached child, every updater and every probe.
> herdr's own pane resume uses an environment that horch did not scrub.

The global rule is sound, but the verifier launcher, docs-sync version command, and conformance real launch are not connected to `Harness::build_command` or `scrub_child_env`; native herdr resume is a known bypass.

Failure scenario: the parent service environment contains the forbidden name. A verifier check invokes `claude`, a conformance launch starts Claude directly, or herdr restores a pane; the child uses API billing instead of the subscription, and the operator sees only the later billing or warning.

Smallest change: require every process creation API to use one scrubbed platform launcher, name verifier, docs sync, conformance, and herdr restore in its architecture test, and refuse native herdr agent resume rather than only warning.

### S43-4 - LOW - Decision D8 leaves non-disk resources without dispatch guards

Lines: 21, 38, 191, 716, 779, 926-929, 997.

> There is no spend budget and no cap on parallel work.
> Health guards pause new dispatch only while free disk or machine load is past a safety floor.

This is a decision gap: D8 is final, so the finding does not propose a worker cap. The design has no dispatch guard for memory, open files, process count, SQLite write backlog, or per-repository git mutation backlog.

Failure scenario: a large ready set dispatches in one pass before load rises. The host reaches its file or memory limit, wrapper and git operations fail together, retries add more ledger and file traffic, and status does not name the admission reason because none exists.

Smallest change: keep unlimited configured parallelism but add resource reservations and dynamic health floors for memory, file descriptors, process count, SQLite backlog, and per-repository git mutations; show every active hold in status.

### S43-5 - MEDIUM - Status cannot explain dispatch holds

Lines: 187-192, 630-635, 716, 865, 997.

> `fleet status` shall show every idea, task, lease, worker and its last signal in one view.

The event ledger explains transitions, but health, pacing, profile-health, gate, and lock holds can leave a task in `ready` without any transition or required status field.

Failure scenario: a task stays ready because pool pacing or a project gate holds dispatch. No state changes, the event table has no new cause, and status shows the task and last signal but not the hold, so the operator cannot answer why it is not running.

Smallest change: add a derived `dispatch_hold` record or status section with typed causes, owning resource, first-seen time, and current evidence; append events when a global hold starts or clears.

### S43-6 - MEDIUM - The needs-you list has no removal or retention lifecycle

Lines: 189, 347, 759, 794-796, 816, 978-980, 1061.

> Anything that needs you shall land in one place (`/fleet/needs-you/`), in `horch ideas`, and as a herdr notification.

The design defines producers but no acknowledgement, resolution, replacement, deletion, retention, or bounded count.

Failure scenario: an approval is revised, a question is answered, and a profile becomes healthy, but their old files remain beside current requests. The operator acts on stale work or ignores the growing list, while repeated scans and notifications become slower and less useful.

Smallest change: give each item a stable key and lifecycle `open|resolved|superseded`, make producers upsert by key, render only open items by default, and define retention for terminal items.

### S43-7 - MEDIUM - The immutable file rule contradicts liveness files

Lines: 314, 338, 358-360, 478-490, 961.

> Every hand-off is a file under `/fleet`, written atomically, never edited after it lands.
> `heartbeat.json` [is] rewritten every 10 s by a wrapper thread.

`progress.jsonl` is also append-only, so a literal immutable-file implementation cannot represent either liveness source.

Failure scenario: one builder enforces immutability and ignores later heartbeat writes. Leases expire for healthy workers and retries or escalation follow; another builder rewrites files but uses the immutable-file ingestion rule, so later sequence values are never applied.

Smallest change: split the protocol into immutable messages and mutable snapshots or append logs, then define atomic replacement and monotonic sequence handling for each mutable type.

### S43-8 - MEDIUM - D15 operational contracts were not reconciled into the design

Lines: 292, 469, 618-627, 638-640, 816, 973, 1020-1065.

> When fleetd is down, the command writes a request file, and fleetd reads it at start.

The DDD model adds request idempotency keys, refusal reasons, optional `reset_at`, a 15 minute question limit, closed result statuses, recovery of a healthy profile, and ownership boundaries, but the design does not state them.

Failure scenario: a command times out after its request file is accepted, the CLI retries, and fleetd creates two ideas or applies an action twice because it has no command key. Separately, builders choose different question deadlines and quota-reset behavior, so the design and contract tests disagree and an unanswered question or quota wait lasts indefinitely.

Smallest change: add D15 to the Amendments table and state each accepted DDD contract in the Requirements, command-channel, question, quota, profile-health, and module-ownership sections.

## Checked and sound

- Only the verifier can move a task to done, and it reruns acceptance commands in the worker sandbox (lines 152, 265, 982, 1018).
- A worker cannot rewrite committed acceptance command strings because fleetd owns immutable `task.yaml` and the inbox (lines 359-360, 444-461).
- Branch rows record an owner, due time, rescue ref, and orphan state before branches exist (lines 588-600).
- Every ledger state change has an event time and cause, which supports historical explanation (lines 188, 630-635, 645).
- Worktree cleanup requires landed work or a verified rescue ref, and failed rescue leaves the tree in place (lines 653, 848-867).
- The design preserves the global Claude credential scrub rule and roster rejection without printing a credential value (lines 1104, 1171, 1199).
- SQLite has one writer and all other components open it read-only, which removes write contention between agents (lines 154, 495-497).
- Missed edge events are safe because the fixed reconcile pass performs full scans (lines 153, 210, 950, 984-1012).
