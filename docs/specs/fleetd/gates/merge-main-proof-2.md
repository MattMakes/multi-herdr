# E1 proof 2: origin/main at v0.5.2 in the trial merge

Task T-330 (E1-prep-3) brought the 3 new commits of `origin/main` into the trial
merge of `merge-main-plan.md` (T-312) and `merge-main-proof.md` (T-314). Each
claim has a label: **verified** (with the evidence) or **not verified**.

## Tip of the trial branch

| ref | commit | note |
|---|---|---|
| `keep/fleetd-finish-trial-merge-main` | `3fd534f7` | the result of T-330. Local branch, not pushed |
| trial branch `trial-proof-2` | `3fd534f7` | 7 fix commits on top of `04ab28c2` |
| merge of `origin/main` | `04ab28c2` | parents `094d0de1` and `5863c698` (`origin/main`, Release v0.5.2) |
| merge of the run branch | `094d0de1` | parents `1e782060` (the trial of T-314) and `4e806d0b` (`fleet/fleetd-finish` at the start of T-330). No conflict |

Verified:
- `git merge-base --is-ancestor origin/main keep/fleetd-finish-trial-merge-main` exits 0.
- `git merge-base --is-ancestor 4e806d0b trial-proof-2` and `git merge-base --is-ancestor 1e782060 trial-proof-2` exit 0.
- `git log --format='%h %p'` gives the parents above.

The run branch moved during T-330: `fleet/fleetd-finish` is now `437190bc`, 1
commit after `4e806d0b`. It adds only `docs/specs/fleetd/gates/ci-run-4.md`.
`git merge-tree --write-tree fleet/fleetd-finish trial-proof-2` exits 0 (no
conflict): verified.

## Commits of main

| commit | PR | what it changes and why |
|---|---|---|
| `c28e6dd6` | #364 | Each tool that starts Google Chrome passes `--disable-features=MacAppCodeSignClone`: `render_frames.mjs`, `text_audit.mjs`, `capture.mjs`, `materials.mjs`, `check_page.py`, `ui-prop-tester/src/browser/process.rs`, and the hand-run commands of 2 video skills. Reason: a Chrome killed during its start left a 2 GB `code_sign_clone` copy, and macOS recorded each copy in its Local Network privacy list, which broke the operator's Chrome on the LAN. CHANGELOG entry under v0.5.2. |
| `50e63b2a` | #363 | A second pane host: `herdr-core` (`hc`) plus the Herdr desktop app (`herdr-app`), behind 1 handle `PaneHost` (`workspace/host.rs`). New: `runtime/backend.rs` (resolution: `--backend`, `HORCH_BACKEND`, pane identity, else `herdr`), `workspace/hc.rs` (the `HcApp` adapter), `workspace/exec.rs` (the bounded call code, moved out of `herdr.rs`), `workspace/place.rs` (spawn-time placement), schema version 9 (`workers.backend`), `backend` on `Brief`, execution records and worker rows, `LaunchEnv.host_sockets` in place of `herdr_socket`, `--no-daemon` for Codex on `hc`, `HC_STATUS_HARNESSES`, `horch doctor` checks of the host, the fakes `fake-hc` and `fake-herdr-app`, 6 new e2e test files, ADR 0003. Reason: run fleets inside the Herdr desktop app beside `herdr` fleets. |
| `5863c698` | - | Release v0.5.2: version, `Cargo.lock`, CHANGELOG heading. |

Verified: `git show --stat` of each commit, `gh pr view 363`, `gh pr view 364`.

## Conflicts

34 files have content conflicts (`UU`), 70 hunks. "ours" is the trial (the run
branch), "theirs" is main. State **R**: resolved, and the trial compiles.
Each rule comes from reading both sides with `git checkout --conflict=diff3`:
verified.

