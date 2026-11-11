## Summary

Measured at `99b05f19` (branch `fleet/fleetd-design-T5-preservation-contract-surface-and-fixed`).
"The design" is `ai_docs/plans/fleetd-design/source-design.md`. Design lines are given as `d:<line>`.

These `NOT MENTIONED` rows would cause lost work, a stuck fleet or a security problem if fleetd dropped them. Most important first.

1. L-3, L-5: a newer schema is refused, and a development build refuses to migrate the shared ledger (`HORCH_ALLOW_LEDGER_MIGRATE`). The design adds v8 and v9 (d:824, d:828) but says nothing about mixed versions. Friends on `self-update` and a running daemon of an older version can then corrupt or lock out the shared ledger.
2. L-4, V-23: each migration first writes `ledger.v<n>-backup-<stamp>.sqlite3` and keeps 3. Without it a failed v8 or v9 migration loses the whole ledger history.
3. E-1, E-2: `HORCH_LEDGER_DB` and `HORCH_STATE_DIR` isolate every test and every scratch run from the shared ledger. The agent rule "never run a dev horch without `HORCH_LEDGER_DB`" depends on it. The design names only `FLEET_ROOT` (d:44).
4. E-10: `FORBIDDEN_ENV` removes `ANTHROPIC_API_KEY` from every agent launch, and roster validation rejects it in `env:`. This is an absolute operator rule. SR-2 (d:130) lets the profile name env vars but does not forbid this one.
5. C-1, M-1 to M-6, V-15: the mail contract. The design replaces typed mail with files (d:267) but still keeps sub-orchestrators interactive (d:49). The redelivery schedule, the mail id, the `[orchestrator re #<id>]` answer form and the `[<role>]` trust tag are not mentioned. If a sub-orchestrator pane loses a line with no redelivery, the idea waits for ever.
6. M-7: `horch done` adds `UNLANDED:` and `UNREGISTERED:` parts. These are the only report of unlanded work today. The design's rescue (d:604-650) covers worktrees of tasks, not manual worktrees outside the ledger.
7. V-10, V-11: the land lock (1 h timeout, stale after the check timeout plus 10 min, at most 5 tries) and the check timeout (20 min, max 24 h). The design's verifier runs checks "with a timeout" (d:208) but gives no value. A lower value fails long test suites. No lock lets 2 lands race on the run branch.
8. V-14: the code-sweep has at most 2 requeues and review rounds of 1 (light) and 3 (full). D6 keeps the review scripts, but the design does not name these limits.
9. F-1, F-2: ticket format C1 and the map format are what `add-map` and the ticket-writer persona read today. The design replaces them with `plan.yaml` (d:295) but keeps "today's spec-writer and ticket-writer steps" (d:828). It does not say which format the idea-lead writes.
10. F-11, V-25: project facts (`horch note --fact`, at most 100 facts of 2048 bytes) and the decisions file. The orchestrator settled that facts carry over (decisions.md), but the design does not name them.
11. C-14, F-20: `horch self-update` and the release asset names (`multi-herdr-v<version>-<target>.tar.gz`, `.zip`, `SHA256SUMS`, 5 targets). The design only runs conformance on self-update (d:759). Rollout (gap 10) is still open.
12. V-17: messages over 600 characters go to a spool file. The design writes everything to files, but the inline limit and the 7-day spool retention are not mentioned.
13. F-8: evidence logs under `~/.horch/evidence/`. The design's `verdict.json` (d:252) replaces the result, but not the log retention or path that `horch ledger steps` prints.

Count of rows per `Design` value (113 rows, first value of a mixed row): KEPT 17, CHANGED 41, NOT MENTIONED 55.
The briefing named "1.5x pace". The code has `pace_factor = 1.25` (V-26). I found no 1.5x value.

## 1. CLI contract (step 3)

Exit codes: `crates/horch/src/exit.rs:4-12` defines 0 success, 1 failure, 2 collector held, 3 refused. Any error is 1 (`main.rs:717-735`).

