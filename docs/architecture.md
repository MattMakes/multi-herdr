# Architecture: what lives where

This page tells you what the program does and which file to open for each
concern. For how the commands connect at run time, see
[command-flow.md](command-flow.md).

## 1 program over 1 core

| binary | entry file | what it does |
|---|---|---|
| `horch` | `crates/horch/src/main.rs` | Runs a fleet: 1 orchestrator pane that spawns worker panes, the task ledger, messaging, tiling, cost and telemetry |

The binary is thin. It parses flags, builds a `RuntimeContext`, calls
`horch-core`, and prints the result. Every worker starts through the spawn
service, and its session record goes to the `session_records` table of the
task ledger.

The workspace has 6 crates:

| crate | holds |
|---|---|
| `crates/horch-core` | The domain: roster, harnesses, routing, executions, messaging, workspace, telemetry, the task ledger |
| `crates/horch` | The `horch` CLI |
| `crates/horch-marketplace` | Skill sources, installs and the lockfile |
| `crates/horch-e2e` | Fake harness binaries and the hermetic end-to-end tests |
| `crates/herdr-install` | The Herdr CLI installer |
| `crates/agent-docs-sync` | The library that mirrors the docs of each agent CLI, also into `herdr-docs/` |

## The rules the tests enforce

`crates/horch-core/tests/arch_scan.rs` scans the source. A junior can break
these rules by accident:

| test | rule | what to do instead |
|---|---|---|
| `arc_05_no_ambient_env_in_core` | No `std::env` read in `horch-core` outside `runtime/` | Read the value in `runtime/` into `RuntimeContext` (`crates/horch/src/bootstrap.rs` builds it once) and pass it down |
| `arc_06_env_mutation_sites_reduced` | No `set_var(` or `remove_var(` in `horch-core` or `horch` | Put the variable on the child `Command` |
| `arc_22_no_error_string_matching` | No branch on an error's message text | Match a typed error (`downcast_ref`); pick an exit code from `crates/horch/src/exit.rs` by type |
| `arc_10_harness_match_only_in_harness` | Outside tests, `HarnessKind` variants are matched only under `harness/` and in `roster/validation.rs` | Add a field to `Capabilities` in `harness/capabilities.rs` and read it |
| `arc_25_no_shim_modules` | No file that only re-exports; each `pub use` names one of the file's own child modules | Import an item from the module that owns it; make an item `pub(crate)` when nothing outside the crate uses it |
| `fitness_domain_imports_nothing_outward` | A file under `fleetd/<context>/domain/` names only `std` and `crate::fleetd::<the same context>::domain`: no other crate, no other module of `crate` | Put the I/O in `adapters/` and the use case in `application/`; pass the data into the domain as values |
| `fitness_no_context_touches_foreign_tables` | A SQL string under `fleetd/<context>/` names only the tables of that context (`FLEETD_TABLE_OWNERS`); the 1 door `fleetd/fleet_runtime/adapters/sqlite.rs` is exempt | Call the context that owns the table, or write the state through the door |
| `fitness_only_custody_and_triage_move_a_worktree_to_removed` | Only the `worktree_remove` job (`worktree_custody/application/remove.rs`) and the triage settle of a gone folder (`taskledger/triage/apply.rs`, behind `settle_refusal`) call the door with the target `removed` | Queue a `worktree_remove` job; never move a worktree to `removed` by hand |
| `fitness_only_worktree_custody_runs_git_worktree_add_remove_or_prune` | Outside tests, only `fleetd/worktree_custody/adapters/` runs `git worktree add`, `remove` or `prune`; `vcs/landing.rs` (the claims of `horch worktree new` and `land`) is the 1 exemption (`WORKTREE_CALL_EXEMPT`). `git worktree list` is a read and is allowed | Queue a `worktree_add` or `worktree_remove` job of worktree custody |
| `fitness_each_worktree_insert_writes_its_state` | Each production `INSERT INTO worktrees` in `horch-core` and `horch` names the `state` column; no new row takes the default `orphaned` of the Phase 3 migration | Write the state of the new row: `reserved` through worktree custody, or the state of the release backfill for a row of an old record |

## horch-core modules

`crates/horch-core/src/lib.rs` lists the modules. "Start at" is the file to
open first.