| # | file | rule | state |
|---|---|---|---|
| 1 | `Cargo.lock` | Ours: the run branch renamed `herdr-docs-sync`. The workspace crates `agent-docs-sync` and `horch-platform` get version `0.5.2` by hand, as main's other crates. `cargo check --locked` accepts the file | R |
| 2 | `README.md` | Ours: the hunk is the retired orchestrator text. Main's pane-host text outside the hunk merged without a conflict: see "For the operator" item 3 | R |
| 3 | `execution/lifecycle.rs` | Both: ours keeps `ledger`, `resume`, `run_launch`. Main's `herdr()` returns `PaneHost::of`. 3 `Brief` literals get `task_yaml` and `backend` | R |
| 4 | `execution/service.rs` | Both: ours' env list (with `HORCH_LEDGER_DB`) plus main's `HORCH_BACKEND=hc` on `hc` | R |
| 5 | `harness/claude.rs` | Combined: ours' reads, writes and sockets per access, written with `Vec`s, so that sockets are the state root plus `host_sockets` of main. The test env names `host_sockets` | R |
| 6 | `harness/codex.rs` | Combined: ours' `horch_platform::start::command`, plus main's `--no-daemon` first on `hc`. Both test sets stay | R |
| 7 | `harness/launch.rs` | Combined: `herdr_socket` goes, main's `backend` and `host_sockets` come, ours' `ledger_db`, `fleet_root`, `task_dir`, `worktree` stay. Both test sets stay | R |
| 8 | `messaging/brief.rs` | Both: `task_yaml` and `backend`, both tests | R |
| 9 | `messaging/delivery.rs` | Combined: ours types only a `TypedLine`. Main keeps the prompt error when `pane_send_text` is `Unsupported`. Main's test `hcb_06_send_line_uses_only_the_prompt_on_hc` sends `TypedLine::Mail` and `TypedLine::Doorbell`, and its call log asserts their texts | R |
| 10 | `messaging/mailbox.rs` | Both fields | R |
| 11 | `taskledger/orchestrator.rs` | Ours: the run branch retired `claim` (FM-16). Decision (a): main's `claim_on` with a backend is not needed | R |
| 12 | `taskledger/schema.rs` | Both sides add version 9. Main's `V9` stays as `BACKEND_VERSION = 9`. `PHASE_1_VERSION = BACKEND_VERSION + 1`, so the fleetd steps move to 10 to 18. `VERSION = PHASE_7_VERSION` (18). Main's test `hcb_03_migrate_from_version_7_adds_backend_and_keeps_rows` calls `comment_sweep_step()` for main's `v8()`. Same reason as row 8 of T-312 | R |
| 13 | `workspace/herdr.rs` | Main's file (the bounded call code is in `exec.rs`), plus ours' `start::command` at 4 sites, `ProcessInfo` and `pane_shell_pid` with its test | R |
| 14 | `tests/baseline_oracles.rs` | Both env keys. Oracle count: `TEAMMATES_AT_A0 + TEAMMATES_AFTER_A0 + 2` (main's `orchestrator-hc.json`, `worker-hc.json`) | R |
| 15 | `tests/execution_plan.rs` | Both fields | R |
| 16 | `tests/taskledger_queue.rs` | Ours: the run branch builds old files with `migrate_to`, so main's "version 9" assert and its `DROP COLUMN backend` are not needed | R |
| 17 | `tests/trf_48_ledger_guard.rs` | Ours, same reason as row 16 | R |
| 18 | `horch-e2e/src/lib.rs` | Both modules: ours' 3, main's `hc_fake` | R |
| 19 | `horch-e2e/tests/fakes.rs` | Both: ours' `phase4` module, main's fake-hc tests | R |
| 20 | `cmd/compact.rs` | Both imports. `Herdr` became unused and went | R |
| 21 | `cmd/delivery_probe.rs` | Ours: the run branch deleted the fleet probe (no orchestrator pane). Main's `PaneHost` change to it is not needed | R |
| 22 | `cmd/doctor.rs` | Both imports. Main's version check `Command::new(&exe)` is `start::command(&exe)` (SR-7) | R |
| 23 | `cmd/mail.rs` | Ours' signature (the mail line is fixed, FR-7) with main's `&PaneHost` | R |
| 24 | `cmd/messaging.rs` | Ours: the run branch deleted `ack_role_mail` | R |
| 25 | `cmd/recipes.rs` | Ours in 8 hunks (the retired orchestrator start, respawn, claim). Theirs in 1: `pane_command_for` passes `HORCH_BACKEND` and `HORCH_PROJECT_DIR`. Git merged main's backend-scoped retire of gone orchestrators into ours' `retire_gone_orchestrators`: it stays, with `&PaneHost` | R |
| 26 | `cmd/spawn.rs` | Both: ours' retired-spawn guard first, then main's backend of the workspace | R |
| 27 | `cmd/taskledger.rs` | Ours: the run branch deleted `HerdrPanes` and `print_stop`, and `own_orchestrator_harness` | R |
| 28 | `cmd/taskledger_finish.rs` | Ours, as row 18 of T-312 | R |
| 29 | `cmd/taskledger_spawn.rs` | Ours: the run branch deleted `insert_row`. Main's `backend` on the row is not needed | R |
| 30 | `cmd/taskledger_wake.rs` | Ours: the run branch deleted the CLI wakes and the pre-claim sweep | R |
| 31 | `cmd/taskledger_watch.rs` | Ours in 6 hunks: fleetd runs the watch. Main's `hcb_10_watchdog_checks_a_worker_on_its_recorded_backend` was in the deleted `reconcile_tests`. Git also put main's `Live::Fixed` into ours' tests: the file was taken from ours, then its test helper builds `PaneHost::herdr_bin` (fix `05bf28b2`) | R |
| 32 | `cmd/main.rs` | Combined: main's `context_with_backend(cli.backend)` with ours' comment | R |
| 33 | `docs/fleet-workflow.md` | Ours: the table shows the fleetd flow. Main's `hc` paragraph describes the retired flow on `hc` | R |
| 34 | `teammates/_base/fleet-orchestrator.md` | Ours: the ideas briefing. Main's `herdr-app do pane.zoom` line is in the old text that ours removed | R |

Paths of rows 3 to 17 are under `crates/horch-core/src/` or `crates/horch-core/`, rows 20 to 32 under `crates/horch/src/`.

Files outside the conflicts that the trial changes in the merge commit:
- `crates/horch-core/src/workspace/exec.rs`: main's moved file, with ours' `horch-platform` process code of `herdr.rs` (`Contained`, `kill_child_group`, `exited_without_reap`, `name_herdr`) and ours' test changes. Decision (b) of PL-2.
- `crates/horch-core/src/workspace/hc.rs`: `start::command` in `hc_command` and `app_command`. Decision (b) of SR-7.
- `crates/horch/src/cmd/fleetd.rs`: `HC_REFUSED` and `refuse_hc`, called by `horch fleetd`.
- `crates/horch/src/cmd/recipes.rs`: `horch fleet` calls `refuse_hc`.
- `crates/horch-e2e/tests/messaging.rs`, `crates/horch-e2e/tests/pane_host_contract.rs`: compile fixes (`backend` field, and `TypedLine::DeliveryEchoPosix` with its output `HORCH_DELIVERY_73` in place of a free echo line).

## Changes of main that fleetd code must follow

| # | change of main | effect on the run branch | state |
|---|---|---|---|
| H1 | Every pane host call outside `workspace/` goes through `PaneHost` (fitness test `hcb_04_no_herdr_construction_outside_workspace`). | fleetd called `Herdr` in 7 places. Decision (b), done in `05bf28b2` and `2e5b90b7`: `HerdrObserver.herdr`, `notify`, `remind`, the `with_pass` builder of `cmd/fleetd.rs` (`PaneHost::for_backend(ctx, Backend::Herdr)`), the fleetd structs, the test helpers of `taskledger_watch.rs` and `tests/needs_you_folder.rs`. New `PaneHost::pane_shell_pid`: herdr only, `Unsupported` on `hc` | verified (the test passes) |
| H2 | Can fleetd run its workers on `hc`? | No. fleetd always uses the `herdr` backend. Missing for `hc`: (1) `pane_shell_pid` on `hc`, which the orphan scan of fleetd needs (`Live` in `cmd/fleetd.rs`); (2) `launch_pane` finds the workspace by its herdr label, and `hc` has spaces and tabs; (3) the launch step `horch spawn --from-pane` splits a herdr pane, and on `hc` it must use `workspace::place`; (4) the doorbell needs `agent_prompt` only, since `hc` has no `pane_send_text`; (5) `HerdrObserver` reads `pane_read`; (6) fleetd's service env has no `HORCH_BACKEND`. `horch fleet` and `horch fleetd` refuse `hc` with `fleetd::HC_REFUSED`, and touch no pane host | verified: `the_hc_backend_is_refused_with_its_fix_and_herdr_is_taken` (unit) and the 2 tests of `hc_fleet_start.rs` (e2e) |
| H3 | The old claim path of main (`orchestrator::claim_on`, `horch fleet` with an orchestrator pane on `hc`) | The run branch retired it before the merge (FM-16, ticket 48 of fleetd-p6), so it is not in the trial. The paths of main that stay (`horch doctor`, `horch mail courier`, `horch tell`, `PaneHost` reads) compile and their tests pass | verified (rows of `hc_doctor`, `hc_courier`, `pane_host_contract`, `fakes`) |
| H4 | Schema version 9 (`workers.backend`). | The installed horch 0.5.2 migrates a ledger to 9. Without the renumber, a fleetd build reads 9 as "Phase 1 done" and skips the fleetd tables. With it, a run-branch file at 9 to 17 is read 1 step too low | renumber verified (`trf_48_every_old_version_migrates_to_current` and `hcb_03_migrate...` pass). `horch --version` gives 0.5.1 on this machine. The live ledger was not read (rule 2) |
| H5 | `LaunchEnv.host_sockets` (row 5). A sandboxed Claude teammate on `hc` gets the `herdr-core` socket, which can run programs outside the sandbox (ADR 0003). | fleetd workers stay on herdr (H2), so the run branch's sandbox and RR-5 rules keep their effect | verified by reading; no `hc` launch of fleetd exists |
| H6 | `workspace/exec.rs` and `workspace/hc.rs` had OS `cfg` sites and `Command::new` in production code (PL-2, SR-7). | Ported, not skipped: `exec.rs` uses `horch_platform::group`, `hc.rs` and `doctor.rs` use `start::command` | verified: `grep` finds no `cfg(`, `libc`, `windows_sys` or `Command::new` above `#[cfg(test)]` in `exec.rs`; the PL-2 scan names only the 2 sites of "Red before the merge" |
| H7 | Session records of main's new tests are written without fleetd. | The run branch writes them through the fleetd channel (T-884). Main's 3 tests serve them: `hcb_03` (`test_serve`), `hcb_04_for_workspace_uses_the_recorded_backend` (`test_serve`), `hc_courier` (`serve_session_records`, and `HORCH_LEDGER_DB` of the harness) | verified (they pass) |
| H8 | Fake binaries `fake-hc` and `fake-herdr-app` (`horch-e2e/Cargo.toml`, `harness.rs` `FAKES`). | They merged without a conflict and build. The harness finds them by name | verified (`fakes` 61 / 0, `hc_doctor` 4 / 0) |
| H9 | `Cargo.toml` members, `.github/workflows/` | Main changes only the version. Main does not change `.github/` or the members | verified (`git diff --stat e5170752 origin/main`) |
| H10 | CHANGELOG | Main adds 17 lines, all under `## v0.5.2`: the Chrome fix of #364. #363 adds no line. `## Unreleased` stays empty. T-330 adds no line | verified |
| H11 | `justfile`: a comment names the Herdr app window | No effect | verified |
| H12 | Counts and oracles: `orchestrator-codex.json`, the new `orchestrator-hc.json` and `worker-hc.json`, `golden_prompts.rs` (+4 lines), `intake_roster.rs`, the 12 `BinOverrides` vars. | `baseline_oracles` counts 2 more files (row 14). `golden_prompts.rs` is the run branch's file (fix `29046595`): the trial keeps the fleetd briefing (row 34) | verified (horch-core suite) |

Searched in the trial for each name that the 3 commits touch: `Herdr::with_bin`,
`workspace::herdr::Herdr`, `herdr_socket`, `host_sockets`, `backend:`,
`claim_on`, `PaneHost`, `output_within`, `Command::new`, `cfg(`, `V9`,
`VERSION`, `migrate_to(` with a literal above 7, `fake-hc`, `fake-herdr-app`,
`CHANNEL_ENV_VARS`, `HC_STATUS_HARNESSES`, `CHANGELOG.md`, `.github/`.

## Result of each suite

Each command ran through `scripts/dev-sandbox.sh`. "first" is the run before the
fixes. "final" is the run on the final code (`2e5b90b7` for the crates,
`3fd534f7` for the e2e files that changed after it). Load is the 1-minute load
average at the start.

| command | first: passed / failed | final: passed / failed | load |
|---|---|---|---|
| `cargo check --workspace --all-targets --locked` | 4 compile errors in `recipes.rs`, `doctor.rs`, `taskledger_watch.rs`, 2 e2e files | exit 0 | 14.7 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 1 error (`disk.rs:190`) | 1 error, the same (red before the merge) | 13.7 / 38.0 |
| `cargo clippy --workspace --bins --release --locked -- -D warnings` (CI form) | exit 0 | exit 0 | 13.7 / 38.0 |
| `cargo fmt --all --check` | 3 files | exit 0 | - |
| `cargo test -p horch-core --locked --no-fail-fast` (223 binaries) | 3768 / 7 | 3773 / 2 (red before the merge) | 15.6 |
| `cargo test -p horch --locked --no-fail-fast` (49 binaries) | 617 / 1 | 617 / 1 (red before the merge) | about 15 |
| `cargo test -p ui-prop-tester --locked` (#364 changed `browser/process.rs`) | 241 / 0 | - | 38.0 |
| `python3 -m unittest tests.test_check_page` (#364) | 60 / 0 | - | - |
| e2e `fakes` (fake-hc, fake-herdr-app) | 61 / 0 | - | 64.6 |
| e2e `hc_courier` (new) | 0 / 1 | 1 / 0 | 64.6 / 38 |
| e2e `hc_doctor` (new) | 4 / 0 | - | 64.6 |
| e2e `hc_fleet_start` (new) | 0 / 6 | 2 / 0 (the file is replaced) | 64.6 / 38 |
| e2e `pane_host_contract` (new) | 2 / 0 | - | 64.6 |
| e2e `telemetry_start_handshake` (changed) | 3 / 0 | - | 64.6 |
| e2e `messaging` (compile fix) | 23 / 0 | - | 64.6 |
| e2e `p4_release` | 4 / 0 | - | 64.6 |
| e2e `p7_release` | 4 / 0 | - | 64.6 |
| e2e `taskledger` | 50 / 1 | - (red before the merge) | 64.6 |
| e2e `fleet_start` | 4 / 0 | - | 64.6 |

The e2e run used `--test-threads=4` (load above 32). Each row: verified (the
run logs of T-330).

Not run (rule 2: they start a real host, or a real Chrome):
- `pane_host_contract::hcb_14_contract_on_a_real_hc_core`: main marks it ignored, opt-in with `HORCH_LIVE_HC=1`. The 2 / 0 row is `hcb_14_contract_on_herdr` and `hcb_14_contract_on_hc` (fakes).
- `render_frames.mjs --self-test`, `text_audit.mjs`, `capture.mjs`, `materials.mjs` of #364: they start Chrome.
- `cargo test --workspace --locked` as 1 command, and the other e2e files.

## Fixes on the trial branch

| commit | file | why |
|---|---|---|
| `9944b2ac` | `horch-e2e/src/lib.rs`, `cmd/compact.rs`, `cmd/doctor.rs` | `cargo fmt` of the merge |
| `29046595` | `messaging/delivery.rs`, `tests/golden_prompts.rs` | `fr7_typed_lines` failed: each typing call must pass `line.text()`. The commit also holds the golden fix: `the_orchestrator_briefing_differs_only_where_sanctioned` failed on main's 4 zoom lines, and the file is now the run branch's file (row 34) |
| `05bf28b2` | `workspace/host.rs`, 3 fleetd files of horch-core, `cmd/fleetd.rs`, `cmd/taskledger_watch.rs` | H1, the unit test of H2, and `test_serve` in `hcb_04_for_workspace_uses_the_recorded_backend` (H7) |
| `a038bb83` | `execution/store.rs` | `hcb_03_backend_of_workspace_finds_the_owner` failed on the operator token (H7) |
| `2e5b90b7` | `tests/needs_you_folder.rs` | compile error after H1: the test built a `Herdr` |
| `fb85abaa` | `horch-e2e/tests/hc_courier.rs` | H7: the test served no session records, and its context named another ledger than the courier |
| `3fd534f7` | `horch-e2e/tests/hc_fleet_start.rs` | main's 6 tests start `horch fleet` with an orchestrator pane and `horch spawn` on `hc`. Both are retired on the run branch, and the trial refuses `hc` (H2). The file now has 2 tests: `horch --backend hc fleet` and `HORCH_BACKEND=hc horch fleetd check` fail with `HC_REFUSED`, make no `hc` space and call no `herdr` |

## Red before the merge

Each test below fails on files that the merge does not change. Verified with
`git diff --stat 4e806d0b trial-proof-2 -- <file>` (empty) unless the row says
otherwise.

| test | cause | evidence |
|---|---|---|
| `arch_scan` `fitness_no_context_touches_foreign_tables` | owner T-327 | known from the ticket |
| `arch_scan` `fitness_workspace_production_code_has_no_os_cfg_outside_horch_platform` | 2 OS `cfg` sites in `fleetd/worker_profiles/adapters/conformance_runner.rs:769` and `:776`, from commit `7f276a1a` (T-320) | `conformance_runner.rs` is not changed by the merge. `arch_scan.rs` differs from the run branch only by the `ui-prop-tester` skip of T-314. Not run on `fleet/fleetd-finish` |
| clippy `--all-targets`: `type_complexity` at `fleetd/worktree_custody/application/disk.rs:190` | test code of the run branch (commit `b7ccafca`, T-292) | `disk.rs` and the lint config are not changed by the merge. The CI form (`--bins --release`) does not build it. Not run on `fleet/fleetd-finish` |
| `horch` `cmd::taskledger_watch::tests::watch_once_runs_the_phase4_pass` | the task is `ready`, the test expects `leased` | verified by a run on `fleet/fleetd-finish` (`437190bc`) in a scratch worktree: the same failure |
| e2e `taskledger` `a_launch_of_an_unknown_teammate_leaves_no_live_row` | the end reason starts with `its spawn failed: ` (`handlers_worker.rs:192`), the test expects `unknown teammate` | `handlers_worker.rs` and `taskledger.rs` are not changed by the merge. Not run on `fleet/fleetd-finish` |

## For the operator

1. **fleetd and `hc`.** fleetd runs its workers only on `herdr` (H2). `horch fleet` and `horch fleetd` refuse `hc` with: "fleetd runs its workers only on herdr: the hc pane host (herdr-core and the Herdr desktop app) has no fleetd adapter. Run this command with --backend herdr, or unset HORCH_BACKEND". The 6 gaps of H2 are the work of an `hc` adapter for fleetd. That needs its own ticket.
2. **The 6 tests of main in `hc_fleet_start.rs` are gone** (fix `3fd534f7`): `hc_fleet_start_makes_a_space_and_records_the_backend`, `hc_spawn_splits_right_of_the_orchestrator`, `hc_spawn_fifth_worker_gets_a_new_tab`, `hc_spawn_with_the_window_closed_uses_a_labelled_run`, `hc_fleet_start_with_the_window_closed_fails_clearly`, `hcb_12_e2e_spawn_refuses_opencode_on_hc_before_any_pane_and_channel_refuses_without_record`. An `hc` adapter for fleetd brings its own tests.
3. **Docs that promise `hc` fleets.** `README.md` (lines 4, 7, 198, 264 to 308: `horch fleet --backend hc`), `docs/adr/0003-two-pane-hosts-behind-one-handle.md`, `docs/live-checks/hc-backend.md`, `docs/specs/pane-host.md`, `docs/testing-and-gates.md`, `docs/architecture.md`, `GLOSSARY.md` merged without a conflict. They describe `hc` fleets of the retired flow. T-330 did not edit them. Edit them after a decision on item 1.
4. **Schema numbers.** The code is now: `COMMENT_SWEEP_VERSION` 8, `BACKEND_VERSION` 9, `PHASE_1_VERSION` 10, `PHASE_2_VERSION` 11, `PHASE_3_VERSION` 12, `EVENT_KINDS_VERSION` 13, `WORKTREE_PATHS_VERSION` 14, `PHASE_4_VERSION` 15, `PHASE_5_VERSION` 16, `PHASE_6_VERSION` 17, `PHASE_7_VERSION` 18, `VERSION` 18. `docs/specs/fleetd/design.md` and `merge-main-proof.md` still name the numbers of T-314. A ledger file that a run-branch build or the trial of T-314 migrated past version 8 must be built again (Q2 of `merge-main-plan.md`).
5. **Watchdog of `hc` workers.** Main's watchdog checked a worker on its recorded backend (`hcb_10`). fleetd's watchdog uses `herdr` for every worker row. A row with `backend = 'hc'` can come only from a horch 0.5.2 of main on the same ledger. Not verified as a behavior.
6. **Red before the merge.** 5 tests and 1 clippy error (section above). 4 of them are not run on `fleet/fleetd-finish`; their files are the same.
7. **Not verified.**
   - `cargo test --workspace --locked` as 1 command, and the e2e files outside the table.
   - The CI result of the trial.
   - The run-branch commit `437190bc` (1 doc file) in the trial.
   - The tests of #364 that start a real Chrome, and the real `hc` contract test.