| # | Command | Who runs it | Where named | Exit codes a caller reads | Design |
| --- | --- | --- | --- | --- | --- |
| C-1 | `horch tell <role> "<msg>"`, `--answers <id>` | worker, orchestrator | `teammates/_base/fleet-worker.md`, `fleet-orchestrator.md:29` | 0 sent or queued; 1 error. A queue without ack prints and exits 0 (`cmd/messaging.rs:62`) | CHANGED: fixed line only (d:267) |
| C-2 | `horch ledger claim` | worker | `CLAIM_TASK`, `cmd/taskledger_spawn.rs:20` | 0 always; no task prints `horch done "No task for <persona>."` (`cmd/taskledger.rs:2411-2430`) | CHANGED: `CLAIM_TASK` goes away (d:826) |
| C-3 | `horch ledger progress`, `note` | worker | `fleet-worker.md`; `waiting:` prefix sets a 24 h wait (`taskledger/workers.rs:388`) | 0; 1 error | CHANGED: writes to outbox (d:827) |
| C-4 | `horch ledger finish [--wait] [--json]` | worker | `fleet-worker.md` | 1 while a step has not passed (`cmd/taskledger_finish.rs:337`); in a pane it detaches and returns 0 | CHANGED: fleetd runs the steps (d:82, d:826) |
| C-5 | `horch ledger done`, `horch done [--unlanded]` | worker | `fleet-worker.md` | 0 even when the tell fails (`cmd/messaging.rs:1127`); refuses until steps passed | CHANGED: `result.json` and stop hook (d:15, d:826) |
| C-6 | `horch ledger handoff <path>` | worker | `fleet-worker.md`, context lines in `teammates/_base/context-messages.md:9-10` | 0; 1 error | NOT MENTIONED |
| C-7 | `horch ledger worktree new/scratch/drop` | worker | `fleet-worker.md` | 0; 1 error | CHANGED: fleetd only runs `git worktree add` (d:17, d:825) |
| C-8 | `horch ledger status` | orchestrator, operator | `cmd/taskledger.rs:41` | 0 only when settled, 1 while work remains (`EXIT_NOT_COMPLETE`, `cmd/taskledger.rs:37`) | CHANGED: `horch status` (d:88) |
| C-9 | `horch ledger add-map [--dry-run]`, `add`, `task` | orchestrator | `fleet-orchestrator.md`, ticket-writer | `--dry-run` 1 on a problem (`cmd/taskledger.rs:2340`) | CHANGED: `horch idea add` (d:62, d:77) |
| C-10 | `horch spawn <teammate> [--model] [--resume]` | orchestrator, reviewer auto-spawn | `fleet-orchestrator.md` | 3 when the usage gate refuses (`main.rs:728`); the reviewer spawn reads 3 (`cmd/taskledger_spawn.rs:386`) | KEPT: `horch spawn` binds a task (d:826) |
| C-11 | `horch route <teammate>` | orchestrator | `fleet-orchestrator.md` | 3 refused (`cmd/route.rs:54`) | KEPT: routing unchanged (d:84) |
| C-12 | `horch compact [--request]` | orchestrator | context lines | 3 = child reported (`cmd/compact.rs:70`) | CHANGED: wrapper hands off at 256k (d:533) |
| C-13 | `horch skills read <id>`, `horch note --fact`, `horch context`, `horch inbox`, `horch peek` | worker, orchestrator | prompt text, golden files `crates/horch-core/tests/golden/worker-*.txt` | 0; 1 error | NOT MENTIONED |
| C-14 | `horch self-update [--check]` | operator, friends | `cmd/selfupdate.rs:5`; `--check` exits 0 either way (`main.rs:572`) | 0; 1 error | NOT MENTIONED (only a conformance trigger, d:759) |
| C-15 | `horch fleet`, `fleet stop` | operator | README, `fleet-orchestrator.md` | 0; 1 error | CHANGED: `horch fleetd` (d:13) |
| C-16 | `horch teammates --check` | operator, CI | `main.rs:287` | non-zero when the roster is wrong | KEPT: profile format kept (d:490) |
| C-17 | `horch update` | `horch fleet`, operator | `main.rs:129` | 1 when 1 updater fails; herdr's code wins (`cmd/update.rs:70-91`) | NOT MENTIONED |
| C-18 | `horch telemetry collect --once` | collector | `cmd/telemetry/mod.rs:453` | 2 = a live collector holds the lock | KEPT: telemetry unchanged (d:84) |
| C-19 | `horch worker` | `horch spawn` in the pane | `cmd/worker.rs:25` | child code, clamped 1 to 255 | CHANGED: wrapper with stop gate (d:79) |
| C-20 | `horch worktree land --check`, `triage`, `restore` | worker outside a ledger run, operator | prompt "Worktrees" section | 0; 1 error | KEPT: triage, restore (d:84, d:647) |
| C-21 | `horch ledger requeue/cancel/waive/unblock/reap/stop` | orchestrator | `fleet-orchestrator.md` | 0; 1 error | NOT MENTIONED (waive: D6 keeps review scripts) |
| C-22 | `horch doctor`, `smoke` | operator, install docs | README | non-zero on a failed check (`cmd/smoke.rs:123`) | NOT MENTIONED |

