<!-- horch-ticket: 1 -->
# Fix the red tests of `crates/horch-e2e/tests/lifecycle.rs`

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --test lifecycle --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/lifecycle.rs`

## Goal

Make the 14 red tests of `crates/horch-e2e/tests/lifecycle.rs` from `docs/specs/fleetd/gates/baseline-2026-11.md` pass without weakening any assert.

## Read first

- `docs/specs/fleetd/gates/baseline-2026-11.md`: section `## Failing tests` (`horch-e2e`, `tests/lifecycle.rs`).
- `crates/horch-e2e/tests/lifecycle.rs`.
- `crates/horch-e2e/src/ledger_seed.rs` and `git show 7532a60d` (how e2e tests seed the ledger and start `h.serve_fleetd()`).
- `crates/horch-e2e/tests/fleet18_messaging.rs` (`g60_missing_registration_still_queues_worker_mail`, a green test that sets `HORCH_LEDGER_DB` to `h.root.join("ledger.sqlite3")` and calls `h.serve_fleetd()`).

## Failing tests and their first error line from the baseline

1. `antigravity_child_never_receives_a_forbidden_key` — `child environment kept a forbidden key: calls=[Object {"argv": Array [String("--print-logs"), String("--workspace"), String("."), String("--mode"), String("agent"), String("--no-color"), String("--output-format"), String("json"), String("Read the file at /private/var/folders/lr/fc3461w92f91125m79233x540000gn/T/horch-e2e-lc-agy-env-sJ1G1g/project/.horch/state/prompts/antigravity-1-1795046910327-79051.md and follow its instructions.")], "bin": String("agy"), "cwd": String("/private/var/folders/lr/fc3461w92f91125m79233x540000gn/T/horch-e2e-lc-agy-env-sJ1G1g/project"), "env": Object {"ANTHROPIC_API_KEY": String("sk-ant-e2e-must-not-reach-worker"), "CLAUDECODE": Null, "CODEX_SANDBOX": Null, "CODEX_SANDBOX_NETWORK_DISABLED": Null, "CODEX_THREAD_ID": Null, "GEMINI_API_KEY": String("e2e-gemini-must-not-reach-worker"), "HOME": String("/private/var/folders/lr/fc3461w92f91125m79233x540000gn/T/horch-e2e-lc-agy-env-sJ1G1g/home"), "HORCH_AGENT_BIN": Null, "HORCH_CALL_LOG": String("/private/var/folders/lr/fc3461w92f91125m79233x540000gn/T/horch-e2e-lc-agy-env-sJ1G1g/calls.jsonl"), "HORCH_E2E_BIN_DIR": String("/Users/matthewscott/repo/multi-herdr/target/debug/deps"), "HORCH_FAKE_SCENARIO": String("happy"), "HORCH_HERDR_SOCK": String("/private/var/folders/lr/fc3461w92f91125m79233x540000gn/T/horch-e2e-lc-agy-env-sJ1G1g/herdr.sock"), "HORCH_PROJECT_DIR": String("/private/var/folders/lr/fc3461w92f91125m79233x540000gn/T/horch-e2e-lc-agy-env-sJ1G1g/project"), "HORCH_ROLE": String("antigravity-1"), "HORCH_SESSION_ID": Null, "HORCH_SPAWN_TOKEN": Null, "HORCH_TEAMMATE": String("antigravity"), "OPENAI_API_KEY": String("sk-openai-e2e-must-not-reach-worker"), "OPENCODE_API_KEY": String("e2e-opencode-must-not-reach-worker"), "PATH": String("/private/var/folders/lr/fc3461w92f91125m79233x540000gn/T/horch-e2e-lc-agy-env-sJ1G1g/bin:/Users/matthewscott/.cargo/bin:/Users/matthewscott/.local/bin:/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"), "TMPDIR": String("/var/folders/lr/fc3461w92f91125m79233x540000gn/T/")}, "extra": Object {}, "pid": Number(79060), "ts_ms": Number(1795046910428)}]`
2. `arc_26_e2e_lifecycle_matrix_antigravity` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
3. `arc_26_e2e_lifecycle_matrix_claude` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
4. `arc_26_e2e_lifecycle_matrix_codex` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
5. `arc_26_e2e_lifecycle_matrix_opencode` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
6. `arc_26_e2e_lifecycle_matrix_pi` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
7. `arc_26_e2e_lifecycle_matrix_prime` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
8. `pane_command_removes_a_forbidden_key_before_horch_starts` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
9. `spawn_fails_when_a_project_teammate_file_does_not_load` — ` stderr: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
10. `spawn_warns_about_a_user_teammate_file_that_does_not_load` — `spawn: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
11. `tel_session_recorded_when_done_killed_by_pane_close` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
12. `tel_session_recorded_when_done_not_called` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
13. `tel_session_recorded_when_done_succeeds` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
14. `worker_warns_once_about_a_teammate_file_that_does_not_load` — `spawn: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`

## What to do for each cause class

- **The retired command `horch ledger run new` or `horch ledger task`:** migrate to `horch_e2e::ledger_seed`, or to the idea flow when the test is about the command.
- **`reading the operator token ... No such file`:** the test needs a fleet root with a token; use the harness helper `h.serve_fleetd()` (with `HORCH_LEDGER_DB` set to `h.root.join("ledger.sqlite3")`) that other green tests use (for example `g60_missing_registration_still_queues_worker_mail` in `crates/horch-e2e/tests/fleet18_messaging.rs` or `world` in `crates/horch-e2e/tests/taskledger.rs`).
- **A behavior that Phase 6 removed on purpose (`horch spawn` retired in Phase 6 under `FR-5`, ticket `47-c6-t908-retire-worker-run-cli.md`):**
  - Where a test checks `horch worker` launch / lifecycle / child environment scrubbing / teammate loading / session recording (`antigravity_child_never_receives_a_forbidden_key`, `arc_26_e2e_lifecycle_matrix_*`, `pane_command_removes_a_forbidden_key_before_horch_starts`, `tel_session_recorded_when_*`, `worker_warns_once_about_a_teammate_file_that_does_not_load`), check whether the test fixture can drive the worker launch through the current worker / `fleetd` flow with `h.serve_fleetd()`. Note that `antigravity_child_never_receives_a_forbidden_key` also intersects with ticket `05-c2-t899-wrapper-operator.md` (env scrubbing in the launch wrapper).
  - Where a test is solely about the retired `horch spawn` CLI command (`spawn_fails_when_a_project_teammate_file_does_not_load`, `spawn_warns_about_a_user_teammate_file_that_does_not_load`, or any `horch spawn`-only check), do **not** delete the test on your own decision; send `QUESTION:` to the orchestrator with the design rule (`FR-5`, Phase 6 ticket `47-c6-t908-retire-worker-run-cli.md`) and a recommended option.
- **A time-out or a result that changes between runs:** the baseline ran at a load average of 16 to 26; run the test 3 times, find the cause, fix it; a flaky test is a bug.
- Never weaken an assert. Do not edit product code; product code is out of scope for this ticket.

## Done when

Each of the 14 tests passes 3 times in a row (or has an orchestrator decision on a `QUESTION:` for tests of retired `horch spawn` behavior), and the Check passes.

## Rules


1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.
