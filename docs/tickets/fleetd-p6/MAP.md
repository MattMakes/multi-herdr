# Map: fleetd build, Phase 6 (the idea dispatcher and sub-orchestrators)

Spec: `docs/specs/fleetd/design.md`

| # | Title | Blocked by | Wave |
|---|---|---|---|
| 01 | [The module files of Phase 6 in ideas, integration, task_execution and fleet_runtime, and their rows in the fitness rules and the architecture doc](01-modules.md) | T-498 | 1 |
| 02 | [The next schema version: ideas, requirements, task_covers and trial_merges, the idea of each task, fix_rounds, and the release backfill](02-schema.md) | T-498 | 1 |
| 03 | [The Phase 6 keys of fleet.toml: idle_stop_s, park_after_days, reminder_every_s and working_hours](03-config.md) | T-498 | 1 |
| 04 | [The idea is an entity of the 1 door: its 10 states and exits in transition(), its store, and the feature exits open to merged and ready to open](04-door.md) | 01, 02 | 2 |
| 05 | [The horch idea and horch ideas commands in the CLI, each subcommand in its own file, with its flags parsed](05-cli.md) | 01 | 2 |
| 06 | [The request kinds of Phase 6: idea_add, idea_amend, idea_approve, idea_revise, idea_park and idea_cancel of class operator, and idea note as a file of class worker](06-kinds.md) | T-498 | 1 |
| 07 | [The idea folder: idea.md, amendments, interview.md, assumptions.md, spec.md and requirements.yaml, written atomically and read with the checks of SR-6](07-folder.md) | 01 | 2 |
| 08 | [plan.yaml and the plan validator frame: the schema of a task, the tickets in format C1 that it names, and the DAG check](08-plan.md) | 01 | 2 |
| 09 | [The coverage rule of the plan validator: each requirement has a task, and each requirement cites an approved assumption or an answer](09-coverage.md) | 08 | 3 |
| 10 | [The profile and gate rules of the plan validator: each profile exists, has a fallback and a sandbox on this OS, and the project declares a fast gate and a full gate](10-profiles.md) | 08 | 3 |
| 11 | [The model guide rule of the plan validator: Opus 5.5 at medium is the default, a higher effort has a reason, a Sonnet task says no decisions, and Fable needs your flag](11-model-guide.md) | 08 | 3 |
| 12 | [The judge pairing rule of the plan validator: each task of a judgment kind has 1 judge task in another session, on another provider when the roster has one](12-judge-rule.md) | 08 | 3 |
| 13 | [horch ledger add-map --dry-run runs the plan validator on a MAP.md or a plan.yaml, with the requirement tables of check-req-coverage.sh](13-dry-run.md) | 09, 10, 11, 12 | 4 |
| 14 | [Plan commit: fleetd validates plan.yaml, writes plan.verdict.json, and commits each task of a valid plan in 1 transaction](14-plan-commit.md) | 04, 13 | 5 |
| 15 | [New features: a feature row for each idea with no --to, its internal run, a checked kebab-case branch name, and the branch created after its row](15-new-features.md) | 04 | 3 |
| 16 | [horch idea add: 1 immutable idea file and 1 captured row for each idea, with no AI, and 2 ideas sent at the same time stay 2 ideas](16-intake.md) | 04, 07, 15 | 4 |
| 17 | [horch idea add --direct: your ticket makes a plan of 1 work task, plus its judge task for a judgment kind, that still passes the plan validator](17-direct.md) | 14, 16 | 6 |
| 18 | [horch idea approve and revise: fleetd writes approval.json with the hash of the assumptions you saw, ignores any other, and I7 holds an idea whose assumptions changed](18-approval.md) | 04, 07 | 3 |
| 19 | [horch idea amend: before approval it joins the interview, after approval it is a revise with its own approval, after merge it is a new idea](19-amend.md) | 16, 18 | 5 |
| 20 | [horch idea park and cancel: worktrees are rescued first, a cancel cancels each task that is not terminal, and an abandoned feature cancels its ideas](20-park-cancel.md) | 04 | 3 |
| 21 | [The needs-you list in /fleet/needs-you/ and as a herdr notification: 1 file for each open item, removed when the item ends](21-needs-you.md) | 01 | 2 |
| 22 | [fleetd handlers for the idea kinds: each operator request of horch idea reaches its use case, with the reason of a refusal](22-handlers.md) | 06, 16, 17, 18, 19, 20 | 7 |
| 23 | [horch idea add, amend, approve, revise, park and cancel send their requests, and approve, add --direct and feature abandon refuse to run inside a fleet attempt](23-cli-write.md) | 05, 22 | 8 |
| 24 | [horch ideas and horch idea show: the board of every idea, its state and what it waits on, the open needs-you items, and the diffs of the features in flight](24-board.md) | 05, 04 | 3 |
| 25 | [horch idea note: the sub-orchestrator records each question and answer as a file of its idea folder, and fleetd ingests it 1 time](25-note.md) | 05, 07 | 3 |
| 26 | [The dispatcher loop: each pass moves each idea to its next state when its files validate, and the ideas of 1 feature are planned 1 at a time](26-dispatcher.md) | 04, 07, 14, 15 | 6 |
| 27 | [The idea-lead persona: an interactive Opus 5.5 sub-orchestrator that runs the brief, spec-writer and ticket-writer steps for 1 idea, 1 phase for each process](27-idea-lead.md) | T-498 | 1 |
| 28 | [Sub-orchestrators: 1 for each feature that is not merged or abandoned, a fresh process for each phase, started when missing and stopped when extra](28-sub-orch.md) | 26 | 7 |
| 29 | [Waiting on you is not a stall: the lease is paused, the idle process stops after 30 min, 1 reminder an hour in your working hours, and a park after 7 days](29-waiting.md) | 03, 28, 21 | 8 |
| 30 | [The question flow: the sub-orchestrator of the feature owns each worker question, fleetd starts it until an answer is recorded, and 15 min with no answer escalates to you](30-questions.md) | 28, 21 | 8 |
| 31 | [The replan of a failed task: the sub-orchestrator of the feature replaces the task or sends it back 1 time, else it is escalated](31-replan.md) | 14, 12 | 6 |
| 32 | [The 4 answer cases of a worker question: covered with a checked citation, revise to you, replan, or an own decision, and the decision log of the feature](32-answers.md) | 25, 30, 31 | 9 |
| 33 | [The roll-up of an idea and ideas finish clean: in_progress to integrating to merged, and fleetd removes the rescued trees of a merged or cancelled idea](33-rollup.md) | 04 | 3 |
| 34 | [A feature is ready only when each of its ideas is integrating, cancelled or parked: the idea state replaces the Phase 4 stand-in](34-feature-ready.md) | 04 | 3 |
| 35 | [A merge with ideas in flight: fleetd records the feature as merged and moves each idea in flight to a new feature, with a rescue ref and a rebase of each task branch](35-merged-with-ideas.md) | 15, 34 | 4 |
| 36 | [Trial merges: the trial_merges table and the trial_merge job, keyed by the 2 tips, for each feature against the primary branch and each pair of features that share a file](36-trial-merges.md) | 01, 02 | 2 |
| 37 | [Stacks: a trial merge that conflicts puts the younger feature on the older one, 1 time, with 1 resolver task that lands on the younger feature](37-stacks.md) | 36, 15 | 4 |
| 38 | [A stack edge ends with its lower feature: fleetd clears stacked_on, writes a rescue ref, rebases the upper feature, runs both gates again, and moves the base of its pull request](38-stack-edges.md) | 37, 34 | 5 |
| 39 | [The verdict file of a judge task: 1 verdict, each finding ranked CRITICAL, HIGH, MEDIUM or LOW, its structural check, and the outcome critical_open that burns no attempt](39-judge-verdict.md) | 01 | 2 |
| 40 | [The dependency of a judge task: it is met when the judged task is in verifying with a passed structural check, and the judge worktree starts from the tip of the judged branch](40-judge-dep.md) | 01 | 2 |
| 41 | [The judge pair in the verifier: the judged task waits in verifying, both reach done with no plan change, a CRITICAL item starts a fix round, and after 2 fix rounds both are escalated](41-judge-pair.md) | 39, 40 | 3 |
| 42 | [The judge branch in a fix round: fleetd writes a rescue ref of the judge branch and moves it onto the new tip of the judged branch](42-judge-branch.md) | 41 | 4 |
| 43 | [The cancel of a judge pair: the cancel of a judged task cancels its judge task, and the cancel of a judge task alone is refused](43-judge-cancel.md) | 40 | 3 |
| 44 | [Remove the orchestrator seat: no agent has operator rights, and an agent in the old orchestrator seat that sends an operator request is refused](44-no-seat.md) | 03 | 2 |
| 45 | [Retire the long-lived orchestrator conversation: horch fleet start launches no orchestrator, and fleetd no longer respawns one](45-retire-orch.md) | 28 | 8 |
| 46 | [The wakes of today's orchestrator go to their new owner: the dispatcher acts, the sub-orchestrator of the feature gets its questions, and you get the needs-you list](46-wakes.md) | 30, 21 | 9 |
| 47 | [Retire the run and plan request kinds: horch ledger run new, run close, run gate, add, add-map, task and plan-workers print the new command, and no code path makes a task without an idea](47-retire-run.md) | 06, 17, 14, 22 | 8 |
| 48 | [Retire the worker request kinds: spawn, worker_new, worker_pid, worker_reap, orchestrator_claim, orchestrator_stop and task_unblock; keep worker_stop and task_fallback](48-retire-worker.md) | 47, 45 | 9 |
| 49 | [task.yaml and START.md name the idea, its requirements.yaml and the decisions file of the feature](49-task-files.md) | 32 | 10 |
| 50 | [The release of Phase 6 at the first pass: the folder of each backfilled idea, and the sub-orchestrator of each feature takes the open questions of the run](50-release.md) | 04, 07, 30, 28 | 9 |
| 51 | [Wire Phase 6 into fleetd, part 1: the dispatcher, the sub-orchestrators, the waits, the questions, the notes and answers, the replans, the roll-up, the plan commit, the needs-you mirror and the release duty](51-pass-ideas.md) | 14, 25, 26, 28, 29, 30, 31, 32, 33, 21, 46, 50 | 10 |
| 52 | [Wire Phase 6 into fleetd, part 2: the trial merges, the stacks and their edges, a merge with ideas in flight, the feature ready rule and the judge pair](52-pass-integration.md) | 51, 34, 35, 36, 37, 38, 41, 42, 43 | 11 |
| 53 | [The fake agents of Phase 6: a fake idea-lead that writes assumptions, a spec, a plan and answers, a forging fake, and a fake judge with a verdict](53-fakes.md) | 23, 25, 27 | 9 |
| 54 | [Make the Phase 0 red tests of ideas green: FM-1 refuses a plan with an uncovered requirement, and FM-8 keeps 2 ideas sent back to back as 2 ideas](54-p0-ideas.md) | 13, 23, 24, 51 | 11 |
| 55 | [Make the Phase 0 red test of 2 conflicting features green: each feature keeps its tip, and the younger one is stacked with 1 resolver task](55-p0-landing.md) | 23, 52, 53 | 12 |
| 56 | [The e2e tests of intake and approval: FR-1, FR-10, FR-12, FM-30 and FM-32 through the binary](56-e2e-intake.md) | 51, 53, 23 | 11 |
| 57 | [The e2e tests of the dispatcher and the sub-orchestrators: FR-2, FM-16, FM-29 and FM-31](57-e2e-sub-orch.md) | 51, 53, 23, 45 | 11 |
| 58 | [The e2e tests of plans: FR-3, FR-4 and FM-10](58-e2e-plans.md) | 51, 53, 23 | 11 |
| 59 | [The e2e tests of the judge pair: RR-3, FM-9 and FM-33, and the 2 cancel tests of the gate](59-e2e-judge.md) | 52, 53 | 12 |
| 60 | [The e2e tests of questions, replans and the needs-you list: RR-12, FM-35, FM-21 and OR-3](60-e2e-questions.md) | 51, 53, 24 | 11 |
| 61 | [The e2e tests of features: FM-39, FM-41 and FM-42, and the gate test of a merge with ideas in flight](61-e2e-features.md) | 52, 53, 23 | 12 |
| 62 | [The release test of Phase 6: a run with an open question starts on Phase 5, and after the update it has 1 idea row, 1 feature row and 1 owner for its question](62-e2e-release.md) | 52, 53, 48 | 12 |
| 63 | [The skills and the teammate files of the old flow: the orchestrate skill, the orchestrator briefing, the spec-writer and the ticket-writer name the new flow](63-skills-teammates.md) | 27, 47, 48 | 10 |
| 64 | [The docs of the old flow: README, the command flow, the fleet workflow and the ticket format name plan.yaml and the idea commands](64-docs.md) | 47, 48 | 10 |
| 65 | [Gate of Phase 6: each rule of the gate green, the expected failures of Phase 6 gone, the run-in-flight test, and the gate file](65-gate.md) | 01, 02, 03, 04, 05, 06, 07, 08, 09, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64 | 13 |

## The check of the run

```
cargo test --workspace --locked
```

As in Phase 0 (`docs/tickets/fleetd-p0/MAP.md`): this command passes when each test that is not on
the list of expected failures passes, and each test on the list fails for its listed reason. The
Check of each ticket also runs the guard tests:
`cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`.

Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No
worker installs a development build, and each test runs the development binary only with temporary
folders (build rule 1). Each ticket says it.

Expected failures: Phase 6 has 3 lines today. Ticket 54 removes the 2 lines of
`crates/horch-e2e/expected-failures/p0_ideas.txt` (FM-1, FM-8) and owns `p0_ideas.rs`. Ticket
55 removes the line of `two_conflicting_features_keep_their_tips_and_reach_a_decision` in
`p0_landing.txt` and owns `p0_landing.rs`. After this phase the lists have only the lines of Phase 7.

## Waves

- Wave 1: 01, 02, 03, 06, 27
- Wave 2: 04, 05, 07, 08, 21, 36, 39, 40, 44
- Wave 3: 09, 10, 11, 12, 15, 18, 20, 24, 25, 33, 34, 41, 43
- Wave 4: 13, 16, 35, 37, 42
- Wave 5: 14, 19, 38
- Wave 6: 17, 26, 31
- Wave 7: 22, 28
- Wave 8: 23, 29, 30, 45, 47
- Wave 9: 32, 46, 48, 50, 53
- Wave 10: 49, 51, 63, 64
- Wave 11: 52, 54, 56, 57, 58, 60
- Wave 12: 55, 59, 61, 62
- Wave 13: 65

The tickets of 1 wave run at the same time. Only ticket 65 is `Parallel: no`. No 2 tickets that
can run at the same time own the same file: the generator of this map checked each pair, and a pair
that shares a file is ordered by its blockers. The chains on 1 file:

- `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs`: 06 -> 47 -> 48.
- `crates/horch/src/cmd/taskledger.rs`: 47 -> 48.
- `crates/horch-core/src/fleetd/fleet_runtime/domain/config.rs` and `adapters/config_file.rs`: 03 -> 44.
- `crates/horch-core/src/fleetd/task_execution/application/planned_tasks.rs`: 14 -> 31.
- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `adapters/effects.rs` and `domain/budget.rs`: 51 -> 52.
- `docs/architecture.md`: 01 -> 05.
- `crates/horch/src/cmd/idea/*.rs`: 05 makes each file; 23, 24 and 25 fill different files.

Ticket 01 makes each new module file of the 4 contexts, so later tickets own different files
(Q16). Each ticket with no blocker in this map (01, 02, 03, 06, 27) is blocked
by `T-498`, the gate of Phase 5.

The groups of the phase:

- Foundation: 01 to 06 (module files, schema, keys, the idea in the 1 door, the CLI tree, the request kinds).
- Ideas: 07 to 25 (the idea folder, `plan.yaml` and the plan validator, the plan commit, new features, intake, `--direct`, approval, amend, park and cancel, the needs-you mirror, the handlers and the CLI).
- Dispatcher and sub-orchestrators: 26 to 33 (the dispatcher, the `idea-lead` persona, the sub-orchestrators, the waits, the question flow, the replan, the answers, the roll-up).
- Integration: 34 to 38 (the feature ready rule, a merge with ideas in flight, trial merges, stacks, stack edges).
- Judge pair: 39 to 43.
- Retire the old flow: 44 to 48, 63, 64.
- Wiring, release and tests: 49 to 62.
- Gate: 65.

## Critical path

01 -> 08 -> 09 -> 13 -> 14 -> 26 -> 28 -> 30 -> 32 -> 51 -> 52 -> 55 -> 65

## Decisions that hold

- A guard whose fact is unknown does not hold.
- fleetd closes a pane, and removes a tree, only with a fact that proves that it is its own; else it opens a needs-you item.
- What the user reads keeps the words of today, and a new event has a word that says what happened.
- A job starts only through the launcher.
- Each ledger write goes through `transition()`, and only fleetd writes the ledger (from Phase 5).
- Each effect is a committed row first.
- The scans and the duties of the pass have a work budget and a cursor.
- OS-specific code is only in `crates/horch-platform`.
- No sandbox for a profile on an OS means no dispatch of that profile and 1 needs-you item.
- Each operator command is checked on the server side.
- No budget cap and no cap on parallel work (decision D8).
- The schema: no ticket writes a schema number. Ticket 02 writes "the next schema version" and reads `VERSION` in `crates/horch-core/src/taskledger/schema.rs`.

## Old flow and new

Each command and each skill step of today's orchestrator flow, and what you do after Phase 6. A
retired command exits 1 and prints its line of this table (tickets 47 and 48).

| Today | After Phase 6 | Request kind | Ticket |
|---|---|---|---|
| `horch ledger run new <slug>` | `horch idea add "<idea>"`: a new feature; fleetd makes its branch and its internal run (Q5). | `run_new` retired; `idea_add` | 47, 16 |
| `horch ledger add-map <MAP.md>` | The sub-orchestrator writes `plan.yaml` and tickets; fleetd validates and commits the plan after your approval. For 1 ticket that you wrote: `horch idea add --direct <ticket>`. | `map_add` retired | 47, 14 |
| `horch ledger add-map --dry-run <file>` | Stays. It runs the plan validator on a `MAP.md` or a `plan.yaml`. | none (a read) | 13 |
| `horch ledger add`, `horch ledger task` | `horch idea add --direct <ticket file> [--to <feature>]`. | `task_add`, `task_solo` retired | 47, 17 |
| `horch ledger plan-workers` | Nothing: fleetd dispatches each ready task. `horch ideas` and `horch status` show each hold. | `plan_workers` retired | 47 |
| `horch spawn <persona>`, `horch ledger spawn` | Nothing for a task: fleetd launches each attempt. To pair with a persona, plan its task with an interactive persona. | `spawn` retired | 48 |
| `horch ledger worker new`, `worker pid` | Nothing: fleetd records each attempt at its launch. | `worker_new`, `worker_pid` retired | 48 |
| `horch ledger reap` | Nothing: fleetd stops an orphan by I5. To stop 1 worker: `horch ledger stop`. | `worker_reap` retired | 48 |
| `horch ledger stop <worker> "<reason>"` | Stays: it stops 1 worker. The design has no other command for it (Q1). | `worker_stop` kept | 48 |
| `horch ledger unblock` | `horch task requeue <task>`. | `task_unblock` retired | 48 |
| `horch ledger fallback` | Stays: it gives 1 task to another persona. The design has no other command for it (Q1). | `task_fallback` kept | 48 |
| `horch ledger cancel`, `horch task cancel` | `horch task cancel <task>`; `horch idea cancel <idea> "<reason>"` for a whole idea. | `task_cancel` kept; `idea_cancel` | 20 |
| `horch ledger waive` | Stays (decision D6 keeps the review scripts and their waivers). | `ledger_waive` kept | none |
| `horch ledger run gate` | Nothing: fleetd runs the full gate of a feature as a job when its task branches are landed. | `run_gate` retired | 47 |
| `horch ledger run close` | You merge the pull request; fleetd sees the merge by content. To drop a feature: `horch feature abandon <feature>`. | `run_close` retired | 47 |
| `horch fleet start` (the orchestrator claim and stop) | `horch fleet start` starts fleetd and herdr with no orchestrator. 1 sub-orchestrator for each feature, started by fleetd. | `orchestrator_claim`, `orchestrator_stop` retired | 45, 48 |
| The orchestrator gets wakes and questions | The sub-orchestrator of the feature gets the questions of its feature; you get the needs-you list, `horch ideas` and herdr notifications. | none | 46, 30, 21 |
| `horch ledger import`, `horch ledger gc` | Stay. | `ledger_import`, `ledger_gc` kept | none |
| `horch ledger worktree scratch`, `horch worktree triage` | Stay (RR-17 needs triage). | `worktree_scratch`, `worktree_triage` kept | none |
| The skill `horch:orchestrate` | Rewritten for you: `horch idea add`, `--direct`, `--to`, `horch ideas`, `horch idea show`, `approve`, `revise`, `amend`, `park`, `cancel`, and the kept commands above. | none | 63 |
| The teammates `spec-writer` and `ticket-writer` | Their steps are the phases of `idea-lead`. | none | 27, 63 |

The operator keeps 4 powers (answer of the orchestrator to Q1): stop 1 worker (`horch ledger stop`),
cancel 1 task (`horch task cancel`), give a task to another persona (`horch ledger fallback`), and add
1 task by hand (`horch idea add --direct`).

The request kinds after Phase 6: the 16 operator kinds of the design (`idea_add`, `idea_amend`,
`idea_approve`, `idea_revise`, `idea_park`, `idea_cancel`, `task_requeue`, `task_cancel`, `job_requeue`,
`feature_abandon`, `branch_abandon`, `rescue_drop`, `worktree_restore`, `worktree_drop`, `ledger_reopen`,
`ledger_dismiss`), plus the 7 kept kinds (`worker_stop`, `task_fallback`, `ledger_waive`,
`ledger_import`, `ledger_gc`, `worktree_scratch`, `worktree_triage`). The worker kinds stay files:
`progress`, `result`, `ask` and `idea note`. 14 kinds are retired: `run_new`, `run_close`, `run_gate`,
`task_add`, `map_add`, `task_solo`, `plan_workers`, `task_unblock`, `worker_reap`, `worker_new`,
`worker_pid`, `spawn`, `orchestrator_claim`, `orchestrator_stop`.

## Open questions

Q1, Q3 and Q5 are answers of the orchestrator (QUESTION of T-499). The other points are choices of
the planner where the design and the code of today do not agree, or where the design leaves a point
open. Each ticket follows them, and the orchestrator can change each one.

- **Q1. The kinds of Phase 5 with no Phase 6 command (answer of the orchestrator, option 1).** Keep
  `ledger_waive`, `ledger_import`, `ledger_gc`, `worktree_scratch` and `worktree_triage`, and retire
  the others that Phase 6 replaces. With the check of the orchestrator: the operator must still stop 1
  worker and give a task to another persona, and the design has no command for these, so
  `worker_stop` and `task_fallback` are kept too. 14 kinds are retired. The table "Old flow and new"
  names the new action for each. Tickets 06, 47, 48.
- **Q2. 1 schema version for Phase 6.** Ticket 02 adds each table and column of the phase and
  the backfill in 1 step, with the constant `PHASE_6_VERSION`. No other ticket changes the schema.
- **Q3. Working hours (answer of the orchestrator, option 1).** The key `working_hours`, default
  `Mon-Fri 09:00-18:00` in the local time zone of the machine, a starting value. A test gives the
  time through the fleet clock. Tickets 03, 29.
- **Q4. The files of a sub-orchestrator are worker files.** `idea note`, `assumptions.md`, `spec.md`,
  `requirements.yaml`, `plan.yaml` and a replan file are files in its idea folder, read with the SR-6
  checks. They never use the channel or the request folder. Tickets 07, 25, 14.
- **Q5. The run under a feature (answer of the orchestrator, option 1).** Tasks, worktrees and gates
  use `runs.run_id` today. fleetd makes 1 internal run row for each feature through `transition()`,
  with the feature branch as its run branch and the id `F-<run seq>` of Phase 4. `horch ledger run
  new` stays as a refused command with a line that names `horch idea add`. Possible later work, not a
  ticket of this phase: remove the `runs` table and move each `run_id` column to the feature.
  Tickets 15, 47.
- **Q6. The approval of an idea with no `assumptions.md`.** A `--direct` idea and a backfilled idea
  have no assumptions phase. `approval.json` names the file that it approves (`assumptions.md`, the
  copied ticket of `--direct`, or the `MAP.md` of a run in flight) and its sha256. I7 compares that
  file. The migration sets `approved_sha256` to the marker `release`, and the first pass writes
  `approval.json` of the run's `MAP.md`. Tickets 02, 17, 18, 50.
- **Q7. The name of a feature.** The design: "The sub-orchestrator gives the name." The design names
  no field. Choice: the line `feature: <kebab-case name>` at the top of `assumptions.md`. A name that
  exists or is not kebab-case keeps the idea in `clarifying` with 1 needs-you item. Tickets
  07, 15, 26.
- **Q8. The judge of a `--direct` judgment task.** fleetd adds it: kind `judge`, a profile of another
  provider from the roster (the first such persona with a sandbox on this OS). Ticket 17.
- **Q9. What you approve.** `horch idea approve` sends the sha256 of `assumptions.md` that it printed,
  and fleetd refuses it when the file changed after. An amend after approval moves the idea back to
  `awaiting_approval` (the door has the exit `specifying` -> `awaiting_approval`, and from later
  states through a revise). Tickets 04, 18, 19, 23.
- **Q10. A parked idea.** Its running tasks go on, and no new task of it is dispatched. Your approval
  or an answer resumes it to the state it left; the door allows `parked` -> that state. Tickets
  04, 20.
- **Q11. The owner of a feature's questions.** `questions.owner` is `idea-lead:<feature id>`. I13 holds
  when the feature has this owner; its process runs when the feature has work for it (a phase, an
  open question, a failed task) and may stop when idle (the 30 min rule). Tickets 02, 28,
  30.
- **Q12. The herdr notification.** herdr shows a toast for a pane that needs input. fleetd reports the
  state `blocked` on the pane `fleet-needs-you` through `report_agent` of
  `crates/horch-core/src/workspace/herdr.rs`. The exact herdr key fields of a key binding stay a fact
  to confirm (design, To confirm by test). Ticket 21.
- **Q13. Build on what is in flight.** The design gives the diffs to the sub-orchestrator at plan time
  and names no file. Choice: `horch idea show <id> --in-flight`, a read. Ticket 24.
- **Q14. `scripts/check-req-coverage.sh` and the validator.** The script checks that each requirement
  has a test; the validator checks that each requirement has a task. Choice: the validator ports the
  rule of the script for the tables that define a requirement id, and a test checks that both find
  the same ids. The script keeps its test check. Ticket 13.
- **Q15. The decisions file of a feature.** The design: "Each feature keeps its own decisions file."
  Choice: `<fleet root>/features/F-NNNN/decisions.md`, written by fleetd only. Tickets 32,
  49.
- **Q16. Module files.** Ticket 01 makes each new file of the 4 contexts (1 file for each
  concern), so that tickets of 1 wave own different files.
- **Q17. The idea of a resolver task between 2 features.** FR-12 needs an idea for each task. Choice:
  the youngest idea in flight of the younger feature. Ticket 37.
- **Q18. The verdict file and HIGH items.** The design names the ranks, not a file form. Choice: a
  `## Verdict` section with `pass` or `critical_open` and a `## Findings` list with `[RANK]` items. A
  HIGH item goes to the decisions file and rings the sub-orchestrator, which fixes it by a replan or
  dismisses it with `horch idea note --dismiss`. Tickets 39, 41, 25.
- **Q19. A `fleet.toml` with `orchestrator_seats` after Phase 6.** The key gives 1 warning line and no
  right. Ticket 44.
- **Q20. A cancel of a task by a replan.** The design says a replan replaces a task. Choice: the old
  task follows A cancel in the transaction that adds the new tasks, so the judge rule of ticket 12
  applies to a replan too. Ticket 31.

Risks of the phase:

- **R1.** Self-hosting: this phase retires `horch ledger run new`, `add-map`, `horch spawn` and the
  orchestrator. The build fleet uses the installed `horch`, so nothing changes for it until a release.
  The first release of Phase 6 ends the orchestrator flow of the build fleet itself; the run in flight
  then moves to its feature (ticket 50), and the operator drives the fleet with `horch idea`.
- **R2.** The release test (62) builds a Phase 5 ledger with the real steps. Phases 2 to 5
  are built at the same time; a late change of their steps can break it.
- **R3.** The plan commit, a merge with ideas in flight, the stack edges, the judge branch and the
  cancel move refs or tasks in 1 transaction. A fault there can lose work. Each is Review `full` with
  `opus-hardening`.
- **R4.** The `idea-lead` persona runs on Claude Code. On Windows Claude Code has no sandbox, so no
  sub-orchestrator runs there until it has one (06 S9); its fallback `codex-sol` needs a Codex
  sandbox that denies a read (Q10 of fleetd-p5).
- **R5.** The herdr notification depends on how herdr shows a pane in state `blocked` (Q12).
- **R6.** Ticket 63 changes golden and oracle files. A wrong update hides a regression of a
  prompt.

## Coverage

Each mechanism of the row Phase 6 in "What each gate needs" and in "New work for each phase" of the
design, each mechanism of item 8 of the Migration plan, each rule of the gate line, and each REQUIRED
item of the ticket of this map, with the 1 ticket that gives it.

| Mechanism or rule | Ticket |
|---|---|
| The module files of the phase | 01 |
| The `ideas`, `requirements` and `task_covers` tables; `tasks.idea_id`; the release backfill | 02 |
| The `trial_merges` table (schema) | 02 |
| The keys of `fleet.toml` (`idle_stop_s`, `park_after_days`, `reminder_every_s`, `working_hours`) | 03 |
| The idea state machine in the 1 door; the door exits of feature `open` -> `merged` and `ready` -> `open` | 04 |
| The `horch idea` and `horch ideas` command tree | 05 |
| The request kinds of the idea commands | 06 |
| The idea folder and its SR-6 reads | 07 |
| `plan.yaml` in place of `MAP.md`; tickets stay C1; the plan validator: schema and DAG | 08 |
| The plan validator: coverage and citations to approved assumptions; I3 at plan time; FM-1 | 09 |
| The plan validator: profiles and declared gates | 10 |
| The model guide in plan validation | 11 |
| The plan validator: judge pairing (FR-4) | 12 |
| `add-map --dry-run` and `scripts/check-req-coverage.sh` into the plan validator | 13 |
| The plan commit, all-or-nothing (FR-3, FM-10) | 14 |
| Several features at the same time; 1 feature branch for each feature; the internal run | 15 |
| `horch idea add` (FR-1, FM-8) | 16 |
| `horch idea add --direct`, the fast lane (FR-12) | 17 |
| `horch idea add --to` | 15 |
| `horch idea approve` and `revise`; I7, the approval part; FR-10; FM-32 forged approval | 18 |
| `horch idea amend` | 19 |
| `horch idea park` and `cancel`; an abandoned feature cancels its ideas | 20 |
| The needs-you list in `needs-you/` and herdr notifications (OR-3) | 21 |
| The handlers of the idea kinds | 22 |
| The CLI of the write commands; SR-5 refusals inside a fleet attempt | 23 |
| `horch ideas` and `horch idea show`; build on what is in flight | 24 |
| `horch idea note` | 25 |
| The dispatcher loop of fleetd; FR-2; the idea level; the ideas of 1 feature in order | 26 |
| The `idea-lead` persona (brief, spec-writer and ticket-writer steps) | 27 |
| 1 sub-orchestrator for each feature; a fresh process for each phase; I7 live part; I13; FM-16; FM-31 | 28 |
| Waiting on you is not a stall (FM-29) | 29 |
| The question flow with the sub-orchestrator as the owner of a question (RR-12, FM-35) | 30 |
| The replan of a failed task (FM-21) | 31 |
| The 4 answer cases and the decisions file | 32 |
| The roll-up of an idea; "Ideas finish clean" (REQUIRED item 4); FM-39 | 33 |
| The rules of the feature level: ready by idea state (REQUIRED item 5) | 34 |
| A merge with ideas in flight; the exit of feature `open` to `merged` (FM-41) | 35 |
| The `trial_merges` table and the trial merge jobs | 36 |
| Stacks | 37 |
| The end of a stack edge (FM-42) | 38 |
| The judge verdict file and its structural check | 39 |
| I2 for a judge task | 40 |
| The judge pair and its fix rounds; RR-3; FM-9 green (REQUIRED item 3); FM-33 | 41 |
| The judge branch move in a fix round | 42 |
| A cancel never leaves a judgment task with no judge task | 43 |
| No agent has operator rights; the attack test (REQUIRED item 1) | 44 |
| The end of the long-lived orchestrator conversation | 45 |
| The wakes of the orchestrator go to their new owners | 46 |
| Retire the run and plan kinds (REQUIRED item 2, part 1); FR-12 no task without an idea | 47 |
| Retire the worker kinds (REQUIRED item 2, part 2) | 48 |
| `task.yaml` and `START.md` name the idea, its requirements and the decisions file | 49 |
| The release duty of Phase 6 | 50 |
| The wiring in fleetd | 51, 52 |
| The fake agents | 53 |
| Gate tests: FM-1, FM-8 (Phase 0 red tests) | 54 |
| Gate test: the Phase 0 red test of 2 conflicting features | 55 |
| Gate tests: FR-1, FR-10, FR-12, FM-30, FM-32 | 56 |
| Gate tests: FR-2, FM-16, FM-29, FM-31 | 57 |
| Gate tests: FR-3, FR-4, FM-10 | 58 |
| Gate tests: RR-3, FM-9, FM-33, and the 2 cancel tests | 59 |
| Gate tests: RR-12, OR-3, FM-21, FM-35 | 60 |
| Gate tests: FM-39, FM-41 (a merge with ideas in flight), FM-42 | 61 |
| The run-in-flight test of Phase 6 | 62 |
| The skills and teammates of the old flow | 63 |
| The docs of the old flow | 64 |
| The gate file | 65 |
| The `needs_you` table | Phase 1 (no ticket) |
| The `questions` table | Phase 2 (no ticket) |
| The `features` and `branches` tables and the landing | Phase 4 (no ticket) |
| The single writer and the command channel | Phase 5 (no ticket) |