## 2. Message contract (step 4)

| # | Item | Form | Location | Reason | Design |
| --- | --- | --- | --- | --- | --- |
| M-1 | Sender tag | `[<role>] ...`; added when missing. Untagged text is the operator | `messaging/message.rs:3-31` | trust boundary, module doc | NOT MENTIONED |
| M-2 | Keywords | `ready`, `DONE:`, `BLOCKED:`, `NOTE:`, `QUESTION:`, `COMPACT-READY`. Parser: uppercase word before `:` | `messaging/mail.rs:825`; prompt text in `teammates/_base/fleet-worker.md` | digest names sender and keyword | CHANGED: questions are files (d:248) |
| M-3 | Mail id | `[<from> #<short id>]`; a redelivery keeps the first id. Full id `<millis13>-<nanos20>-<pid>-<counter>` | `messaging/message.rs:34-48`, `messaging/mail.rs:863-874` | C2 of `docs/specs/trace-fixes.md` | NOT MENTIONED |
| M-4 | Answer form | `[orchestrator re #<id>] <text>`; only to the asker | `cmd/messaging.rs:254-270` | C2 | NOT MENTIONED |
| M-5 | Redelivery suffix | `<line> (redelivery <n>: not acknowledged; run horch inbox)` | `cmd/mail.rs:321`, `cmd/mail.rs:381` | courier until ack | NOT MENTIONED |
| M-6 | Digest line | `[horch] NOTE: <n> unread messages (<from> <KEYWORD>, ...). Run horch inbox.`; NOTE lines get no redelivery to the orchestrator | `messaging/mail.rs:818-850` | do not flood the orchestrator, `cmd/mail.rs:258` | NOT MENTIONED |
| M-7 | DONE parts | `... \| UNLANDED: <claim>: <reason>`, `... \| UNREGISTERED: <entry>`; finish form `UNLANDED: <reason> \| <summary>` | `execution/lifecycle.rs:224-228`, `taskledger/finish/mod.rs:1112`; read by `taskledger/import.rs:462` | report work that did not land | NOT MENTIONED |
| M-8 | Channel event | JSON-RPC `notifications/claude/channel`, `params {content, meta {role}}`; Codex: `codex queue --thread <id> --message` | `messaging/channel.rs:700-720` | reason not recorded | NOT MENTIONED |
| M-9 | Horch notices | `[horch] NOTE: ...` lines: finish result, reviews in, context requests | `messaging/mail.rs:780-797`, `teammates/_base/context-messages.md` | durable notice ends a wait | CHANGED: constant wake line (d:267) |
| M-10 | Long message | over 600 chars: spool file plus 1 tagged line with 240-char head | `messaging/message.rs:17-21`, `message.rs:66` | a busy TUI splits long lines | NOT MENTIONED |

## 3. File contracts (step 5)