| module | owns | start at |
|---|---|---|
| `ids` | typed identities (`ExecutionId`, `RoleName`, `PaneId`, ...) | `crates/horch-core/src/ids.rs` |
| `clock` | the one clock horch reads (`HORCH_NOW` pins it) | `crates/horch-core/src/clock.rs` |
| `fsx` | durable file writes and the cross-process directory lock (`DirLock`) | `crates/horch-core/src/fsx.rs` |
| `procid` | process identity: a pid with its start time, and the signal to that exact process | `crates/horch-core/src/procid.rs` |
| `exeid` | which binary a long-lived horch process runs, so a stale watchdog or collector is noticed after an install | `crates/horch-core/src/exeid.rs` |
| `heartbeat` | the liveness of background jobs (the compaction job) | `crates/horch-core/src/heartbeat.rs` |
| `sweep` | the removal of stale state whose owner is gone (records, spools, sockets, registries, mail, bundles, codex homes, temp files), run by `fleet`, `spawn` and `doctor --clean` | `crates/horch-core/src/sweep.rs` |
| `runtime` | the process boundary: `RuntimeContext`, paths, binary overrides (`HORCH_*_BIN`), fault points | `crates/horch-core/src/runtime/context.rs`, `crates/horch-core/src/runtime/paths.rs`, `crates/horch-core/src/runtime/bins.rs` |
| `roster` | `teammates/` files: parsing, layering, `--check` rules, effort and permission mapping | `crates/horch-core/src/roster/repository.rs`, `crates/horch-core/src/roster/validation.rs` |
| `prompts` | rendering briefings from `teammates/` | `crates/horch-core/src/prompts.rs` |
| `skills` | skill catalogs, phase selection, activation plans, launch bundles | `crates/horch-core/src/skills/selection.rs`, `crates/horch-core/src/skills/catalog.rs` |
| `harness` | 1 adapter per agent CLI (claude, codex, OpenCode, pi, Prime, Antigravity), capabilities, the launch flow, the `agent-list` inventory | `crates/horch-core/src/harness/mod.rs`, `crates/horch-core/src/harness/launch.rs` |
| `routing` | quota-aware routing: policy, quota pools, the spawn decision | `crates/horch-core/src/routing/decision.rs`, `crates/horch-core/src/routing/quota.rs` |
| `execution` | executions: model, the session record (`horch sessions`) and its store in the `session_records` table, spawn plan, the spawn service, lifecycle (`done`) | `crates/horch-core/src/execution/service.rs`, `crates/horch-core/src/execution/store.rs` |
| `messaging` | the worker brief, the mailbox and message delivery | `crates/horch-core/src/messaging/brief.rs`, `crates/horch-core/src/messaging/mailbox.rs` |
| `project_notes` | durable project facts (`horch note --fact`) and the host line of every brief | `crates/horch-core/src/project_notes.rs` |
| `taskledger` | the SQLite task ledger (`horch ledger`): runs, tasks, claims, workers, the watchdog, the finish steps, deterministic stop, the import of old JSON ledgers, the worktree triage of parked worktrees (`taskledger/triage/`) | `crates/horch-core/src/taskledger/mod.rs`, `crates/horch-core/src/taskledger/finish/mod.rs` |
| `fleetd` | the fleet daemon, 1 folder for each context (`fleet_runtime`, `task_execution`, `worker_profiles`, `ideas`, `worktree_custody`, `integration`), each with `domain/`, `application/` and `adapters/` | `crates/horch-core/src/fleetd/mod.rs` |
| `fleetd/integration` | the fleetd context of features, task branches, bundles, gates and merges; it owns the tables `features`, `branches`, `bundles`, `bundle_members`, `gate_runs` and `remote_tips`, and runs the job kinds `bundle_build`, `fast_gate`, `full_gate`, `fetch`, `push` and `rebase` | `crates/horch-core/src/fleetd/integration/mod.rs`, `crates/horch-core/src/fleetd/integration/adapters/executors.rs` |
| `fleetd/fleet_runtime/domain/requests` | the request contract: the kind catalog and the class of each kind, the `request_id` rule, the size rule of a line, the reasons of a refusal (fleetd-p5 ticket 08) | `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs` |
| `fleetd/fleet_runtime/domain/authorize` | the authorization decision of a request, from its facts (fleetd-p5 ticket 09) | `crates/horch-core/src/fleetd/fleet_runtime/domain/authorize.rs` |
| `fleetd/fleet_runtime/domain/operator` | `is_orchestrator_seat`, the 1 function that knows an orchestrator seat (fleetd-p5 ticket 09) | `crates/horch-core/src/fleetd/fleet_runtime/domain/operator.rs` |
| `fleetd/fleet_runtime/application/requests` | the apply of a request: pending, then applied or refused in 1 transaction; a seen `request_id` gets its stored result (fleetd-p5 ticket 12) | `crates/horch-core/src/fleetd/fleet_runtime/application/requests.rs` |
| `fleetd/fleet_runtime/application/handlers_run` | the handlers of the run and plan request kinds (fleetd-p5 ticket 17) | `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_run.rs` |
| `fleetd/fleet_runtime/application/handlers_task` | the handlers of the task request kinds (fleetd-p5 ticket 18) | `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_task.rs` |
| `fleetd/fleet_runtime/application/handlers_worker` | the handlers of the worker request kinds (fleetd-p5 ticket 19) | `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_worker.rs` |
| `fleetd/fleet_runtime/application/handlers_custody` | the handlers of the custody, job and integration request kinds (fleetd-p5 ticket 20) | `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs` |
| `fleetd/fleet_runtime/adapters/requests_store` | the `requests` rows in SQLite (fleetd-p5 ticket 12) | `crates/horch-core/src/fleetd/fleet_runtime/adapters/requests_store.rs` |
| `fleetd/fleet_runtime/adapters/channel_server` | the command channel of fleetd: bind, the peer user check, 1 request line, the reply (fleetd-p5 ticket 13) | `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs` |
| `fleetd/fleet_runtime/adapters/request_spool` | the request files in `<fleet root>/run/requests/`: checks, row, then archive (fleetd-p5 ticket 14) | `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_spool.rs` |
| `fleetd/fleet_runtime/adapters/request_wire` | the JSON line of a request and of its reply, and `payload_sha256` with sorted keys (fleetd-p5 ticket 08) | `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_wire.rs` |
| `fleetd/fleet_runtime/adapters/protected_files` | the 1 reader of an untrusted file: owner, mode, link, real path and size checks (fleetd-p5 ticket 07) | `crates/horch-core/src/fleetd/fleet_runtime/adapters/protected_files.rs` |
| `fleetd/fleet_runtime/adapters/token_file` | the operator token `<fleet root>/run/operator.token`, mode 0600 (fleetd-p5 ticket 10) | `crates/horch-core/src/fleetd/fleet_runtime/adapters/token_file.rs` |
| `fleetd/fleet_runtime/adapters/peer_process` | the peer process fact: is a channel peer in a live attempt (fleetd-p5 ticket 11) | `crates/horch-core/src/fleetd/fleet_runtime/adapters/peer_process.rs` |
| `fleetd/fleet_runtime/adapters/channel_client` | the client of the channel: 1 request, the peer check, the result after a lost reply, or a request file (fleetd-p5 ticket 15) | `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_client.rs` |
| `fleetd/fleet_runtime/adapters/job_result_file` | the job result file `<fleet root>/run/jobs/<job id>-<run>.json` (fleetd-p5 ticket 33) | `crates/horch-core/src/fleetd/fleet_runtime/adapters/job_result_file.rs` |
| `fleetd/fleet_runtime/adapters/polite_stops_store` | the `polite_stops` rows in SQLite (fleetd-p5 ticket 34) | `crates/horch-core/src/fleetd/fleet_runtime/adapters/polite_stops_store.rs` |
| `fleetd/task_execution/domain/outbox` | the outbox of an attempt: its files and the rules of a hand-off file (fleetd-p5 ticket 26) | `crates/horch-core/src/fleetd/task_execution/domain/outbox.rs` |
| `fleetd/task_execution/application/outbox_ingest` | the ingest of the outbox files of each live attempt (fleetd-p5 ticket 29) | `crates/horch-core/src/fleetd/task_execution/application/outbox_ingest.rs` |
| `fleetd/task_execution/adapters/outbox_reader` | reads the outbox files of an attempt with the owner and mode checks (fleetd-p5 ticket 29) | `crates/horch-core/src/fleetd/task_execution/adapters/outbox_reader.rs` |
| `fleetd/task_execution/adapters/outbox_writer` | the outbox writer of a worker command (fleetd-p5 ticket 26) | `crates/horch-core/src/fleetd/task_execution/adapters/outbox_writer.rs` |
| `fleetd/task_execution/adapters/attempt_files` | reads `heartbeat.json`, `exit.json` and `session.json` of an attempt (fleetd-p5 ticket 31) | `crates/horch-core/src/fleetd/task_execution/adapters/attempt_files.rs` |
| `fleetd/worker_profiles/domain/operator_boundary` | the operator boundary of a worker sandbox: the 3 denied operator acts and the writable paths (fleetd-p5 tickets 35 to 37) | `crates/horch-core/src/fleetd/worker_profiles/domain/operator_boundary.rs` |
| `fleetd/worker_profiles/adapters/sandbox_rules` | the sandbox rules of profile validation (fleetd-p5 ticket 38) | `crates/horch-core/src/fleetd/worker_profiles/adapters/sandbox_rules.rs` |
| `fleetd/ideas/domain/idea` | the idea entity: its 10 states and the exits of each state (fleetd-p6 ticket 04) | `crates/horch-core/src/fleetd/ideas/domain/idea.rs` |
| `fleetd/ideas/domain/idea_folder` | the idea folder: idea.md, the amendments, interview.md, assumptions.md, spec.md and requirements.yaml, and the checks of SR-6 on each file (fleetd-p6 ticket 07) | `crates/horch-core/src/fleetd/ideas/domain/idea_folder.rs` |
| `fleetd/ideas/domain/approval` | the approval of an idea: approval.json and the hash of the assumptions that you saw (fleetd-p6 ticket 18) | `crates/horch-core/src/fleetd/ideas/domain/approval.rs` |
| `fleetd/ideas/domain/plan` | plan.yaml: the schema of a task and the tickets in format C1 that it names (fleetd-p6 ticket 08) | `crates/horch-core/src/fleetd/ideas/domain/plan.rs` |
| `fleetd/ideas/domain/plan_validator` | the plan validator frame: it runs each plan rule and collects the verdict (fleetd-p6 ticket 08) | `crates/horch-core/src/fleetd/ideas/domain/plan_validator.rs` |
| `fleetd/ideas/domain/plan_rules_dag` | the DAG rule of the plan validator: the task dependencies have no cycle and name only tasks of the plan (fleetd-p6 ticket 08) | `crates/horch-core/src/fleetd/ideas/domain/plan_rules_dag.rs` |
| `fleetd/ideas/domain/plan_rules_coverage` | the coverage rule of the plan validator: each requirement has a task and cites an approved assumption or an answer (fleetd-p6 ticket 09) | `crates/horch-core/src/fleetd/ideas/domain/plan_rules_coverage.rs` |
| `fleetd/ideas/domain/plan_rules_profiles` | the profile rule of the plan validator: each profile exists, has a fallback and has a sandbox on this OS (fleetd-p6 ticket 10) | `crates/horch-core/src/fleetd/ideas/domain/plan_rules_profiles.rs` |
| `fleetd/ideas/domain/plan_rules_gates` | the gate rule of the plan validator: the project declares a fast gate and a full gate (fleetd-p6 ticket 10) | `crates/horch-core/src/fleetd/ideas/domain/plan_rules_gates.rs` |
| `fleetd/ideas/domain/plan_rules_judges` | the judge pairing rule of the plan validator: each task of a judgment kind has 1 judge task in another session (fleetd-p6 ticket 12) | `crates/horch-core/src/fleetd/ideas/domain/plan_rules_judges.rs` |
| `fleetd/ideas/domain/plan_rules_model_guide` | the model guide rule of the plan validator: the default model and effort, and the reason for each other choice (fleetd-p6 ticket 11) | `crates/horch-core/src/fleetd/ideas/domain/plan_rules_model_guide.rs` |
| `fleetd/ideas/domain/answer` | the 4 answer cases of a worker question: covered with a citation, revise, replan, or an own decision (fleetd-p6 ticket 32) | `crates/horch-core/src/fleetd/ideas/domain/answer.rs` |
| `fleetd/ideas/application/intake` | horch idea add: 1 immutable idea file and 1 captured row for each idea (fleetd-p6 ticket 16) | `crates/horch-core/src/fleetd/ideas/application/intake.rs` |
| `fleetd/ideas/application/direct` | horch idea add --direct: a plan of 1 work task, plus its judge task for a judgment kind (fleetd-p6 ticket 17) | `crates/horch-core/src/fleetd/ideas/application/direct.rs` |
| `fleetd/ideas/application/approval` | horch idea approve and revise: write approval.json, and hold an idea whose assumptions changed (I7) (fleetd-p6 ticket 18) | `crates/horch-core/src/fleetd/ideas/application/approval.rs` |
| `fleetd/ideas/application/amend` | horch idea amend: the amendment of an idea before approval, after approval and after merge (fleetd-p6 ticket 19) | `crates/horch-core/src/fleetd/ideas/application/amend.rs` |
| `fleetd/ideas/application/park_cancel` | horch idea park and cancel: rescue the worktrees first, then park or cancel the idea and its tasks (fleetd-p6 ticket 20) | `crates/horch-core/src/fleetd/ideas/application/park_cancel.rs` |
| `fleetd/ideas/application/dispatcher` | the dispatcher loop: each pass moves each idea to its next state when its files validate (fleetd-p6 ticket 26) | `crates/horch-core/src/fleetd/ideas/application/dispatcher.rs` |
| `fleetd/ideas/application/sub_orchestrators` | the sub-orchestrators: 1 for each open feature, a fresh process for each phase (fleetd-p6 ticket 28) | `crates/horch-core/src/fleetd/ideas/application/sub_orchestrators.rs` |
| `fleetd/ideas/application/waiting` | waiting on you: the paused lease, the idle stop, the reminders and the park after the configured days (fleetd-p6 ticket 29) | `crates/horch-core/src/fleetd/ideas/application/waiting.rs` |
| `fleetd/ideas/application/plan_commit` | plan commit: validate plan.yaml, write plan.verdict.json, and commit the tasks of a valid plan in 1 transaction (fleetd-p6 ticket 14) | `crates/horch-core/src/fleetd/ideas/application/plan_commit.rs` |
| `fleetd/ideas/application/replan` | the replan of a failed task: the sub-orchestrator replaces the task or sends it back 1 time, else it is escalated (fleetd-p6 ticket 31) | `crates/horch-core/src/fleetd/ideas/application/replan.rs` |
| `fleetd/ideas/application/rollup` | the roll-up of an idea: in_progress to integrating to merged, and the removal of the rescued trees of an ended idea (fleetd-p6 ticket 33) | `crates/horch-core/src/fleetd/ideas/application/rollup.rs` |
| `fleetd/ideas/application/question_flow` | the question flow: the sub-orchestrator of the feature owns each worker question, and an unanswered question escalates to you (fleetd-p6 ticket 30) | `crates/horch-core/src/fleetd/ideas/application/question_flow.rs` |
| `fleetd/ideas/application/answers` | the answer of a worker question in its 4 cases, and the decision log of the feature (fleetd-p6 ticket 32) | `crates/horch-core/src/fleetd/ideas/application/answers.rs` |
| `fleetd/ideas/application/notes_ingest` | the ingest of the notes of horch idea note: each question and answer file of an idea folder is ingested 1 time (fleetd-p6 ticket 25) | `crates/horch-core/src/fleetd/ideas/application/notes_ingest.rs` |
| `fleetd/ideas/application/queries` | horch ideas and horch idea show: the board of each idea, what it waits on, and the open needs-you items (fleetd-p6 ticket 24) | `crates/horch-core/src/fleetd/ideas/application/queries.rs` |
| `fleetd/ideas/application/release` | the release of Phase 6 at the first pass: the folder of each backfilled idea and the owner of each open question (fleetd-p6 ticket 50) | `crates/horch-core/src/fleetd/ideas/application/release.rs` |
| `fleetd/ideas/adapters/ideas_store` | the `ideas` rows in SQLite (fleetd-p6 ticket 04) | `crates/horch-core/src/fleetd/ideas/adapters/ideas_store.rs` |
| `fleetd/ideas/adapters/idea_folder_files` | the files of an idea folder: atomic writes and checked reads (fleetd-p6 ticket 07) | `crates/horch-core/src/fleetd/ideas/adapters/idea_folder_files.rs` |
| `fleetd/ideas/adapters/plan_yaml` | the reader of plan.yaml (fleetd-p6 ticket 08) | `crates/horch-core/src/fleetd/ideas/adapters/plan_yaml.rs` |
| `fleetd/ideas/adapters/idea_notes_reader` | the reader of the note files that horch idea note writes in an idea folder (fleetd-p6 ticket 25) | `crates/horch-core/src/fleetd/ideas/adapters/idea_notes_reader.rs` |
| `fleetd/ideas/adapters/needs_you_folder` | the needs-you folder `/fleet/needs-you/`: 1 file for each open item, removed when the item ends (fleetd-p6 ticket 21) | `crates/horch-core/src/fleetd/ideas/adapters/needs_you_folder.rs` |
| `fleetd/ideas/adapters/notify` | the herdr notification of a needs-you item (fleetd-p6 ticket 21) | `crates/horch-core/src/fleetd/ideas/adapters/notify.rs` |
| `fleetd/integration/domain/trial_merge` | a trial merge, keyed by its 2 tips: a feature against the primary branch, or 2 features that share a file (fleetd-p6 ticket 36) | `crates/horch-core/src/fleetd/integration/domain/trial_merge.rs` |
| `fleetd/integration/domain/stack` | a stack: a conflicting trial merge puts the younger feature on the older one, 1 time, with 1 resolver task (fleetd-p6 ticket 37) | `crates/horch-core/src/fleetd/integration/domain/stack.rs` |
| `fleetd/integration/application/new_features` | new features: a feature row for each idea with no --to, its internal run and a checked kebab-case branch name (fleetd-p6 ticket 15) | `crates/horch-core/src/fleetd/integration/application/new_features.rs` |
| `fleetd/integration/application/trial_merges` | the trial merges: queue a trial_merge job for each pair that needs one and record its outcome (fleetd-p6 ticket 36) | `crates/horch-core/src/fleetd/integration/application/trial_merges.rs` |
| `fleetd/integration/application/stacks` | the stacks: stack the younger feature of a conflict and make its resolver task (fleetd-p6 ticket 37) | `crates/horch-core/src/fleetd/integration/application/stacks.rs` |
| `fleetd/integration/application/stack_edges` | the end of a stack edge: clear stacked_on, write a rescue ref, rebase the upper feature and run both gates again (fleetd-p6 ticket 38) | `crates/horch-core/src/fleetd/integration/application/stack_edges.rs` |
| `fleetd/integration/application/merged_with_ideas` | a merge with ideas in flight: record the feature as merged and move each idea in flight to a new feature (fleetd-p6 ticket 35) | `crates/horch-core/src/fleetd/integration/application/merged_with_ideas.rs` |
| `fleetd/integration/application/judge_branch` | the judge branch in a fix round: a rescue ref of the judge branch, then the move onto the new tip of the judged branch (fleetd-p6 ticket 42) | `crates/horch-core/src/fleetd/integration/application/judge_branch.rs` |
| `fleetd/integration/adapters/exec_trial_merge` | the executor of the trial_merge job (fleetd-p6 ticket 36) | `crates/horch-core/src/fleetd/integration/adapters/exec_trial_merge.rs` |
| `fleetd/integration/adapters/trial_merges_store` | the `trial_merges` rows in SQLite (fleetd-p6 ticket 36) | `crates/horch-core/src/fleetd/integration/adapters/trial_merges_store.rs` |
| `fleetd/task_execution/domain/judge_pair` | the verdict file of a judge task: 1 verdict, each finding ranked, and the outcome critical_open (fleetd-p6 ticket 39) | `crates/horch-core/src/fleetd/task_execution/domain/judge_pair.rs` |
| `fleetd/task_execution/application/judge_pair` | the judge pair in the verifier: the judged task waits in verifying, a CRITICAL item starts a fix round (fleetd-p6 ticket 41) | `crates/horch-core/src/fleetd/task_execution/application/judge_pair.rs` |
| `fleetd/task_execution/application/planned_tasks` | the tasks of a committed or replanned plan, with the idea of each task and the requirements that it covers (fleetd-p6 ticket 14) | `crates/horch-core/src/fleetd/task_execution/application/planned_tasks.rs` |
| `fleetd/fleet_runtime/application/handlers_idea` | the handlers of the idea request kinds: each operator request of horch idea reaches its use case (fleetd-p6 ticket 22) | `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_idea.rs` |
| `fleetd/worker_profiles/domain/conformance` | the 10 checks of the adapter conformance suite, the verdict of 1 check and the form of `conformance.json` (fleetd-p7 tickets 23 and 24) | `crates/horch-core/src/fleetd/worker_profiles/domain/conformance.rs` |
| `fleetd/worker_profiles/domain/health` | the health of a worker profile from its last `profile_unhealthy` or `profile_healthy` event (fleetd-p7 ticket 26) | `crates/horch-core/src/fleetd/worker_profiles/domain/health.rs` |
| `fleetd/worker_profiles/domain/docs_jobs` | when fleetd queues a `docs_sync` or a `conformance` run (fleetd-p7 ticket 28) | `crates/horch-core/src/fleetd/worker_profiles/domain/docs_jobs.rs` |
| `fleetd/worker_profiles/application/conformance` | run the conformance suite of an adapter through a port (fleetd-p7 tickets 23 and 24) | `crates/horch-core/src/fleetd/worker_profiles/application/conformance.rs` |
| `fleetd/worker_profiles/application/health` | mark a worker profile unhealthy or healthy (fleetd-p7 ticket 26) | `crates/horch-core/src/fleetd/worker_profiles/application/health.rs` |
| `fleetd/worker_profiles/application/docs_jobs` | queue the `docs_sync` and `conformance` jobs through `jobs::queue` (fleetd-p7 ticket 28) | `crates/horch-core/src/fleetd/worker_profiles/application/docs_jobs.rs` |
| `fleetd/worker_profiles/application/docs_sync` | the port of the docs sync executor, because horch-core has no HTTP crate (fleetd-p7 ticket 22) | `crates/horch-core/src/fleetd/worker_profiles/application/docs_sync.rs` |
| `fleetd/worker_profiles/application/drift_idea` | a docs drift or a failed conformance check becomes an idea (fleetd-p7 ticket 27) | `crates/horch-core/src/fleetd/worker_profiles/application/drift_idea.rs` |
| `fleetd/worker_profiles/adapters/conformance_runner` | the real launch of each conformance check through the launcher (fleetd-p7 tickets 23 and 24) | `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs` |
| `fleetd/worker_profiles/adapters/docs_dir` | the docs files of a CLI under `<fleet root>/docs/<cli>/<version>/` (fleetd-p7 tickets 22 and 25) | `crates/horch-core/src/fleetd/worker_profiles/adapters/docs_dir.rs` |
| `fleetd/task_execution/domain/context_limit` | the outcome `context_limit` of an attempt and its loop guard (fleetd-p7 ticket 18) | `crates/horch-core/src/fleetd/task_execution/domain/context_limit.rs` |
| `compaction` | context windows, thresholds, row states and the compaction job | `crates/horch-core/src/compaction/policy.rs`, `crates/horch-core/src/compaction/job.rs` |
| `workspace` | the herdr client, layout, tiling and balancing | `crates/horch-core/src/workspace/herdr.rs`, `crates/horch-core/src/workspace/tile.rs` |
| `telemetry` | live per-pane token telemetry and the collector | `crates/horch-core/src/telemetry/collect.rs`, `crates/horch-core/src/telemetry/readers.rs` |
| `usage` | what a run cost, read back from each harness's own records | `crates/horch-core/src/usage.rs` |
| `vcs` | typed git and landing | `crates/horch-core/src/vcs/git.rs` |
| `digest` | sha256 digests over canonical JSON | `crates/horch-core/src/digest.rs` |
| `testkit` | the in-repo PRNG and property-test runner | `crates/horch-core/src/testkit.rs` |

