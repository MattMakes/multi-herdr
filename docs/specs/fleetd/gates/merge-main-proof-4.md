# E1 proof 4: the merge of origin/main 1f03526b (the comment ban)

Task T-221 (E1) wrote this file. It lists each conflict of
`git merge --no-ff origin/main` and its resolution.

## Commits on the run branch before the merge

| commit | what |
|---|---|
| `b38771c9` | `git merge --no-ff keep/fleetd-finish-trial-merge-main` (main to v0.5.2 `5863c698`). 1 conflict: `crates/horch-core/src/execution/service.rs`. Kept the role retry of T-400 and added `plan.execution.backend = self.ctx.herdr.backend` of main. `crates/horch-core/tests/r_execution_service.rs`: the new T-400 test got `open_pane: None`, which the hc change of main adds to `ExecutionService`. |
| `b2ecc396` | `git checkout origin/main -- scripts/comments`, and `comment_lint.py list` saved to `comments-before-the-ban.jsonl` (51829 comments in 1237 files). |
| `477ef509` | `comment_lint.py strip` on 1236 files (51724 comment lines). `scripts/live/context.sh` is not stripped: `crates/horch/tests/context_cli.rs` finds its steps by banner. `verify --base HEAD` gave 0 code changes before `cargo fmt`. After `cargo fmt`, 42 files differ only by whitespace, trailing commas, closure braces and import order (checked with a script). |

## Method for the conflicts

`git merge --no-ff origin/main` gave 144 conflicts. For each file, a script compared
the code of `5863c698:<file>` and `1f03526b:<file>` without comments and without
whitespace (`comment_lint.code_only`).

- 138 files: `1f03526b` changed only comments. Resolution: the run branch (`--ours`).
  2 of them the run branch deleted (`DU`), and they stay deleted.
- 6 files: `1f03526b` changed code. Resolution: the run branch, plus each code change of main, by hand.

## The 6 files with a code change of main

| file | code change of main | resolution |
|---|---|---|
| `crates/horch-core/src/roster/validation.rs` | count 82 to 83: main adds the teammate `comment-purger` | run branch count 83 + 1 = 84 |
| `crates/horch-core/src/taskledger/tasks.rs` | rustfmt layout of `let blocker` only | run branch (the same code) |
| `crates/horch-core/tests/baseline_oracles.rs` | `"comment-purger"` in `SKIP_NEW_TEAMMATES` | run branch, plus that line |
| `crates/horch-e2e/tests/fleet18_brief_worktree.rs` | `names.len()` 88 to 89 (`comment-purger`) | run branch, 88 to 89 |
| `crates/horch/tests/context_cli.rs` | constants lose their trailing comments; the c3 assert reads `json.dumps({"type": "compact"})`, because the old text stood only in a comment of `context.sh` | run branch, plus the new c3 assert |
| `scripts/phase-gate.sh` | the function and the step `comment_ban` | run branch, plus both |

## The 138 files with only comment changes of main