| # | File | Writer | Reader | Location | Design |
| --- | --- | --- | --- | --- | --- |
| F-1 | Ticket `docs/tickets/<slug>/<NN>-<name>.md`, `<!-- horch-ticket: 1 -->`, fields Persona, Parallel, Kind, Review, Check, Own, Do not touch | ticket-writer, orchestrator | `add-map`, `claim` snapshot (`ticket_snapshot`, `ticket_sha`) | `docs/specs/ticket-format.md:11-45`, `taskledger/map.rs:212` | CHANGED: `plan.yaml` (d:295) |
| F-2 | Map `MAP.md`: table `#`, `Title`, `Blocked by`, `Wave` | ticket-writer | `add-map` | `docs/specs/ticket-format.md:82-97` | CHANGED: `plan.yaml` `after:` (d:316) |
| F-3 | Teammate frontmatter, `deny_unknown_fields`, 45 fields (agent, model, effort, skills, fallbacks, horch_access, env, compact_window, ...) | operator | roster, `teammates --check` | `roster/teammate.rs:84-287` | KEPT plus new fields (d:490-525) |
| F-4 | `teammates/_base/*.md` (fleet-worker, fleet-orchestrator, context-messages, context-windows, harness-defaults) | repo | prompt builder, golden tests | `teammates/_base/`; pinned by `tests/golden_prompts.rs`, `tests/oracles/` | CHANGED: `prompts/worker-base.md` (d:230) |
| F-5 | Skill bundle `<state>/skill-bundles/<execution_id>/skills/<id>/` | spawn | agent (`horch skills read`) | `skills.rs:108-153` | NOT MENTIONED |
| F-6 | `skills/copied.json` (deny_unknown_fields) | operator | catalog | `skills/catalog.rs` | NOT MENTIONED |
| F-7 | Handoff `ai_docs/handoffs/<role>-whats-next.md`; orchestrator `orchestrator-whats-next.md` | worker | next claim (`PRIOR WORK`) | `compaction/policy.rs:396-409`, `taskledger/orchestrator.rs:42`; `workers.handoff_path` | CHANGED: handoff from `progress.jsonl` (d:679) |
| F-8 | Evidence `~/.horch/evidence/<run>/<task>/...`, land locks `~/.horch/evidence/locks/` | finish steps | `ledger steps`, reviewers | `cmd/taskledger.rs:2570`, `finish/land.rs:401` | NOT MENTIONED |
| F-9 | `~/.horch/ledger.sqlite3` (+ `-wal`, `-shm`, `.outbox.lock`, backups) | horch | horch | `taskledger/store.rs:25` | KEPT (d:86) |
| F-10 | `~/.horch/watchdog-<run>.log`, watchdog lock | watchdog | operator | `cmd/taskledger_watch.rs:568`, `taskledger/watchdog.rs:714` | CHANGED: fleetd (d:76) |
| F-11 | `<state>/facts/<hash>.txt` | `horch note --fact` | claim prints them | `project_notes.rs:10-14` | NOT MENTIONED |
| F-12 | `<state>/mail/<workspace>/`, `<state>/messages/<workspace>/` | tell, courier | inbox, courier | `messaging/mail.rs:101`, `messaging/message.rs:62` | CHANGED (d:267) |
| F-13 | `<state>/<project slug>.json` legacy session ledger | old horch | `ledger import`, `sessions` | `execution/store.rs:432` | NOT MENTIONED |
| F-14 | `<state>/claude/`, `codex/`, `codex-home/`, `pi/`, `prime/`, `tmp/` per-pane config | spawn | agent CLI | `harness/claude.rs`, `harness/codex.rs`, `sweep.rs` | CHANGED: `attempts/A1/home/` (d:251) |
| F-15 | `<state>/policy.json`, `quota.json`, `telemetry/` (`collector.json`, `snapshot.json`) | operator, collector | route, quota, usage | `routing/policy.rs:86`, `routing/quota.rs`, `telemetry/lock.rs` | KEPT (d:84) |
| F-16 | `<state>/compact/`, `context/` | compact job | `horch context` | `compaction/jobfile.rs`, `cmd/context.rs` | CHANGED (d:533) |
| F-17 | `<state>/worktrees/`, `update/`, `self-check.json`, `channels/` | horch | horch | `vcs/landing.rs`, `harness/update.rs`, `cmd/selfupdate.rs`, `sweep.rs` | NOT MENTIONED |
| F-18 | `<data>/startup-errors`, marketplace skills under `~/.local/share/horch/` | spawn, marketplace | operator | `execution/lifecycle.rs`, `paths.rs:129-136` | NOT MENTIONED |
| F-19 | Brief file `<pane>.brief.json`, `SCHEMA = 2` | spawn | `horch worker` | `messaging/brief.rs:22` | CHANGED: `task.yaml`, `START.md` (d:243-245) |
| F-20 | Release assets `multi-herdr-v<ver>-<target>.tar.gz` / `.zip`, `SHA256SUMS`, 5 targets | `.github/workflows/release.yml:50-58` | `self-update` | `cmd/selfupdate.rs:5`, `:120` | NOT MENTIONED |