The harness kinds are in `harness/mod.rs` (`HarnessKind`): `claude`, `codex`,
`opencode`, `pi`, `prime`, `antigravity`, and `none` (the smoke fake). Each
real kind has one adapter file in `harness/` and one `Capabilities` constant
in `harness/capabilities.rs`.

## The CLI layer

`crates/horch/src/cmd/` holds 1 file per `horch` command group:

| file | command |
|---|---|
| `agentlist.rs` | `horch agent-list` |
| `balancecmd.rs` | `horch balance` |
| `branchcmd.rs` | `horch branch abandon` (not built yet) |
| `channel.rs` | `horch channel serve` (hidden; the worker-report channel relay, an MCP stdio server) |
| `compact.rs` | `horch compact` |
| `context.rs` | `horch context` |
| `cost.rs` | `horch cost` |
| `cost/` | the report tables and rendering of `horch cost` |
| `delivery_probe.rs` | the delivery checks of `horch fleet` and `horch doctor` |
| `docscmd.rs` | `horch docs sync` (the pages, manifest and CHANGES.md of each CLI; also the docs runner of the `docs_sync` job) |
| `doctor.rs` | `horch doctor` |
| `featurecmd.rs` | `horch feature abandon` (not built yet) |
| `fleetd.rs` | `horch fleetd` (the fleet daemon: the composition root of its pass) and `horch fleetd check` (restarts a gone, hung or older fleetd; each `horch` command makes the same check first) |
| `idea/` | `horch idea` and `horch ideas`: 1 file for each subcommand; `mod.rs` holds the subcommand tree |
| `idea/add.rs` | `horch idea add <text> \| -f <file> \| --direct <ticket file>` (not built yet) |
| `idea/amend.rs` | `horch idea amend` (not built yet) |
| `idea/approve.rs` | `horch idea approve` (not built yet) |
| `idea/board.rs` | `horch ideas` (not built yet) |
| `idea/cancel.rs` | `horch idea cancel` (not built yet) |
| `idea/note.rs` | `horch idea note` (not built yet) |
| `idea/park.rs` | `horch idea park` (not built yet) |
| `idea/revise.rs` | `horch idea revise` (not built yet) |
| `idea/show.rs` | `horch idea show` (not built yet) |
| `install.rs` | `horch install` (copies `horch`, and deletes a retired `multi-herdr-dataset`) |
| `jobcmd.rs` | `horch job requeue`, and `horch job run` (hidden; the job process that fleetd starts) |
| `layoutcmd.rs` | `horch layout` |
| `sessionscmd.rs` | `horch sessions`, `horch ledger legacy` (the old session-ledger plumbing) |
| `mail.rs` | `horch mail courier` (hidden; delivers durable worker mail) |
| `marketplacecmd.rs` | `horch marketplace` |
| `messaging.rs` | `horch tell`, `inbox`, `peek`, `note`, `done` |
| `mod.rs` | the module list and `load_roster` |
| `mode.rs` | `horch mode` (switches a Codex pane between write and plan) |
| `operator_request.rs` | 1 operator request of a command to fleetd: the send, and the exit of an applied, refused or queued request (RR-5) |
| `quotacmd.rs` | `horch quota` |
| `recipes.rs` | `horch fleet`, `horch orchestration`, `horch pane-launch` (hidden) |
| `rescuecmd.rs` | `horch rescue drop` (not built yet) |
| `requestcmd.rs` | `horch request <request_id>`: the state and the result of 1 request, read-only (RR-8) |
| `restore.rs` | herdr's agent session restore: the restored pane's identity, and the warning in `fleet` and `doctor` |
| `resultcmd.rs` | `horch result`: writes `result.json` of an attempt; `--check` tests the Stop hooks (not built yet) |
| `route.rs` | `horch route` |
| `selfupdate.rs` | `horch self-update`, and the stale-version warning at boot |
| `skillscmd.rs` | `horch skills` |
| `smoke.rs` | `horch smoke messaging`, `fleet`, `tile` |
| `spawn.rs` | `horch spawn` |
| `taskcmd.rs` | `horch task requeue` and `horch task cancel`; `horch ledger requeue` and `cancel` run the same code |
| `taskledger.rs` | `horch ledger` (the SQLite task ledger): every subcommand except `legacy` |
| `taskledger_finish.rs` | `horch ledger finish`: the finish steps in the foreground or detached, and the mail with the result of a detached finish |
| `taskledger_spawn.rs` | the task-ledger side of `horch spawn`: the worker row and its id in the brief |
| `taskledger_wake.rs` | the wake lines the task ledger sends the orchestrator |
| `taskledger_watch.rs` | `horch ledger watch` (the watchdog) and its restart when it goes quiet |
| `teammatescmd.rs` | `horch teammates` |
| `telemetry/` | `horch telemetry` |
| `tilecmd.rs` | `horch tile` |
| `update.rs` | `horch update`, and the update step of `fleet` and `orchestration` |
| `usagecmd.rs` | `horch usage` |
| `worker.rs` | `horch worker` (hidden; runs inside a spawned pane) |
| `worktree.rs` | `horch worktree new`, `land`, `list` (outside a ledger run); `horch worktree triage`, `triage apply`, `restore` and `horch ledger gc` (#308) |

Shared files: `crates/horch/src/bootstrap.rs` (the one read of the process
environment), `crates/horch/src/exit.rs` (exit codes), `crates/horch/src/output.rs`
(stdout without a panic on a closed pipe).

## Where state lives

| what | path | source |
|---|---|---|
| state root | `$HORCH_STATE_DIR`, else `${XDG_STATE_HOME:-~/.local/state}/horch` | `runtime/paths.rs` `state_root` |
| session records | the `session_records` table of the task ledger (`<state root>/ledger.sqlite3` when `HORCH_STATE_DIR` is set); an old `<state root>/<slug of the project path>.json` is read only | `execution/store.rs` `ExecutionStore::for_project` |
| task ledger | `$HORCH_LEDGER_DB`, else `~/.horch/ledger.sqlite3`, 1 file per machine | `taskledger/store.rs` `default_path` |
| skill bundles per launch | `<state root>/skill-bundles/<execution id>/` | `skills.rs`, `harness/launch.rs` |
| marketplace skills (data root) | `$HORCH_DATA_DIR`, else `${XDG_DATA_HOME:-~/.local/share}/horch/` | `runtime/paths.rs` `data_root` |

`horch ledger legacy path` prints the old JSON session ledger path for the current project.
`horch ledger path` prints the task ledger path.

A herdr pane does not inherit the environment of the `horch` that split it.
So every horch process in a fleet gets the spawner's data root this way:

- The orchestrator pane command sets `HORCH_DATA_DIR` (`cmd/recipes.rs`
  `pane_command_for`, through `PaneShell::command_line_with_env`).
  `pane-launch` and its agent inherit it.
- Every other pane command sets `HORCH_DATA_DIR` the same way (`execution/service.rs`
  `run`, through `PaneShell::command_line_with_env`). The brief carries no
  `data_root` field. `horch worker` and its agent inherit the variable. See
  [command-flow.md](command-flow.md), section 2.

## How teammates and skills get into the binary

`crates/horch-core/build.rs` globs `teammates/` (every file without a leading
`_`, plus `teammates/_base/` and `teammates/_template.md`) and every file
under `skills/`, and compiles them in. Adding a file adds a teammate or a
skill. There is no Rust list to edit. An installed `horch` works without a
checkout.

At run time the roster is layered (`load_roster` in `crates/horch/src/cmd/mod.rs`):

1. the compiled-in files;
2. `~/.config/horch/teammates`;
3. `$HORCH_TEAMMATES_DIR`;
4. an explicit directory. A pane is a fresh shell that does not inherit
   `$HORCH_TEAMMATES_DIR`, so `horch fleet` passes the path to the pane
   outright (`roster/repository.rs` `load_layered`).

A later layer overrides an earlier one for the same teammate name. The `justfile` exports
`HORCH_TEAMMATES_DIR` to the repo's `teammates/`, so `just` recipes always use
your edits.

## Read next

- [command-flow.md](command-flow.md): the commands as diagrams.
- [testing-and-gates.md](testing-and-gates.md): the gate, oracles and fakes.
- The recipes in [README.md](README.md#recipes).
- The spec: [specs/architecture.md](specs/architecture.md).