- `Cargo.toml`
- `crates/herdr-docs-sync/src/main.rs`
- `crates/herdr-install/src/place.rs`
- `crates/herdr-install/src/win.rs`
- `crates/horch-core/Cargo.toml`
- `crates/horch-core/src/execution/lifecycle.rs`
- `crates/horch-core/src/execution/model.rs`
- `crates/horch-core/src/execution/plan.rs`
- `crates/horch-core/src/execution/records.rs`
- `crates/horch-core/src/execution/service.rs`
- `crates/horch-core/src/execution/store.rs`
- `crates/horch-core/src/fsx.rs`
- `crates/horch-core/src/harness/antigravity.rs`
- `crates/horch-core/src/harness/claude.rs`
- `crates/horch-core/src/harness/codex.rs`
- `crates/horch-core/src/harness/launch.rs`
- `crates/horch-core/src/harness/none.rs`
- `crates/horch-core/src/harness/opencode.rs`
- `crates/horch-core/src/harness/pi.rs`
- `crates/horch-core/src/harness/prime.rs`
- `crates/horch-core/src/harness/trust.rs`
- `crates/horch-core/src/harness/update.rs`
- `crates/horch-core/src/messaging/brief.rs`
- `crates/horch-core/src/messaging/channel.rs`
- `crates/horch-core/src/messaging/delivery.rs`
- `crates/horch-core/src/messaging/report.rs`
- `crates/horch-core/src/procid.rs`
- `crates/horch-core/src/project_notes.rs`
- `crates/horch-core/src/roster/teammate.rs`
- `crates/horch-core/src/runtime/bins.rs`
- `crates/horch-core/src/runtime/context.rs`
- `crates/horch-core/src/runtime/machine.rs`
- `crates/horch-core/src/runtime/paths.rs`
- `crates/horch-core/src/runtime/process.rs`
- `crates/horch-core/src/skills/materialize.rs`
- `crates/horch-core/src/taskledger/finish/complexity.rs`
- `crates/horch-core/src/taskledger/finish/land.rs`
- `crates/horch-core/src/taskledger/finish/mod.rs`
- `crates/horch-core/src/taskledger/finish/pr.rs`
- `crates/horch-core/src/taskledger/import.rs`
- `crates/horch-core/src/taskledger/map.rs`
- `crates/horch-core/src/taskledger/orchestrator.rs`
- `crates/horch-core/src/taskledger/outbox.rs`
- `crates/horch-core/src/taskledger/runs.rs`
- `crates/horch-core/src/taskledger/schema.rs`
- `crates/horch-core/src/taskledger/sql_words.rs`
- `crates/horch-core/src/taskledger/status.rs`
- `crates/horch-core/src/taskledger/stop.rs`
- `crates/horch-core/src/taskledger/store.rs`
- `crates/horch-core/src/taskledger/supervisor.rs`
- `crates/horch-core/src/taskledger/triage/apply.rs`
- `crates/horch-core/src/taskledger/triage/facts.rs`
- `crates/horch-core/src/taskledger/triage/mod.rs`
- `crates/horch-core/src/taskledger/triage/refs.rs`
- `crates/horch-core/src/taskledger/util.rs`
- `crates/horch-core/src/taskledger/watchdog.rs`
- `crates/horch-core/src/taskledger/workers.rs`
- `crates/horch-core/src/taskledger/worktree_ops.rs`
- `crates/horch-core/src/telemetry/lock.rs`
- `crates/horch-core/src/vcs/network.rs`
- `crates/horch-core/src/workspace/exec.rs`
- `crates/horch-core/src/workspace/herdr.rs`
- `crates/horch-core/tests/arch_scan.rs`
- `crates/horch-core/tests/common/warm.rs`
- `crates/horch-core/tests/context_launch.rs`
- `crates/horch-core/tests/context_prime.rs`
- `crates/horch-core/tests/execution_plan.rs`
- `crates/horch-core/tests/execution_store.rs`
- `crates/horch-core/tests/golden_prompts.rs`
- `crates/horch-core/tests/harness_defaults.rs`
- `crates/horch-core/tests/i308_triage_apply.rs`
- `crates/horch-core/tests/nfr.rs`
- `crates/horch-core/tests/r_workspace_arrange.rs`
- `crates/horch-core/tests/taskledger_queue.rs`
- `crates/horch-core/tests/telemetry_collect.rs`
- `crates/horch-core/tests/trf_02_ticket_template.rs`
- `crates/horch-core/tests/trf_27_review_loop.rs`
- `crates/horch-core/tests/trf_48_ledger_guard.rs`
- `crates/horch-core/tests/w3_finish_steps.rs`
- `crates/horch-core/tests/w3_taskledger_core_tasks.rs`
- `crates/horch-e2e/Cargo.toml`
- `crates/horch-e2e/src/bin/fake-codex.rs`
- `crates/horch-e2e/src/bin/fake-herdr.rs`
- `crates/horch-e2e/src/bin/fake-opencode.rs`
- `crates/horch-e2e/src/bin/fake-prime.rs`
- `crates/horch-e2e/src/harness.rs`
- `crates/horch-e2e/src/lib.rs`
- `crates/horch-e2e/src/process.rs`
- `crates/horch-e2e/tests/brief.rs`
- `crates/horch-e2e/tests/done_budget.rs`
- `crates/horch-e2e/tests/e2e.rs`
- `crates/horch-e2e/tests/fleet18_messaging.rs`
- `crates/horch-e2e/tests/fleet18_observability.rs`
- `crates/horch-e2e/tests/fleet_start.rs`
- `crates/horch-e2e/tests/hc_fleet_start.rs`
- `crates/horch-e2e/tests/i308_fleet_steward.rs`
- `crates/horch-e2e/tests/i308_triage.rs`
- `crates/horch-e2e/tests/lifecycle.rs`
- `crates/horch-e2e/tests/messaging.rs`
- `crates/horch-e2e/tests/relay_first_delivery.rs`
- `crates/horch-e2e/tests/restored_pane.rs`
- `crates/horch-e2e/tests/routing_provenance.rs`
- `crates/horch-e2e/tests/scenario.rs`
- `crates/horch-e2e/tests/skill_intake_spawn.rs`
- `crates/horch-e2e/tests/skills_exposure.rs`
- `crates/horch-e2e/tests/startup_error.rs`
- `crates/horch-e2e/tests/taskledger.rs`
- `crates/horch-e2e/tests/tell_grace.rs`
- `crates/horch-e2e/tests/watchdog_liveness.rs`
- `crates/horch-e2e/tests/worktree.rs`
- `crates/horch-marketplace/src/git.rs`
- `crates/horch-marketplace/src/model.rs`
- `crates/horch-marketplace/src/source.rs`
- `crates/horch-marketplace/tests/marketplace.rs`
- `crates/horch/Cargo.toml`
- `crates/horch/src/cmd/compact.rs`
- `crates/horch/src/cmd/delivery_probe.rs`
- `crates/horch/src/cmd/install.rs`
- `crates/horch/src/cmd/mail.rs`
- `crates/horch/src/cmd/messaging.rs`
- `crates/horch/src/cmd/recipes.rs`
- `crates/horch/src/cmd/selfupdate.rs`
- `crates/horch/src/cmd/smoke.rs`
- `crates/horch/src/cmd/spawn.rs`
- `crates/horch/src/cmd/taskledger.rs`
- `crates/horch/src/cmd/taskledger_finish.rs`
- `crates/horch/src/cmd/taskledger_spawn.rs`
- `crates/horch/src/cmd/taskledger_wake.rs`
- `crates/horch/src/cmd/taskledger_watch.rs`
- `crates/horch/src/cmd/telemetry/mod.rs`
- `crates/horch/src/cmd/worker.rs`
- `crates/horch/src/cmd/worktree.rs`
- `crates/horch/src/main.rs`
- `crates/horch/tests/ledger_cli.rs`
- `crates/horch/tests/trf_11_add_map_cli.rs`
- `crates/horch/tests/trf_37_ledger_handoff_cli.rs`
- `crates/horch/tests/w3_ledger_cli_pins.rs`
- `justfile`