## 4. Ledger schema contract (step 6)

| # | Item | Fact | Location | Reason | Design |
| --- | --- | --- | --- | --- | --- |
| L-1 | Version | `VERSION = 7`, in `PRAGMA user_version` | `taskledger/schema.rs:15` | reason not recorded | CHANGED: v8 leases, v9 ideas (d:824, d:828) |
| L-2 | How a migration runs | `Store::open`: WAL, `synchronous=NORMAL`, foreign keys on, then `migrate_guarded` in 1 `BEGIN IMMEDIATE` transaction; forward only | `taskledger/store.rs:163-193`, `schema.rs:599-646` | 2 openers must not both create tables (`schema.rs:600`) | KEPT: phased versions (d:824) |
| L-3 | Newer file refused | file above `VERSION` fails with "Run horch self-update, or ... copy the backup" | `schema.rs:575-597` | old binary does not know the columns | NOT MENTIONED |
| L-4 | Backup | `ledger.v<n>-backup-<UTC>.sqlite3`, 3 kept | `schema.rs:519-573` | rows written after the backup are lost (message) | NOT MENTIONED |
| L-5 | Dev-build guard | a `target/` build refuses to migrate `<passwd home>/.horch/ledger.sqlite3` unless `HORCH_ALLOW_LEDGER_MIGRATE=1` | `store.rs:30-80`, `:118` | installed horch would refuse the file | NOT MENTIONED |
| L-6 | Read-only open | `open_read_only` for sandboxed commands | `store.rs:196-205` | Codex sandbox cannot write | KEPT: others read-only (d:827) |
| L-7 | Tables v1 | projects, runs, tasks, task_deps, task_files, workers, checks, stops, worktrees, events, task_steps, import_raw, import_findings | `schema.rs:32-360` | `docs/specs/task-ledger.md` sections 4, 11 | CHANGED: new tables (d:390-470) |
| L-8 | Tables v2-v7 | worker_waits, worktree_modes (v2); run_gates, outbox, session_records (v7) | `schema.rs:380-490` | v7: trace-fixes C4, D10, TRF-45 | NOT MENTIONED (outbox, session_records); worker_waits CHANGED (d:824) |
| L-9 | Imported rows | `origin = 'import'` skips the `CHECK` rules | `schema.rs:17-28` | section 11.2 | NOT MENTIONED |
| L-10 | Readers outside horch | none found: no script, skill, teammate or workflow opens the ledger. `scripts/live/*.sh` use sqlite3 only on opencode data. E2E tests use `HORCH_LEDGER_DB` | `scripts/live/telemetry.sh:137`, `crates/horch-e2e/src/harness.rs` | - | NOT MENTIONED |
| L-11 | Busy timeout | 30 s; open lock stale after 5 min | `store.rs:14-18` | journal-mode switch is not covered by busy timeout (`store.rs:171`) | NOT MENTIONED |

## 5. Environment contract (step 7)

Names and meanings only. Location is the first read.

| # | Name | Meaning | Location | Design |
| --- | --- | --- | --- | --- |
| E-1 | `HORCH_LEDGER_DB` | task ledger file; travels in the brief | `runtime/paths.rs:33-38` | NOT MENTIONED |
| E-2 | `HORCH_STATE_DIR` | state root; passed to panes | `paths.rs:20-24` | CHANGED: `FLEET_ROOT` (d:44) |
| E-3 | `HORCH_DATA_DIR`, `XDG_STATE_HOME`, `XDG_DATA_HOME` | data root, XDG roots | `paths.rs:120-136` | NOT MENTIONED |
| E-4 | `HORCH_PROJECT_DIR` | project root, else cwd | `paths.rs:138` | NOT MENTIONED |
| E-5 | `HORCH_ROLE`, `HORCH_TEAMMATE`, `HORCH_AGENT`, `HORCH_MODEL`, `HORCH_RECORD_ID`, `HORCH_SESSION_ID`, `HORCH_RESUME`, `HORCH_TASK`, `HORCH_WORKER_ID`, `HORCH_WORKSPACE_ID` | identity of a worker pane; a detached child gets none of them | `runtime/context.rs:330-342`, `runtime/process.rs:240-243` | CHANGED: task folder binds the worker (d:243) |
| E-6 | `HORCH_REPORT_SOCKET` | `horch_access: report` pane: only progress, tell, done | `runtime/context.rs:357`, `harness/claude.rs:1566` | CHANGED: outbox files (d:827) |
| E-7 | `HORCH_CLAIM_TARGET`, `HORCH_RUN_ID` | task a spawned reviewer claims first; run of a spawn | `cmd/taskledger_spawn.rs:24`, `:214` | CHANGED: push dispatch (d:14) |
| E-8 | `HORCH_ORCHESTRATOR_RESPAWN` | `<slug> <n>` on a respawned orchestrator | `taskledger/orchestrator.rs:46` | CHANGED (d:828) |
| E-9 | `HORCH_VERIFY` | finish profile default | `taskledger/model.rs:383` | NOT MENTIONED |
| E-10 | `ANTHROPIC_API_KEY` | removed from every launch; rejected in `env:` | `harness/launch.rs:271`, `roster/validation.rs:1681` | NOT MENTIONED |
| E-11 | `HORCH_ALLOW_LEDGER_MIGRATE` | lets a dev build migrate the shared ledger | `taskledger/store.rs:30` | NOT MENTIONED |
| E-12 | `HORCH_*_BIN` (HERDR, CLAUDE, CODEX, GIT, GH, NODE, PI, PRIME, OPENCODE, OLLAMA, ANTIGRAVITY, SQLITE3) | binary overrides | `runtime/bins.rs` | NOT MENTIONED |
| E-13 | `HORCH_TEAMMATES_DIR`, `HORCH_CODE_SWEEP_DIR`, `HORCH_COMPLEXITY_CLI` | roster and review tool folders | `runtime/context.rs:382`, `cmd/taskledger.rs:2552`, `:2562` | NOT MENTIONED |
| E-14 | `HORCH_NO_WATCHDOG`, `HORCH_NO_WAKE`, `HORCH_NO_UPDATE`, `HORCH_NO_SELF_CHECK`, `HORCH_SELF_UPDATED` | turn a background action off | `cmd/taskledger_watch.rs:432`, `cmd/taskledger_wake.rs:27`, `runtime/context.rs:420-421`, `cmd/selfupdate.rs:309` | NOT MENTIONED |
| E-15 | `HORCH_TELL_GRACE_MS`, `HORCH_SPAWN_WAIT_MS`, `HORCH_STOP_GRACE_MS`, `HORCH_UPDATE_TIMEOUT_MS`, `HORCH_UPDATE_COOLDOWN_DAYS`, `HORCH_PROBE_TIMEOUT_MS`, `HORCH_LOCAL_PROBE_TIMEOUT_MS` | time overrides | `runtime/context.rs:414-425`, `cmd/taskledger.rs:1686` | NOT MENTIONED |
| E-16 | `HORCH_NOW`, `HORCH_FAULT`, `HORCH_TEST_*`, `HORCH_RELEASE_API`, `HORCH_MACHINE_FILE`, `HORCH_QUOTA_FILE`, `HORCH_OPENCODE_DB`, `HORCH_HOSTNAME`, `HORCH_CLAUDE_MANAGED_SETTINGS` | test clock, fault points, fixtures; hostname beside every PID | `runtime/context.rs:398-428`, `runtime/machine.rs:385`, `paths.rs:43` | KEPT: fault points used by Phase 0 (d:822) |
| E-17 | `HORCH_TILE`, `HORCH_BALANCE`, `HORCH_DELIVERY_73`, `HORCH_TARGET` | layout switches, delivery probe, build triple | `runtime/context.rs:404-405`, `cmd/delivery_probe.rs:14`, `cmd/selfupdate.rs:25` | NOT MENTIONED |
| E-18 | Set for agents: `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `TMPDIR`, `OPENCODE_CONFIG_CONTENT`; read: `HERDR_PANE_ID`, `HERDR_SOCKET_PATH`, `CODEX_SANDBOX` | agent CLI wiring (ticket 06 owns details) | `harness/*.rs`, `main.rs:731` | KEPT (d:171, d:178) |

## 6. Fixed values (step 8)

| # | Value | Location | Why | Impact if changed | Design |
| --- | --- | --- | --- | --- | --- |
| V-1 | check every 600 s, `stall_rounds` 3 (30 min to dead) | `schema.rs:56-57`, `docs/specs/task-ledger.md:126-127` | spec defaults | earlier or later reap | CHANGED: 5 s pass, stall 5 min, lease 120 s (d:52, d:824) |
| V-2 | herdr `working` proxy at most `2 * stall_rounds` checks (about 70 min) | `taskledger/watchdog.rs:15-17` | #203 | idle pane held longer | CHANGED (d:64, d:668) |
| V-3 | `heartbeat_every_s` 30; `HEARTBEAT_EVERY` 2 s, `STALE_AFTER` 30 s | `schema.rs:58`, `heartbeat.rs:25-29` | spec | dead worker seen later | CHANGED: 10 s, stale 30 s (d:362, d:670) |
| V-4 | `waiting:` progress wait 24 h | `taskledger/workers.rs:389` | reason not recorded | idle claim held 1 day | CHANGED: `max_wait_s` 3600 (d:515) |
| V-5 | question wait 7 days from the oldest question | `workers.rs:825` | "polling must never extend it" (`:819`) | stuck worker held 7 days | CHANGED (d:515) |
| V-6 | reviews wait `REVIEWS_WAIT_SECS` 7 days | `workers.rs:615` | #177 | same | CHANGED (d:515) |
| V-7 | detached finish wait `3 * check + 3600` s | `cmd/taskledger_finish.rs:137-143` | cover the idle worker | worker reaped mid-finish | NOT MENTIONED |
| V-8 | `LAUNCH_GRACE_S` 120, `CLAIM_GRACE_S` 600, `UNCLAIMED_AFTER_S` 180, `REWAKE_AFTER_S` 120, `RECONCILE_S` 30, `REVIEWS_IN_AFTER_S` 60 | `workers.rs:454-458`, `watchdog.rs:815-823`, `:1006` | #190 | lost spawns or wake storms | CHANGED: start timeout 90 s (d:710) |
| V-9 | orchestrator respawn: 1 per 600 s, 3 per run | `taskledger/orchestrator.rs:36-39` | D6, C7 of trace-fixes | respawn loop or dead fleet | CHANGED: dispatcher in code (d:77) |
| V-10 | land lock 1 h, stale margin 10 min, 5 tries; check timeout 20 min, max 24 h; notice every 30 s | `finish/land.rs:18-40` | `task-ledger.md:661` | long suites fail; lands race | NOT MENTIONED |
| V-11 | `vcs/landing.rs` lock stale 1 h, timeout 30 s, heartbeat 60 s | `vcs/landing.rs:20-22` | reason not recorded | as V-10 | NOT MENTIONED |
| V-12 | push bound 10 min, other git network 3 min | `vcs/network.rs:39-41` | reason not recorded | hung push blocks land | NOT MENTIONED |
| V-13 | run `max_attempts` 3; retry priority +1000 | `schema.rs:393`, `tasks.rs:23` | reason not recorded | more cost or lost tasks | KEPT: 3 (d:418, d:517) |
| V-14 | code-sweep `MAX_REQUEUES` 2; review rounds light 1, full 3 | `finish/codesweep.rs:280`, `ticket-format.md:33` | C1 | review loops or no review | NOT MENTIONED (D6 keeps scripts) |
| V-15 | redelivery 30, 60, 120, 240, 300, 300 s, then every 300 s, never expires | `messaging/mail.rs:799-810` | module doc | lost answers | NOT MENTIONED |
| V-16 | digest first at 5 min, gap 15 min; courier poll 5 s; ack retention 7 days | `mail.rs:812-814`, `:17`, `cmd/mail.rs:18` | do not flood | orchestrator misses or drowns | NOT MENTIONED |
| V-17 | `MAX_INLINE` 600 chars, head 240; spool retention 7 days, sweep every 1 h | `message.rs:17-21`, `sweep.rs:36-49` | busy TUI splits lines | split lines lose the tag | NOT MENTIONED |
| V-18 | `TELL_GRACE` 30 s, `DONE_PUSH_BUDGET` 20 s, channel send 12 s, connect 5 s, codex queue 10 s | `messaging/delivery.rs:201-243`, `messaging/channel.rs:24-34` | reason not recorded | tell hangs or drops | NOT MENTIONED |
| V-19 | context watch `min(300000, floor(0.8 x native))`, headroom floor 20000 | `compaction/window.rs:5-18` | CTX-07, CTX-08 | no time to write a handoff | CHANGED: 256000 every seat (d:106, d:829) |
| V-20 | compaction: match window 15 min, request deadline 20 min, 2 requests, handoff max age 1 h | `compaction/policy.rs:26-37` | CTX-10, CTX-11 | stuck or lost compaction | CHANGED (d:533) |
| V-21 | compact job: idle timeout 1800 s, marker and settle 600 s, reading 180 s, resend 30 s | `cmd/compact.rs:63-79` | reason not recorded | compaction aborts | CHANGED (d:533) |
| V-22 | `SPAWN_WAIT` 15 s, stop grace TERM to KILL 10 s | `cmd/spawn.rs:40`, `cmd/taskledger.rs:1686` | ADR 0002 | orphan processes | CHANGED: SIGINT, 20 s, SIGKILL (d:217) |
| V-23 | `BUSY_TIMEOUT` 30 s, schema backups 3 | `store.rs:14`, `schema.rs:520` | see L-4, L-11 | lock errors; lost history | NOT MENTIONED |
| V-24 | trash refs 30 days (`ledger gc`) | `triage/refs.rs:20` | #308 | lost rescue copies | KEPT: "same retention rules" (d:649) |
| V-25 | project facts 100, 2048 bytes each | `project_notes.rs:7-9` | oldest expire first | facts lost or prompt bloat | NOT MENTIONED |
| V-26 | routing: tight 0.85, exhausted 0.98, pace 1.25x at 0.50 used, probe 15 min, stale 30 min, retention 35 days, headroom 2.0 | `routing/policy.rs:66-82` | design 13.4 (`main.rs:720`) | wrong refuse or spend | KEPT: routing (d:84); D2 adds waiting_quota |
| V-27 | agent CLI update cooldown 7 days, timeout 180 s, max 1800 s, lock stale 1 h | `harness/cooldown.rs:28`, `runtime/context.rs:198`, `harness/update.rs:67-71` | reason not recorded | broken CLI on a fresh release | NOT MENTIONED |
| V-28 | self-update download 120 s, 256 MiB max, check cache 24 h | `cmd/selfupdate.rs:31-40` | reason not recorded | stalled or unsafe update | NOT MENTIONED |
| V-29 | grid: tab 1 2x2 workers, overflow 2x3; arrange lock 20 s, stale 60 s | `workspace/tile.rs:57-65`, `workspace/arrange.rs:41-46` | reason not recorded | layout only | NOT MENTIONED |
| V-30 | telemetry: unseen 300 s, quiet 24 h, tidy 60 min, batch 10000 | `telemetry/start.rs:34`, `telemetry/collect.rs:876-912` | reason not recorded | usage data gaps | KEPT (d:84) |
| V-31 | import heavy file 1 GiB | `taskledger/import.rs:33` | reason not recorded | slow import | NOT MENTIONED |
| V-32 | `ABORT_EXIT_CODE` 86 for fault points | `runtime/fault.rs:12` | tests read it | fault tests break | KEPT: Phase 0 fault points (d:822) |
