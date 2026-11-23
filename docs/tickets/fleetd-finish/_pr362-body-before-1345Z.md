Run `fleetd-finish`: tasks land on `fleet/fleetd-finish`; this PR brings them to `fleet/fleetd-build`.

<!-- horch:task T-1 -->
### T-1 A1 isolation: dev-sandbox script, shared-ledger write guard, service label, e2e seal, and old-watchdog stop proof

- A1 1: scripts/dev-sandbox.sh runs a command with isolated horch dirs and removes them on exit and on signal (T-1) (`fb8b31c8`)
- A1 2: Store::open refuses each dev-build writable open of the shared ledger before it touches the disk, unless HORCH_ALLOW_LEDGER_MIGRATE=1 (T-1) (`c3a60353`)
- A1 3: a non-default ledger gets the service label horch-fleetd-<8 hex> of its canonical path; definitions validate the label; horch-fleetd stays for the default ledger (T-1) (`f262a3c4`)
- A1 4: the e2e harness seals HORCH_LEDGER_DB=<root>/ledger/ledger.sqlite3 by default, outside HORCH_STATE_DIR, with FLEET_ROOT unset (T-1) (`b3af3659`)
- A1 5: a test proves that the old-watchdog takeover of ledger A stops only the PID and start time that ledger A records, with ledger B in the same folder (T-1) (`4f217282`)
- A1 1: dev-sandbox.sh also removes .dev-sandbox when no other run uses it (T-1) (`a54c210c`)
- A1 1: comment sweep removes 29 comments that retell the code (T-1) (`085427fb`)

<details><summary>8 files changed</summary>

- `.gitignore`
- `crates/horch-core/src/taskledger/store.rs`
- `crates/horch-core/tests/old_watchdog.rs`
- `crates/horch-e2e/src/harness.rs`
- `crates/horch-platform/src/service.rs`
- `crates/horch-platform/tests/service.rs`
- `crates/horch/src/cmd/fleetd.rs`
- `scripts/dev-sandbox.sh`

</details>

Landed by opus-hardening-1 at `085427fba539`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-1 -->

<!-- horch:task T-2 -->
### T-2 A2 triage bug: fix detached-HEAD worktree handling in horch worktree triage apply

- fleetd-finish A2: triage apply treats a (detached) branch as no branch and uses the commit id for ARCHIVE and DELETE (L14, T-2) (`41b0f2e2`)
- A2 comment sweep: remove 7 comments that retell the code (T-2) (`68503d2f`)

<details><summary>3 files changed</summary>

- `crates/horch-core/src/taskledger/triage/apply.rs`
- `crates/horch-core/src/taskledger/triage/mod.rs`
- `crates/horch-core/tests/worktree_triage.rs`

</details>

Landed by opus-1 at `68503d2f7d80`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-2 -->

<!-- horch:task T-25 -->
### T-25 R1: end review of phases 0, P and 1

- R1: the end review of phases 0, P and 1 (the platform layer, the control loop, the jobs, the fleet clock, the launcher, the health guards) (`6d90f625`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/reviews/end-review-p0-pp-p1.md`

</details>

Landed by codex-sol-9 at `6d90f625880c`.
<!-- /horch:task T-25 -->

<!-- horch:task T-27 -->
### T-27 R3: end review of phase 4

- R3: the end review of phase 4 (the verifier, result.json, the Stop hooks, the landing state machine, bundles, gates, git push and fetch jobs, the git sandbox) (`5cc44579`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/reviews/end-review-p4.md`

</details>

Landed by codex-sol-11 at `5cc44579b2ae`.
<!-- /horch:task T-27 -->

<!-- horch:task T-28 -->
### T-28 R4: end review of phase 5

- R4: the end review of phase 5 (the command channel, request files, the requests table, the operator token, the peer check, the worker sandbox deny rules, the single writer) (`b4bdadac`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/reviews/end-review-p5.md`

</details>

Landed by codex-sol-12 at `b4bdadac2ea8`.
<!-- /horch:task T-28 -->

<!-- horch:task T-26 -->
### T-26 R2: end review of phases 2 and 3

- R2: the end review of phases 2 and 3 (leases, the stall ladder, waiting_quota, questions, worktrees, rescue, triage) (`6fd4f167`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/reviews/end-review-p2-p3.md`

</details>

Landed by codex-sol-10 at `6fd4f16717bc`.
<!-- /horch:task T-26 -->

<!-- horch:task T-30 -->
### T-30 R6: end review of phase 7

- R6: the end review of phase 7 (docs sync, the conformance suite, profile health, exec mode, the scrubbed launcher, the check Operator boundary) (`6f73f2ad`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/reviews/end-review-p7.md`

</details>

Landed by codex-sol-14 at `6f73f2adba8b`.
<!-- /horch:task T-30 -->

<!-- horch:task T-29 -->
### T-29 R5: end review of phase 6

- R5: the end review of phase 6 (ideas, the dispatcher, the sub-orchestrator, the plan validator, the judge pair, the question flow, trial merges, stacks) (`127bc22d`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/reviews/end-review-p6.md`

</details>

Landed by codex-sol-13 at `127bc22d8055`.
<!-- /horch:task T-29 -->

<!-- horch:task T-16 -->
### T-16 D1 Antigravity Stop hook: add the Stop hook in antigravity.rs and its test in harness_defaults.rs

- fleetd-finish 16: a push attempt of agy gets a Stop hook in <worktree>/.agents/hooks.json that maps exit 2 of horch result --check to {"decision":"continue"}; the git exclude file keeps it out of commits, a tracked project file stays unchanged, and a claim attempt gets no hook (T-16) (`b1382d1a`)
- fleetd-finish 16: comment sweep removes 10 narrating comments of the agy Stop hook and keeps the vendor facts (T-16) (`2f21c38b`)
- fleetd-finish 16: a symlink at .agents or .agents/hooks.json gets no agy Stop hook, so horch writes no file outside the worktree (T-16) (`df3f9299`)

<details><summary>2 files changed</summary>

- `crates/horch-core/src/harness/antigravity.rs`
- `crates/horch-core/tests/harness_defaults.rs`

</details>

Landed by opus-3 at `df3f92996b9b`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-16 -->

<!-- horch:task T-24 -->
### T-24 CI0: run the CI of the run branch on macOS, Linux and Windows now, and record each failure

- CI0: the first CI result of fleetd on 3 OSes (`6608ac79`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`

</details>

Landed by qa-engineer-2 at `6608ac79e5e6`.
<!-- /horch:task T-24 -->

<!-- horch:task T-42 -->
### T-42 R7: watchlist coverage check and review of each uncovered point

- R7: audit watchlist coverage and review uncovered points (`8d4df21b`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/reviews/watchlist-coverage.md`

</details>

Landed by codex-sol-15 at `8d4df21bb434`.
<!-- /horch:task T-42 -->

<!-- horch:task T-41 -->
### T-41 Fix R2-F1: settle each triage_apply decision under its own savepoint (FM-11, RR-7, RR-16, RR-17)

- fleetd-finish 39: triage_apply ingest settles each decision under its own savepoint, so a refused fact fails only its own decision (R2-F1, T-41) (`e62000c4`)
- fleetd-finish 39: comment sweep removes 5 narrating comments of the triage_apply savepoint change and keeps the ingest contract (T-41) (`425ddab1`)

<details><summary>2 files changed</summary>

- `crates/horch-core/src/taskledger/triage/apply.rs`
- `crates/horch-core/tests/triage_apply_job.rs`

</details>

Landed by opus-5 at `425ddab11ea4`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-41 -->

<!-- horch:task T-51 -->
### T-51 Fix R3-F2: seed the Antigravity Stop hook on exec-mode push launches (FM-6)

- fleetd-finish 60: an Antigravity exec attempt seeds the horch-result-gate Stop hook in .agents/hooks.json, as a push launch does (R3-F2, FM-6) (T-51) (`726f56de`)
- Comment sweep T-51: drop the caller list on seed_stop_hook and the test doc comment (T-58) (`9e9d9346`)

<details><summary>3 files changed</summary>

- `crates/horch-core/src/harness/antigravity.rs`
- `crates/horch-core/src/harness/launch.rs`
- `crates/horch-core/tests/p7_antigravity_exec.rs`

</details>

Landed by opus-6 at `9e9d93461a9b`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-51 -->

<!-- horch:task T-53 -->
### T-53 Strengthen R1 SR-6 job result file tests for process identity and non-running job rows

- Strengthen the SR-6 job result file tests: the pid guard of run_job and the state guard of ingest_results (T-53) (`fe9495d1`)
- Comment sweep of T-53: remove 11 narrating comments from the SR-6 job result file tests (`57f26733`)
- Move the note check of the SR-6 ignored-file test to a helper (T-53) (`dc6ea2be`)
- Remove the narration doc comment of assert_one_ignored_note (T-53 comment sweep) (`fdf2e33b`)

<details><summary>1 file changed</summary>

- `crates/horch-core/tests/fleetd_job_result_file.rs`

</details>

Landed by qa-engineer-3 at `fdf2e33b124a`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-53 -->

<!-- horch:task T-59 -->
### T-59 CI FIX-1: the e2e harness must not take the cargo profile from a folder name

- CI FIX-1: the e2e harness takes the profile folder from the target layout, not from the last folder name (T-59) (`57a2e031`)
- Comment sweep T-59: remove 3 comments that retell code in crates/horch-e2e/src/harness.rs (`83b085ab`)

<details><summary>2 files changed</summary>

- `crates/horch-e2e/src/harness.rs`
- `crates/horch-e2e/src/lib.rs`

</details>

Landed by opus-7 at `83b085ab0232`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-59 -->

<!-- horch:task T-57 -->
### T-57 Strengthen R2 FM-12 and FM-25 tests for fetch timeout, missing origin, and git-writer release for rescue jobs

- fleetd-finish 94: FM-12 and FM-25 tests prove a failed or hung fetch releases the git writer for a queued rescue (T-57) (`d1e93883`)
- fleetd-finish 94: comment sweep removes 7 narrating comments of the FM-12 and FM-25 tests and keeps the FM doc comments and the zombie note (T-57) (`be3a8818`)
- fleetd-finish 94: split the hung fetch test into helpers to cut its cognitive complexity (T-57) (`2de858d0`)
- fleetd-finish 94: comment sweep 2 removes 4 narrating doc comments of the hung fetch test helpers and keeps the FM doc comment and the zombie note (T-57) (`c5838258`)

<details><summary>2 files changed</summary>

- `crates/horch-core/tests/fetch_job.rs`
- `crates/horch-core/tests/worktree_jobs.rs`

</details>

Landed by qa-engineer-6 at `c583825856cf`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-57 -->

<!-- horch:task T-56 -->
### T-56 Strengthen R2 FM-38 repair worktree tests and prevent triage from parking unsettled task branches

- fleetd-finish 93: triage never lists the worktree of an unsettled task branch, and the FM-38 tests prove triage and import leave it alone (T-56) (`3d053335`)
- fleetd-finish 93: comment sweep removes 5 comments of the unsettled-branch triage change that retell the code and tests (T-56) (`e231bc8d`)

<details><summary>2 files changed</summary>

- `crates/horch-core/src/taskledger/triage/mod.rs`
- `crates/horch-core/tests/repair_attempts.rs`

</details>

Landed by qa-engineer-5 at `e231bc8d6923`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-56 -->

<!-- horch:task T-3 -->
### T-3 B1 baseline: execute fmt, clippy, and full workspace tests in the sandbox and record baseline-2026-11.md

- B1 3: baseline-2026-11.md records the first executed fmt, clippy and test run of fleetd-build: 361 failing tests, 69 min, target 9.0G (T-3) (`f81544e8`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/baseline-2026-11.md`

</details>

Landed by qa-engineer-1 at `f81544e8f851`.
<!-- /horch:task T-3 -->

<!-- horch:task T-50 -->
### T-50 Fix R6-F4: prepare the isolated real-CLI conformance release gate and operator step (FR-6, FR-8, SR-1, SR-8, FM-18, FM-19)

- fleetd-finish 82: scripts/live/p7-real-conformance.sh runs the production conformance suite of each real CLI in a temp fleet, keeps <out>/<cli>/<version>/conformance.json and fails on each check that does not pass; the gate test runs it with the fakes (R6-F4, T-50) (`38841a03`)
- comment sweep T-50: remove 17 comments of the p7 gate test and the gate script; rename SOURCES to DOCS_SOURCE_BINS (`fb33bd1b`)
- fleetd-finish 82: restore the Operator release step in the header of scripts/live/p7-real-conformance.sh; ticket 82 requires it in the header and in --help (T-50) (`f3991213`)

<details><summary>2 files changed</summary>

- `crates/horch-e2e/tests/p7_conformance_binary.rs`
- `scripts/live/p7-real-conformance.sh`

</details>

Landed by opus-hardening-2 at `f3991213272c`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-50 -->

<!-- horch:task T-75 -->
### T-75 CI FIX-2: install socat with bubblewrap on Linux CI

- CI FIX-2: install socat with bubblewrap on Linux CI (T-75) (`b001e2df`)
- CI FIX-2: comment sweep removes the sandbox.rs comment, since socat is checked in harness/claude.rs (T-75) (`0f98cf56`)

<details><summary>1 file changed</summary>

- `.github/workflows/ci.yml`

</details>

Landed by opus-8 at `0f98cf565eec`. comment-sweep: swept.
<!-- /horch:task T-75 -->

<!-- horch:task T-79 -->
### T-79 CI FIX-6: gate the unix symlink test in horch-marketplace for Windows test build

- ci-fix-6: gate mkt_06_rejects_symlink with cfg(unix) so the marketplace test target builds on Windows (T-79) (`d729a51f`)

<details><summary>1 file changed</summary>

- `crates/horch-marketplace/tests/marketplace.rs`

</details>

Landed by qa-engineer-7 at `d729a51f2422`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-79 -->

<!-- horch:task T-84 -->
### T-84 CI FIX-8: migrate stale CLI routing tests in p5_route_ledger_run.rs, p5_route_ledger_task.rs and p5_route_operator_commands.rs

- fleetd-finish 110: CI FIX-8 migrates the p5 CLI routing tests to the Phase 6 commands (T-84) (`8982b825`)
- T-84 comment sweep: remove 3 history and narration comments from the p5 routing tests (`7994bbfd`)

<details><summary>3 files changed</summary>

- `crates/horch/tests/p5_route_ledger_run.rs`
- `crates/horch/tests/p5_route_ledger_task.rs`
- `crates/horch/tests/p5_route_operator_commands.rs`

</details>

Landed by qa-engineer-10 at `7994bbfdd6e6`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-84 -->

<!-- horch:task T-95 -->
### T-95 CI FIX-10: fix the timing/process failure in horch-core tests/gate_jobs.rs

- ci-fix-10: gate_jobs alive() treats a zombie as dead, so the killed gate group is seen as gone on Linux (T-95) (`1950f464`)

<details><summary>1 file changed</summary>

- `crates/horch-core/tests/gate_jobs.rs`

</details>

Landed by qa-engineer-14 at `1950f464dee8`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-95 -->

<!-- horch:task T-85 -->
### T-85 CI FIX-8: fix stale tests in p6_retired_run_commands.rs, p6_retired_worker_commands.rs and w3_ledger_cli_pins.rs

- fleetd-finish CI FIX-8: the p6 retired-command pins and w3_ledger_cli_pins match Phase 6 (T-85) (`2cc19d0d`)
- T-85 comment sweep: delete 8 narration comments in the p6 and w3 test pins, name the e2e seed fixture path (`4fc28694`)

<details><summary>3 files changed</summary>

- `crates/horch/tests/p6_retired_run_commands.rs`
- `crates/horch/tests/p6_retired_worker_commands.rs`
- `crates/horch/tests/w3_ledger_cli_pins.rs`

</details>

Landed by qa-engineer-11 at `4fc286943d18`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-85 -->

<!-- horch:task T-82 -->
### T-82 CI FIX-8: migrate stale CLI tests in context_cli.rs, ledger_cli.rs and trf_11_add_map_cli.rs

- fleetd-finish 108: migrate the stale CLI tests of context_cli, ledger_cli and trf_11_add_map_cli to the Phase 6 commands (T-82) (`04a07d19`)
- comment sweep T-82: remove 8 narration and history comments from the migrated CLI tests (T-113) (`61e7e1d2`)

<details><summary>3 files changed</summary>

- `crates/horch/tests/context_cli.rs`
- `crates/horch/tests/ledger_cli.rs`
- `crates/horch/tests/trf_11_add_map_cli.rs`

</details>

Landed by qa-engineer-8 at `61e7e1d26c9e`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-82 -->

<!-- horch:task T-55 -->
### T-55 Strengthen R2 e2e triage tests in p3_triage.rs with a multi-item triage_apply decision boundary

- fleetd-finish 92: a multi-item triage_apply e2e test keeps the completed first item when later items are refused (R2, FM-11, RR-16, RR-17, T-55) (`f1c9230d`)
- fleetd-finish 92: an e2e test edits the result file of a held triage_apply job, and the ingest keeps item 1 when the fact of item 2 is refused (R2-F1, T-55) (`7a7a2e6a`)
- fleetd-finish 92: comment sweep removes 8 narrating comments of the multi-item triage_apply e2e tests (T-55) (`a0a9630d`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/tests/p3_triage.rs`

</details>

Landed by qa-engineer-4 at `a0a9630d8b5d`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-55 -->

<!-- horch:task T-98 -->
### T-98 CI FIX-10: fix the launching start-timeout failure in horch-core tests/fleetd_pass.rs

- CI FIX-10: the launching start-timeout test waits for the launched process to exit before any pass scans it (T-98) (`67864a99`)
- CI FIX-10: comment sweep removes 2 comments of the start-timeout test that retell the test and its helper (T-116) (`afc8313d`)

<details><summary>1 file changed</summary>

- `crates/horch-core/tests/fleetd_pass.rs`

</details>

Landed by qa-engineer-15 at `afc8313df506`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-98 -->

<!-- horch:task T-101 -->
### T-101 CI FIX-10: fix unexplained failures in horch-core fleetd_handlers_custody.rs, fleetd_pass_p3.rs, trial_merges.rs, w3_finish_steps.rs and w3_taskledger_core_status.rs

- CI FIX-10: the 5 horch-core test files follow the queued triage_apply job, the job result file, the lease line of status and the ended attempt of a replaced worker (T-101) (`6bbc5224`)
- Comment sweep T-101: remove 6 comments that retell the test code (`5632bc4a`)

<details><summary>5 files changed</summary>

- `crates/horch-core/tests/fleetd_handlers_custody.rs`
- `crates/horch-core/tests/fleetd_pass_p3.rs`
- `crates/horch-core/tests/trial_merges.rs`
- `crates/horch-core/tests/w3_finish_steps.rs`
- `crates/horch-core/tests/w3_taskledger_core_status.rs`

</details>

Landed by qa-engineer-16 at `5632bc4a671f`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-101 -->

<!-- horch:task T-122 -->
### T-122 Fix the red tests of `channel.rs`, `fleet18_delivery_probe.rs`, and `relay_first_delivery.rs`

- fleetd-finish 143: the 4 red channel.rs tests serve a test fleetd on the short state ledger before they insert session records (T-122) (`d78bc545`)
- fleetd-finish 143: delete the 4 tests of the removed horch fleet startup probe (T-122) (`0972d42e`)
- fleetd-finish 143: comment sweep removes 1 comment of channel.rs world() that retells the ledger path and the fleetd coupling (T-122) (`66f774b7`)

<details><summary>3 files changed</summary>

- `crates/horch-e2e/tests/channel.rs`
- `crates/horch-e2e/tests/fleet18_delivery_probe.rs`
- `crates/horch-e2e/tests/relay_first_delivery.rs`

</details>

Landed by qa-engineer-22 at `66f774b7865f`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-122 -->

<!-- horch:task T-120 -->
### T-120 Fix the red tests of `crates/horch-e2e/tests/e2e.rs`

- T-120: move or delete the 12 red e2e tests of removed Phase 6 behavior (`6001e6be`)
- T-120 comment sweep: remove 2 comments that narrate the retired horch spawn and retell the route assertions (`2a04583d`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/tests/e2e.rs`

</details>

Landed by qa-engineer-20 at `2a04583daa6d`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-120 -->

<!-- horch:task T-102 -->
### T-102 CI FIX-10: fix unexplained failures in horch-core baseline_oracles.rs, fr5_push_only.rs, harness.rs, intake_roster.rs, p5_profile_validation.rs, p7_wrapper_exec.rs and trf_02_ticket_template.rs

- CI FIX-10: the 8 stale horch-core tests match Phase 6 and Phase 7 (T-102) (`ce94c040`)
- Comment sweep T-102: remove 6 comments that retell the tests, and name the pinned Codex facts (`ca20fa5f`)

<details><summary>7 files changed</summary>

- `crates/horch-core/tests/baseline_oracles.rs`
- `crates/horch-core/tests/fixtures/harness/codex_rules_orchestrator.rules`
- `crates/horch-core/tests/fr5_push_only.rs`
- `crates/horch-core/tests/intake_roster.rs`
- `crates/horch-core/tests/p5_profile_validation.rs`
- `crates/horch-core/tests/p7_wrapper_exec.rs`
- `crates/horch-core/tests/trf_02_ticket_template.rs`

</details>

Landed by qa-engineer-17 at `ca20fa5f3eb2`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-102 -->

<!-- horch:task T-114 -->
### T-114 e2e harness teardown: processes of fleetd are alive 5 s after SIGTERM under load

- fleetd-finish 43: e2e teardown waits the design stop grace (stop_grace_s 20 s + 5 s) for processes to end after SIGTERM, not a fixed 5 s (T-114) (`94d0e10e`)
- T-114 comment sweep: remove 6 narration comments from the teardown grace and its tests, rename REAP_MARGIN to PS_SCAN_MARGIN (`4e268040`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/src/harness.rs`

</details>

Landed by opus-9 at `4e268040e3eb`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-114 -->

<!-- horch:task T-125 -->
### T-125 Fix the red tests of `crates/horch-core/tests/p7_conformance.rs`

- fleetd-finish 146: the fake ends a conformance check with no result when the scenario has conformance and exit_without_result; p0_dispatch fm6 keeps its passing background check (T-125) (`b52edb00`)
- comment sweep removes 1 comment sentence of the fake that retells ends_with_no_result (T-125) (`405ddb05`)

<details><summary>2 files changed</summary>

- `crates/horch-core/tests/p7_conformance.rs`
- `crates/horch-e2e/src/exec.rs`

</details>

Landed by qa-engineer-24 at `405ddb057ad5`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-125 -->

<!-- horch:task T-87 -->
### T-87 CI FIX-8: migrate stale tests in horch-e2e i308_fleet_steward.rs, i308_triage.rs, skill_intake_spawn.rs, watchdog_liveness.rs and worktree.rs

- CI FIX-8: migrate the stale e2e tests of i308_fleet_steward, i308_triage, watchdog_liveness and worktree to ledger_seed (T-87) (`8060ebda`)
- CI FIX-8: skill_intake_spawn.rs again, with the 3 security cases of TRF-43 in their new form (T-87) (`1f9560cf`)
- comment sweep T-87: remove 18 comments that retell the test code or cite tickets (T-132) (`1c75d9e8`)

<details><summary>6 files changed</summary>

- `crates/horch-e2e/expected-failures/i308_fleet_steward.txt`
- `crates/horch-e2e/tests/i308_fleet_steward.rs`
- `crates/horch-e2e/tests/i308_triage.rs`
- `crates/horch-e2e/tests/skill_intake_spawn.rs`
- `crates/horch-e2e/tests/watchdog_liveness.rs`
- `crates/horch-e2e/tests/worktree.rs`

</details>

Landed by qa-engineer-12 at `1c75d9e8c6c2`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-87 -->

<!-- horch:task T-121 -->
### T-121 Fix the red tests of `crates/horch-e2e/tests/messaging.rs`

- fleetd-finish 142: the e2e messaging tests start their worker pane without the retired horch spawn (T-121) (`708df4c3`)
- comment sweep T-121: remove 7 comments from crates/horch-e2e/tests/messaging.rs (`825f9fc8`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/tests/messaging.rs`

</details>

Landed by qa-engineer-21 at `825f9fc80d27`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-121 -->

<!-- horch:task T-83 -->
### T-83 CI FIX-8: migrate stale CLI tests in p3_worktrees_cli.rs, p4_question_file.rs and p4_result_cli.rs

- ci-fix-8: p3_worktrees_cli, p4_question_file and p4_result_cli seed the ledger directly and bind each attempt in place of the retired horch ledger run new, ledger add and worktree new (T-83) (`602fa23b`)
- comment sweep T-83: delete 15 comments in the 3 migrated test files; rename PATH to PATH_WITHOUT_AGENT_CLIS in p3_worktrees_cli (T-134) (`a300bd8b`)

<details><summary>3 files changed</summary>

- `crates/horch/tests/p3_worktrees_cli.rs`
- `crates/horch/tests/p4_question_file.rs`
- `crates/horch/tests/p4_result_cli.rs`

</details>

Landed by qa-engineer-9 at `a300bd8bf53f`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-83 -->

<!-- horch:task T-123 -->
### T-123 Fix the red tests of `p5_read_only_worker.rs` and `p5_release.rs`

- fleetd-finish 144: p5_release and p5_read_only_worker drop their fleetds before the harness, and p5_read_only_worker waits for the fleetd channel before the sandboxed report (T-123) (`ae950bbd`)
- comment sweep removes 3 comments of p5_read_only_worker and p5_release that state field drop order and the fleetd channel wait (T-123) (`7fbdc0d3`)

<details><summary>2 files changed</summary>

- `crates/horch-e2e/tests/p5_read_only_worker.rs`
- `crates/horch-e2e/tests/p5_release.rs`

</details>

Landed by qa-engineer-26 at `7fbdc0d3b049`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-123 -->

<!-- horch:task T-135 -->
### T-135 Land the fix of task T-39 (R1-F3, the pass loop wakes on events) with a longer check time

- T-39: fleetd wakes its pass loop on a request file, an attempt outbox event, a job result file or a ledger commit (RR-4, R1-F3); p1_pass seeds its run with ledger_seed (`b4586096`)
- T-39: p1_pass reads the ledger read-only, so a poll never waits on the migration lock during a long pass (`8a91e941`)
- T-39: list 4 stale p1_pass tests as expected failures until ticket 41 or 42 of fleetd-finish repairs them (`9d965224`)
- T-39: p1_pass wake test and scan test stay stable on a loaded host (`bc7910ad`)
- T-135 comment sweep: remove narrating comments in p1_pass.rs and fleetd.rs; name the ledger commit helper, the armed data_version and the drained watch events (`17a414c2`)

<details><summary>3 files changed</summary>

- `crates/horch-e2e/expected-failures/p1_pass.txt`
- `crates/horch-e2e/tests/p1_pass.rs`
- `crates/horch/src/cmd/fleetd.rs`

</details>

Landed by opus-10 at `17a414c26182`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-135 -->

<!-- horch:task T-130 -->
### T-130 Land the fix of task T-106 (p7_docs_sync and p7_release) with a longer check time

- CI FIX-10: the drift tests of p7_docs_sync fail the StopGate check with ignore_stop_gate (T-106) (`bbe501bc`)
- comment sweep T-130: remove 1 comment from crates/horch-e2e/tests/p7_docs_sync.rs (`3c358000`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/tests/p7_docs_sync.rs`

</details>

Landed by qa-engineer-25 at `3c3580008b73`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-130 -->

<!-- horch:task T-4 -->
### T-4 C1 land T-898: FR-10 fleet-process security boundary

- T-898 WIP: fleet_process rule (FR-10), idea-lead attempt goes live on its pane wrapper, FR-10 e2e tests and fake (`894c38d5`)
- T-898 WIP: FR-10 tests d and b fixed (events.cause, SIGTERM is the polite stop); outlive fake waits 600 s (`5c635262`)
- T-898: I5 goes on with the stop ladder for the group of a gone leader (OrphanGroup), keeps a group with a new leader (needs-you group_kept); test b green (`ace84073`)
- T-898: Operator boundary probes a double-fork orphan of the agent; e2e orphan case moved there; fix A tests (attempt stays live, idea note accepted) (`5bac9bfb`)
- C1 T-898: the SR-8 gate test waits for fleetd to move the done task branch to ready and to drop its bundle before it reads the state that the gate must not change (T-4) (`e7434164`)
- Comment sweep T-4: delete 65 comments that restate the code of the T-898 diff (`2ca5899e`)
- C1 T-898: fleet_process goes on from the new parent when a process of the chain ends during the walk (T-4) (`5830a07c`)
- C1 T-898: the evidence of the teardown leak of p6_fr10_approval under load, for ticket 43 (T-4) (`e4a442e7`)
- C1 T-898: a read that fails after a match keeps the answer Yes in fleet_process (T-4) (`e4c349c5`)
- C1 T-898: horch idea note waits while its sub-orchestrator attempt has no recorded wrapper, as a task command does; starting() counts each attempt, also one with no task (T-4) (`c5e56798`)
- comment sweep T-4: remove 13 comments that retell the code or the test name (T-141) (`2cf1c153`)

<details><summary>39 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/adapters/effects.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/adapters/peer_process.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/adapters/polite_stops_store.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/domain/authorize.rs`
- `crates/horch-core/src/fleetd/ideas/adapters/idea_notes_reader.rs`
- `crates/horch-core/src/fleetd/ideas/application/dispatcher.rs`
- `crates/horch-core/src/fleetd/ideas/application/sub_orchestrators.rs`
- `crates/horch-core/src/fleetd/task_execution/adapters/outbox_writer.rs`
- `crates/horch-core/src/fleetd/task_execution/adapters/sqlite.rs`
- `crates/horch-core/src/fleetd/task_execution/application/commands.rs`
- `crates/horch-core/src/fleetd/task_execution/domain/task.rs`
- `crates/horch-core/src/fleetd/worker_profiles/application/conformance.rs`
- `crates/horch-core/src/fleetd/worker_profiles/domain/conformance.rs`
- `crates/horch-core/src/harness/antigravity.rs`
- `crates/horch-core/tests/fetch_job.rs`
- `crates/horch-core/tests/fleetd_channel.rs`
- `crates/horch-core/tests/fleetd_handlers_custody.rs`
- `crates/horch-core/tests/p6_authorize.rs`
- and 19 more files

</details>

Landed by opus-2 at `2cf1c153b342`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-4 -->

<!-- horch:task T-40 -->
### T-40 Fix R1-F4: isolate each feature and task branch in its own savepoint (OR-2)

- fleetd-finish 38: features::follow_in and landing::follow isolate each feature and task branch row in its own savepoint; a failed row rolls back alone, opens 1 stable needs-you item, and the pass goes on (R1-F4, OR-2) (T-40) (`97770740`)
- fleetd-finish 38: comment sweep removes 15 narration comments from the follow-failed savepoint code and its 2 tests (T-40) (`f7b6aba4`)

<details><summary>4 files changed</summary>

- `crates/horch-core/src/fleetd/integration/application/features.rs`
- `crates/horch-core/src/fleetd/integration/application/landing.rs`
- `crates/horch-core/tests/feature_states.rs`
- `crates/horch-core/tests/landing_states.rs`

</details>

Landed by opus-13 at `f7b6aba49022`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-40 -->

<!-- horch:task T-44 -->
### T-44 Fix R5-F4: write own_decision assumptions.md as a post-commit effect and recover unwritten answer files from the ledger (RR-8, FM-11)

- fleetd-finish 52: answer_question writes no file in the transaction; own_decision assumptions.md is an intended effect in the question event, written by Recorded::write, and pending_answer_files recovers unwritten answer files from the ledger (R5-F4, RR-8, FM-11) (T-44) (`93bbb0d6`)
- comment sweep T-44: delete 15 comments in answers.rs and question_answers.rs; rename Own to OwnDecisionFiles and assumed to sha_after_pending (T-148) (`dc887056`)

<details><summary>2 files changed</summary>

- `crates/horch-core/src/fleetd/ideas/application/answers.rs`
- `crates/horch-core/tests/question_answers.rs`

</details>

Landed by opus-15 at `dc887056c983`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-44 -->

<!-- horch:task T-38 -->
### T-38 Fix R1-F2: make Windows channel reply writes bounded and cancellable on shutdown (PL-3, RR-7)

- fleetd-finish 36: Windows channel writes have a timeout and the channel stop is bounded (R1-F2, PL-3, RR-7) (T-38) (`55191224`)
- T-38 comment sweep: delete 19 comments that retell the code (`e2b2d89e`)

<details><summary>5 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`
- `crates/horch-core/tests/fleetd_channel.rs`
- `crates/horch-platform/src/channel.rs`
- `crates/horch-platform/src/windows/channel.rs`
- `crates/horch-platform/tests/channel.rs`

</details>

Landed by opus-12 at `e2b2d89ee6a8`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-38 -->

<!-- horch:task T-49 -->
### T-49 Fix R6-F2: probe outside-sandbox write access to the request folder in operator_reach (SR-8)

- fleetd-finish 81: operator_reach probes an outside-sandbox write into the request folder; an unwritable folder fails with 'proves nothing' (R6-F2, SR-8) (T-49) (`93180211`)
- comment sweep: remove 3 narration comments from the T-49 operator_reach write probe and its test (T-146) (`34df7486`)

<details><summary>2 files changed</summary>

- `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs`
- `crates/horch-core/tests/p7_operator_boundary.rs`

</details>

Landed by opus-hardening-5 at `34df74863712`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-49 -->

<!-- horch:task T-43 -->
### T-43 Fix R5-F2: require a durable open question or wait row before a clarifying sub-orchestrator enters waiting_on_you (FR-2, FM-29, RR-7)

- fleetd-finish 50: a clarifying idea waits on you only with a durable open interview question; horch idea note --ask records it, an interview note or an amendment closes it (R5-F2, T-43) (`56439500`)
- fleetd-finish 50: comment sweep of T-43 removes 18 comments that retell the code (T-151) (`d883d4a3`)
- fleetd-finish 50: an ask note with unreadable amendments is rejected alone and the ingest goes on; an ask question is 1 line of at most 1000 characters, quoted in the waiting item (T-43) (`0d7f5faf`)

<details><summary>7 files changed</summary>

- `crates/horch-core/src/fleetd/ideas/adapters/idea_notes_reader.rs`
- `crates/horch-core/src/fleetd/ideas/application/notes_ingest.rs`
- `crates/horch-core/src/fleetd/ideas/application/waiting.rs`
- `crates/horch-core/tests/fleetd_pass_p6a.rs`
- `crates/horch-core/tests/idea_notes.rs`
- `crates/horch-core/tests/idea_waiting.rs`
- `crates/horch/src/cmd/idea/note.rs`

</details>

Landed by opus-14 at `0d7f5faf8ef9`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-43 -->

<!-- horch:task T-124 -->
### T-124 Fix the red tests of `brief.rs`, `restored_pane.rs`, `routing_provenance.rs`, `scenario.rs`, `startup_error.rs`, and `tell_grace.rs`

- fleetd-finish 145: brief.rs launches its worker through fleetd, restored_pane.rs and tell_grace.rs write the registry, brief and session record that horch spawn left (T-124) (`fe4c4c98`)
- fleetd-finish 145: routing_provenance.rs checks the provenance of fleetd launches and the refusal of a closed pool (T-124) (`7f4f9dfa`)
- fleetd-finish 145: startup_error.rs checks the startup error of the attempts that fleetd launches (T-124) (`712fa672`)
- fleetd-finish 145: scenario.rs step 8 checks the refusal of a launch of opus with every pool exhausted (T-124) (`aa2c0bed`)
- fleetd-finish 145: the 6 e2e files stop their test fleetd and its jobs before teardown (T-124) (`e7d3be9c`)
- fleetd-finish 145: comment sweep of the 6 e2e files of T-124 removes 111 comment lines (T-142) (`0d001eb2`)
- fleetd-finish 145: scenario.rs story moves steps 5, 10 and 11 into helpers to stay under the complexity limit of 25 (T-124) (`5dbcbd5d`)
- comment sweep T-124: scenario.rs drops 4 comments of the step helpers and names the cost arguments of 1 report of every call (T-147) (`04479e77`)

<details><summary>6 files changed</summary>

- `crates/horch-e2e/tests/brief.rs`
- `crates/horch-e2e/tests/restored_pane.rs`
- `crates/horch-e2e/tests/routing_provenance.rs`
- `crates/horch-e2e/tests/scenario.rs`
- `crates/horch-e2e/tests/startup_error.rs`
- `crates/horch-e2e/tests/tell_grace.rs`

</details>

Landed by qa-engineer-23 at `04479e7799b7`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-124 -->

<!-- horch:task T-107 -->
### T-107 CI FIX-10: fix unexplained failures in horch-e2e fleet_start.rs, p0_ideas.rs and skills_exposure.rs

- ci-fix-10: the skills_exposure, fleet_start and p0_ideas e2e tests pass under fleetd (T-107) (`825cd889`)
- comment sweep of T-107: remove 18 comments; name the conformance check, the job limits and the probe flags (T-145) (`c174ef71`)

<details><summary>6 files changed</summary>

- `crates/horch-e2e/expected-failures/skills_exposure.txt`
- `crates/horch-e2e/src/bin/fake-claude.rs`
- `crates/horch-e2e/src/bin/fake-codex.rs`
- `crates/horch-e2e/tests/common/attempt.rs`
- `crates/horch-e2e/tests/fleet_start.rs`
- `crates/horch-e2e/tests/skills_exposure.rs`

</details>

Landed by qa-engineer-19 at `c174ef71f3a2`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-107 -->

<!-- horch:task T-88 -->
### T-88 CI FIX-8: migrate stale tests in horch-e2e p0_dispatch.rs, p0_landing.rs, p0_liveness.rs, p0_rescue.rs, p0_supervision.rs and p1_release.rs

- ci-fix-8 114: the e2e tests of p0_liveness, p0_rescue, p0_supervision and p1_release seed their run and tasks with ledger_seed in place of the retired horch ledger run new and add; p0_rescue lets a running fleetd make each tree and the rescue ref (T-88) (`0e17363d`)
- ci-fix-8 114: the p0_landing test of 2 conflicting features waits for its running jobs at its end, so the teardown of its fleetd leaks none (T-88) (`2681f2db`)
- ci-fix-8 114: p0_rescue sets worktree_list_period_s to 5 so the rescue starts within CAUGHT_S, and its bind waits for the rescued tree; p1_release waits for fleetd to remove the lock of the old watchdog (T-88) (`7ddbb093`)
- comment sweep T-88: remove 13 comments that the diff adds to 5 e2e tests (T-140) (`93441039`)
- ci-fix-8 114: the p0_supervision restart test waits for the old fleetd to end after the row names the new one (T-88) (`8fe60287`)
- comment sweep T-88: remove 1 comment that the diff adds to p0_supervision.rs (T-149) (`060d3540`)

<details><summary>5 files changed</summary>

- `crates/horch-e2e/tests/p0_landing.rs`
- `crates/horch-e2e/tests/p0_liveness.rs`
- `crates/horch-e2e/tests/p0_rescue.rs`
- `crates/horch-e2e/tests/p0_supervision.rs`
- `crates/horch-e2e/tests/p1_release.rs`

</details>

Landed by qa-engineer-13 at `060d354099bf`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-88 -->

<!-- horch:task T-62 -->
### T-62 Fix R4-F2: wake ChannelServer::stop and Drop without depending on the socket pathname (RR-8, FM-15)

- fleetd-finish 71: ChannelServer::stop and Drop end without the socket path - a fleetd-channel-accept thread waits in accept, and the server thread reads the stop flag each ACCEPT_RETRY (R4-F2, T-62) (`821043ce`)
- comment sweep T-62: remove 8 narrating comments, keep 2 platform notes, split STOP_POLL from ACCEPT_RETRY (T-154) (`c62aa322`)

<details><summary>2 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`
- `crates/horch-core/tests/fleetd_channel.rs`

</details>

Landed by opus-17 at `c62aa322e135`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-62 -->

<!-- horch:task T-156 -->
### T-156 The question text of an idea-lead accepts line separators and bidi characters, and the status line does not escape it

- fleetd-finish 151: the question of an ask note refuses line separators and bidi characters, and the waiting item escapes it (T-156) (`b817549d`)
- fleetd-finish 151: comment sweep of T-156 removes 3 comments that retell the code (T-161) (`4a8a79df`)

<details><summary>4 files changed</summary>

- `crates/horch-core/src/fleetd/ideas/adapters/idea_notes_reader.rs`
- `crates/horch-core/src/fleetd/ideas/application/waiting.rs`
- `crates/horch-core/tests/idea_notes.rs`
- `crates/horch-core/tests/idea_waiting.rs`

</details>

Landed by opus-hardening-6 at `4a8a79df27d6`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-156 -->

<!-- horch:task T-71 -->
### T-71 p1_pass: move rr4, rr7 and fm11 to the push flow and remove their expected failures

- p1_pass: move rr4, rr7 and fm11 to the push flow and remove their expected failures (T-71) (`88064cdf`)
- comment sweep T-71: remove 17 comments that retell the code in p1_pass.rs (T-155) (`121cb74c`)

<details><summary>2 files changed</summary>

- `crates/horch-e2e/expected-failures/p1_pass.txt`
- `crates/horch-e2e/tests/p1_pass.rs`

</details>

Landed by opus-16 at `121cb74c2a21`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-71 -->

<!-- horch:task T-17 -->
### T-17 D2 SR-3 external-message boundary: test and enforce external messaging denial from a verifier and from a worker

- T-17 SR-3: worker profiles deny gh issue comment, gh pr comment, gh pr review and gh discussion (`8af27a86`)
- T-17 SR-3: add Network::Local and sandboxed_local to the platform sandbox (`a87b4423`)
- T-17 SR-3: a task check has no off-host network from now (`043cc3d5`)
- T-17 comment sweep: remove 13 narration and banner comments; the verifier check test spawns its webhook thread only when a check sandbox exists (`139ba977`)

<details><summary>32 files changed</summary>

- `crates/horch-core/src/fleetd/worker_profiles/adapters/check_sandbox.rs`
- `crates/horch-core/src/fleetd/worker_profiles/domain/launch_env.rs`
- `crates/horch-core/src/harness/claude.rs`
- `crates/horch-core/src/harness/launch.rs`
- `crates/horch-core/src/skills.rs`
- `crates/horch-core/tests/golden_prompts.rs`
- `crates/horch-core/tests/oracles/launch/architect-reviewer.json`
- `crates/horch-core/tests/oracles/launch/backend-developer.json`
- `crates/horch-core/tests/oracles/launch/designer.json`
- `crates/horch-core/tests/oracles/launch/frontend-developer.json`
- `crates/horch-core/tests/oracles/launch/idea-lead.json`
- `crates/horch-core/tests/oracles/launch/opus-architect.json`
- `crates/horch-core/tests/oracles/launch/opus-domain.json`
- `crates/horch-core/tests/oracles/launch/opus-hardening.json`
- `crates/horch-core/tests/oracles/launch/opus-verify.json`
- `crates/horch-core/tests/oracles/launch/opus.json`
- `crates/horch-core/tests/oracles/launch/orchestration-orchestrator.json`
- `crates/horch-core/tests/oracles/launch/product-lead.json`
- `crates/horch-core/tests/oracles/launch/qa-engineer.json`
- `crates/horch-core/tests/oracles/launch/researcher.json`
- and 12 more files

</details>

Landed by qa-engineer-28 at `139ba977456a`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-17 -->

<!-- horch:task T-6 -->
### T-6 C3 land T-558: intake and approval e2e tests (FR-1, FR-10, FR-12, FM-30, FM-32)

- e2e: intake and approval through the binary (FR-1, FR-10, FR-12, FM-30, FM-32) (`a1bf01a8`)
- e2e: FR-10 tries the approval while the idea is in awaiting_approval (fake key await_approval) (`04ff549c`)
- e2e: fr10 shows the ledger when the approval inside passes (`31a0cd27`)
- Merge branch 'fleet/fleetd-build' into fleet/fleetd-build-T558-the-e2e-tests-of-intake-and-approval-fr (`5c36c57c`)
- Merge remote-tracking branch 'origin/fleet/fleetd-build-T558-the-e2e-tests-of-intake-and-approval-fr' into fleet/fleetd-finish-T6-c3-land-t-558-intake-and-approval-e2e-te (`070de0aa`)
- comment sweep T-6: drop 37 narrating comments in idea_agent.rs and p6_intake.rs (`efea1ed0`)
- C3 T-558: fm32 waits for the decided row of the request file, not the pending row that fleetd inserts first; it failed under load avg 26 with 'pending -' (T-6) (`41bfcb0b`)

<details><summary>2 files changed</summary>

- `crates/horch-e2e/src/idea_agent.rs`
- `crates/horch-e2e/tests/p6_intake.rs`

</details>

Landed by opus-11 at `41bfcb0b5153`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-6 -->

<!-- horch:task T-37 -->
### T-37 Fix R1-F1: run complexity-sweep in the worker sandbox profile (SR-4, SR-6)

- fleetd-finish 35: complexity-sweep runs cargo clippy and the analyser in the sandbox profile of the task's worker through check_launch and run_check; a task with no worker or no profile gets SR-4 and no verdict (R1-F1, SR-4, SR-6) (T-37) (`ad1a4878`)
- comment sweep T-37: remove 19 comments that retell the code (T-152) (`bc66acfc`)

<details><summary>5 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`
- `crates/horch-core/src/taskledger/finish/complexity.rs`
- `crates/horch-core/src/taskledger/finish/mod.rs`
- `crates/horch-core/tests/check_sandbox.rs`
- `crates/horch-core/tests/verifier_review.rs`

</details>

Landed by opus-hardening-3 at `bc66acfcc3b2`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-37 -->

<!-- horch:task T-138 -->
### T-138 CI1: second CI run of the run branch after the harness fix, and the difference to CI0

- CI1: the second CI result of fleetd on 3 OSes (`0d956b1e`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/ci-run-2.md`

</details>

Landed by qa-engineer-29 at `0d956b1e1e2e`.
<!-- /horch:task T-138 -->

<!-- horch:task T-157 -->
### T-157 The launcher fails with "the start time of process N is unreadable" on a loaded host

- T-157: the launcher waits out the exit window of a child before it fails a launch (`bd55b1f5`)
- T-157: comment sweep removes 3 comments that retell the code and keeps the macOS exit-window note (T-164) (`200b9468`)

<details><summary>1 file changed</summary>

- `crates/horch-core/src/fleetd/worker_profiles/adapters/launcher.rs`

</details>

Landed by opus-18 at `200b9468f5e0`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-157 -->

<!-- horch:task T-97 -->
### T-97 CI FIX-10: fix the Linux ECONNRESET failure in horch-core tests/fleetd_channel.rs

- fleetd channel: drain the unread bytes of a line over the limit after its refusal, so a close with unread bytes does not reset the client on Linux (T-97) (`7d27c69e`)
- comment sweep T-97: drop 3 narrating comments in channel_server.rs, keep the Linux ECONNRESET note on drain_input (`e339a045`)

<details><summary>1 file changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`

</details>

Landed by qa-engineer-31 at `e339a0453562`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-97 -->

<!-- horch:task T-54 -->
### T-54 Strengthen R1 FM-12 e2e job tests in p1_jobs.rs for ledger_seed, stalled channel client, and bounded daemon stop

- p1_jobs: seed the run with ledger_seed, add the stalled channel client and bounded SIGTERM test, wait on the fleet clock (T-54) (`e5c959ef`)
- comment sweep: remove 10 comments from the T-54 p1_jobs diff; name the shared-checkout update, the tokenless request and the 2-pass wait in code (T-162) (`9ecd412d`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/tests/p1_jobs.rs`

</details>

Landed by qa-engineer-30 at `9ecd412d6adb`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-54 -->

<!-- horch:task T-160 -->
### T-160 The boot conformance job of a test fleetd fails on a loaded host, and the FR-10 test then approves the wrong idea

- fleetd-finish 153: the FR-10 fake approves the idea named by [fr10:target=<id>], not the first idea in awaiting_approval; a system idea of a failed boot conformance job no longer changes the result (T-160) (`8654df8c`)
- fleetd-finish 153: comment sweep of T-160 removes 3 comments that retell the code (T-167) (`be881fba`)

<details><summary>2 files changed</summary>

- `crates/horch-e2e/src/fr10_agent.rs`
- `crates/horch-e2e/tests/p6_fr10_approval.rs`

</details>

Landed by opus-19 at `be881fba51dd`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-160 -->

<!-- horch:task T-5 -->
### T-5 C2 T-899: remove operator-class requests and operator token access from the worker wrapper

- C2 T-899: the worker wrapper reads no operator token and sends no operator request (T-5) (`8db249b8`)
- Comment sweep T-5: remove 36 narration comments from the T-899 diff (`d3cba51f`)

<details><summary>6 files changed</summary>

- `crates/horch-core/src/execution/lifecycle.rs`
- `crates/horch-core/src/execution/service.rs`
- `crates/horch-core/src/execution/store.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_client.rs`
- `crates/horch-core/tests/p5_operator_boundary.rs`
- `crates/horch-e2e/tests/p5_operator_boundary.rs`

</details>

Landed by opus-hardening-4 at `d3cba51f2556`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-5 -->

<!-- horch:task T-126 -->
### T-126 A worker that fleetd launches in exec mode gets no skills briefing

- fleetd-finish 147: the exec launch puts the skills briefing of Bundle::briefing_in before its prompt (T-126) (`703f37ff`)
- fleetd-finish 147: the doc of claude::exec_command names the skills briefing before the START.md path (T-126) (`e70e1832`)
- fleetd-finish 147: a fresh exec prompt names the START.md line, and the fake exec agent reads the path from the last line of the prompt (T-126) (`5162086b`)
- fleetd-finish 147: the conformance render replaces each skills bundle folder with a word, so the codex adapter hash stays stable with the briefing (T-126) (`d778ca0a`)
- fleetd-finish 147: comment sweep of T-126 removes 10 comments that retell the code (T-126) (`88802e20`)

<details><summary>7 files changed</summary>

- `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs`
- `crates/horch-core/src/harness/claude.rs`
- `crates/horch-core/src/harness/launch.rs`
- `crates/horch-core/tests/harness_defaults.rs`
- `crates/horch-core/tests/p7_conformance_job.rs`
- `crates/horch-e2e/expected-failures/skills_exposure.txt`
- `crates/horch-e2e/src/exec.rs`

</details>

Landed by opus-20 at `88802e20552a`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-126 -->

<!-- horch:task T-61 -->
### T-61 Fix R4-F1: reconcile refused or rejected queued requests and do not report queued session_records as committed (RR-8, FM-15)

- fleetd-finish 70: a refused queued request and a rejected request file each open 1 needs-you item, and ExecutionStore::update_written says when a session_records change is only queued (R4-F1, RR-8, FM-15) (T-61) (`b922acad`)
- comment sweep T-61: remove 6 comments that repeat the code, a module doc or a test name (T-170) (`18c8ae35`)

<details><summary>4 files changed</summary>

- `crates/horch-core/src/execution/store.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_spool.rs`
- `crates/horch-core/tests/execution_store.rs`
- `crates/horch-core/tests/fleetd_request_files.rs`

</details>

Landed by opus-23 at `18c8ae35ce23`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-61 -->

<!-- horch:task T-63 -->
### T-63 Fix R4-F3: implement Windows ACL-based protected-file checks for request spool and untrusted files (SR-5)

- fleetd-finish 72: a request file and the token file pass the protected checks on Windows by owner SID and DACL, and stay mode 0600/0700 on unix (R4-F3, SR-5) (T-63) (`28b7a295`)
- comment sweep T-63: remove 13 comments that repeat the code, a cfg attribute or a function name (T-174) (`920f573d`)

<details><summary>11 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_client.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/adapters/protected_files.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_spool.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/adapters/token_file.rs`
- `crates/horch-core/tests/fleetd_request_files.rs`
- `crates/horch-core/tests/protected_files.rs`
- `crates/horch-platform/src/fs.rs`
- `crates/horch-platform/src/linux/fs.rs`
- `crates/horch-platform/src/mac/fs.rs`
- `crates/horch-platform/src/unix/fs.rs`
- `crates/horch-platform/src/windows/fs.rs`

</details>

Landed by opus-hardening-8 at `920f573ddcf7`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-63 -->

<!-- horch:task T-119 -->
### T-119 Fix the red tests of `crates/horch-e2e/tests/lifecycle.rs`

- fleetd-finish 140: the lifecycle tests drive fleetd with a seeded task, since horch spawn is retired (T-119) (`ebaa6a3d`)
- fleetd-finish 140: comment sweep of T-119 removes 27 comments of lifecycle.rs, and FORBIDDEN takes its value from horch_platform::start::FORBIDDEN_ENV (T-119) (`07915763`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/tests/lifecycle.rs`

</details>

Landed by qa-engineer-34 at `079157636809`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-119 -->

<!-- horch:task T-169 -->
### T-169 C4 land T-559: dispatcher and sub-orchestrator e2e tests (FR-2, FM-16, FM-29, FM-31)

- fleetd reads the product clock: IdeaPorts::now and the rescue ref expiry follow HORCH_NOW (T-559) (`09b80a8f`)
- e2e: FR-2, FM-16, FM-29 and FM-31 through the binary (T-559) (`7e525206`)
- e2e p6_sub_orchestrators: a failed wait shows the notes of the fakes and interview.md; cargo fmt (T-559) (`d4160765`)
- Merge branch 'fleet/fleetd-build' into fleet/fleetd-build-T559-the-e2e-tests-of-the-dispatcher-and-the (`4a4d67f7`)
- Merge remote-tracking branch 'origin/fleet/fleetd-build-T559-the-e2e-tests-of-the-dispatcher-and-the' into fleet/fleetd-finish-T169-c4-land-t-559-dispatcher-and-sub-orchest (`5c1123b8`)
- C4 T-559: the e2e tests of p6_sub_orchestrators pass; FM-16 waits for T-897 (T-169) (`0d7880c2`)
- Comment sweep of T-169: delete 95 comment lines; name the die-once claude wrapper and the Monday start time (T-176) (`e5c8d131`)

<details><summary>5 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`
- `crates/horch-e2e/expected-failures/p6_sub_orchestrators.txt`
- `crates/horch-e2e/src/idea_agent.rs`
- `crates/horch-e2e/tests/p6_sub_orchestrators.rs`
- `crates/horch/src/cmd/fleetd.rs`

</details>

Landed by opus-22 at `e5c8d131e241`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-169 -->

<!-- horch:task T-65 -->
### T-65 Fix R4 test gaps: prove inherited live HORCH_LEDGER_DB isolation and Codex operator-boundary probe (RR-5, SR-8, FM-32)

- fleetd-finish 74: the sealed harness test proves an inherited live HORCH_LEDGER_DB and FLEET_ROOT never reach a child, and the p7 test proves the Codex version and network_proxy refusal with the 3-act sandbox probe; the gate test waits for the boot conformance jobs and compares the state of its own feature (RR-5, SR-8, FM-32) (T-65) (`d5d89436`)
- comment sweep T-65: remove 13 comments, 1 of them a banner, that repeat the asserts, the test names or the wait label (T-175) (`e3b51522`)

<details><summary>2 files changed</summary>

- `crates/horch-core/tests/p7_operator_boundary.rs`
- `crates/horch-e2e/tests/p5_operator_boundary.rs`

</details>

Landed by qa-engineer-35 at `e3b515222bb0`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-65 -->

<!-- horch:task T-163 -->
### T-163 FR-10: an agent can approve an idea while its own attempt is launching and has no recorded pid

- fleetd-finish 154: an idea_approve is refused while an attempt launches with no recorded process (FR-10, T-163) (`f70d08c0`)
- fleetd-finish 154: the outlive fake of fr10_b starts its wait for the end of its attempt at the approval of its idea (T-163) (`0dd93cac`)
- Comment sweep of T-163: delete 32 comment lines; name the wait for the own approval and the launching fact of attempt_fact (T-177) (`24b49204`)

<details><summary>9 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/adapters/peer_process.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/domain/authorize.rs`
- `crates/horch-core/tests/p6_authorize.rs`
- `crates/horch-core/tests/peer_process.rs`
- `crates/horch-e2e/src/fr10_agent.rs`
- `crates/horch-e2e/tests/p6_fr10_approval.rs`
- `crates/horch/src/cmd/idea/mod.rs`

</details>

Landed by opus-hardening-7 at `24b49204c253`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-163 -->

<!-- horch:task T-96 -->
### T-96 CI FIX-10: fix the concurrent dirlock failure in horch-core tests/execution_store.rs

- CI FIX-10: a write to a ledger file whose first writer has not migrated it yet reads no session rows instead of failing with 'no such table: session_records' (T-96) (`a7ada56b`)
- comment sweep T-96: remove 1 doc comment on has_session_records_table that explains the first-writer race (T-179) (`f1e2b0b3`)

<details><summary>2 files changed</summary>

- `crates/horch-core/src/execution/store.rs`
- `crates/horch-core/tests/execution_store.rs`

</details>

Landed by qa-engineer-36 at `f1e2b0b3b6fe`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-96 -->

<!-- horch:task T-8 -->
### T-8 C5 land T-561: judge-pair e2e tests and product fixes (RR-3, FM-9, FM-33)

- fleetd-p6 59: the e2e tests of the judge pair - RR-3, FM-9, FM-33 and the 2 cancel tests of the gate, through horch idea add --direct and a test fleetd (T-561) (`130f18ba`)
- fleetd-p6 59: the judge pair tests read the plan of their own idea, the refusal names the judged task as T-<seq>, and the fix round and FM-9 asserts read exact task moves (T-561) (`e84a37ec`)
- fleetd-p6 59: the judge pair tests declare the fast and full gates of the project, which the plan validator needs, and a failed wait shows the idea folder and plan.verdict.json (T-561) (`f61db9fb`)
- Merge branch 'fleet/fleetd-build' into fleet/fleetd-build-T561-the-e2e-tests-of-the-judge-pair-rr-3-fm (`95ef93f3`)
- Merge branch 'fleet/fleetd-build' into fleet/fleetd-build-T561-the-e2e-tests-of-the-judge-pair-rr-3-fm (`349b7622`)
- fleetd-p6 59: the judge pair tests make the fleet workspace with Harness::fleet_workspace of T-890 (T-561) (`f7dc4052`)
- fleetd-p6 59: a judge task with no check command passes the check step, so its structural check and its verdict decide; each other task with no check is still refused (fault 7, T-561) (`0eafdc37`)
- fleetd-p6 59: the check step of a judge task with no check command gives the true reason 'a judge task is decided by its structural check and its verdict' (verification::plan_judge), and the unit test asserts it (fault 7, T-561) (`8a207cb6`)
- fleetd-p6 59: a --direct judge task gets its verdict file as its 1 output and Own row (<first Own path of the judged ticket>.review.md, else <key>.review.md); the plan validator refuses a judge task without exactly 1 .md output, an output path outside the repository and 2 tasks with 1 output path; the fake judge commits the Own path of its task (fault 8, T-561) (`609c8bf2`)
- fleetd-p6 59: the failure context of the judge pair tests shows the profile of each task and the health facts of each profile (T-561) (`0729b301`)
- Merge branch 'fleet/fleetd-build' into fleet/fleetd-build-T561-the-e2e-tests-of-the-judge-pair-rr-3-fm (`304f5f51`)
- fleetd-p6 59: a --direct judge task gets the first healthy profile of another provider with a sandbox at the plan time; with none, the refused plan names each unhealthy profile and its cause (fault 9, T-561) (`1286c1e6`)
- fleetd-p6 59: the judge pair tests put the ledger and the fleet root outside the state folder, as in production, so the conformance job passes for codex (T-561) (`8d102217`)
- fleetd-p6 59: the judge pair tests add the idea after the conformance run of each CLI ended, so the plan sees the health of each profile (T-561) (`66f996b5`)
- Merge branch 'fleet/fleetd-build' into fleet/fleetd-build-T561-the-e2e-tests-of-the-judge-pair-rr-3-fm (`55191d7d`)
- fleetd-p6 59: the judge pick skips a council seat (a reserved top tier) and each profile that the profile rules of the plan validator refuse, with the same rule functions; the refused plan names each skipped profile and its cause (fault 10, T-561) (`aab758b3`)
- fleetd-p6 59: the failure context of the judge pair tests shows the jobs, the task facts with their data and the attempt facts (T-561) (`e3418f80`)
- Merge branch 'fleet/fleetd-build' into fleet/fleetd-build-T561-the-e2e-tests-of-the-judge-pair-rr-3-fm (`2912cdbc`)
- fleetd-p6 59: FM-33 waits for each judge verdict as 1 step with its own deadline; the failure context shows the time of each event (T-561) (`717882da`)
- fleetd-p6 59: I2 promotes the judge task of a fix round only when its judge branch holds the new judged tip, by the rule of judge_branch::follow, so fleetd moves the branch before the next judge attempt (judge pair step 6; fault 11, T-561) (`147f7878`)
- and 4 more commits

<details><summary>16 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`
- `crates/horch-core/src/fleetd/ideas/application/direct.rs`
- `crates/horch-core/src/fleetd/ideas/domain/plan_rules_judges.rs`
- `crates/horch-core/src/fleetd/ideas/domain/plan_validator.rs`
- `crates/horch-core/src/fleetd/integration/application/judge_branch.rs`
- `crates/horch-core/src/fleetd/task_execution/application/judge_pair.rs`
- `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`
- `crates/horch-core/src/fleetd/task_execution/domain/verification.rs`
- `crates/horch-core/tests/idea_direct.rs`
- `crates/horch-core/tests/judge_branch.rs`
- `crates/horch-core/tests/judge_pair.rs`
- `crates/horch-core/tests/plan_rules_judges.rs`
- `crates/horch-core/tests/plan_validator.rs`
- `crates/horch-e2e/src/agent.rs`
- `crates/horch-e2e/src/idea_agent.rs`
- `crates/horch-e2e/tests/p6_judge_pair.rs`

</details>

Landed by opus-24 at `540c5e65fee0`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-8 -->

<!-- horch:task T-45 -->
### T-45 Fix R5-F3: pass task kind into verification::plan so a judge task skips the command check and runs its structural verdict check (RR-3, FM-9, FM-33)

- R5-F3: verification::Facts carries the effective task kind, and verification::plan decides not_applicable with JUDGE_NO_CHECK for a changed judge task with no check command (RR-3, FM-9, FM-33; T-45) (`00015d75`)
- Comment sweep T-45: remove the doc comment of work_kind_of that retells the code (T-180) (`e6dc16f7`)

<details><summary>2 files changed</summary>

- `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`
- `crates/horch-core/src/fleetd/task_execution/domain/verification.rs`

</details>

Landed by opus-26 at `e6dc16f7dae2`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-45 -->

<!-- horch:task T-46 -->
### T-46 Fix R5-F6: give direct-plan judge tasks a deterministic output, a generated verdict-format ticket, and effective kind `judge` in task.yaml (FR-4, RR-3, RR-12)

- R5-F6: a direct judge task gets its verdict output, a generated verdict-format ticket tickets/02-<key>-judge.md, and kind: judge in task.yaml from spec_json (T-46) (`552632b8`)
- Comment sweep T-46: remove 6 comments that the T-46 diff added; spec_kind reads only the plan kind, and the judge dispatch test binds the work task, then the judge task (`870c4dd7`)

<details><summary>3 files changed</summary>

- `crates/horch-core/src/fleetd/ideas/application/direct.rs`
- `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`
- `crates/horch-core/tests/idea_direct.rs`

</details>

Landed by opus-27 at `870c4dd76514`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-46 -->

<!-- horch:task T-9 -->
### T-9 C6 land T-603: exec-mode e2e tests

- fleetd-p7 33: e2e tests of exec mode through a test fleetd - each CLI runs its task in exec mode, a usage limit restarts on the fallback, a question waits in blocked_on_user, the agy context limit hands off and its loop opens 1 needs-you item, each new attempt gets 256000; 7 tests listed for T-892 (no wrapper pid on a push attempt, the quota hint of exit.json); the exec fakes play the agent keys, and the new key ask_file runs horch ask (`58be4c7f`)
- fleetd-p7 33: the base already runs the agent keys in exec mode; remove the 2nd call (`71e03a11`)
- fleetd-p7 33: p7_exec labels the herdr workspace with the project folder name, as horch fleet does, so a launch with no recorded pane finds it (`4210c3da`)
- fleetd-p7 33: p7_exec keeps the ledger and the fleet root in the state folder with no FLEET_ROOT, so the worker of a pane finds the same paths (`1cccb143`)
- fleetd-p7 33: the task of p7_exec has Review: none, so its landing needs no code-sweep skill (`a09213ce`)
- fleetd-p7 33: the scenario script of p7_exec reads the attempt number with sh builtins only; the test checks no outcome word for an attempt that did the task (`87d141d0`)
- fleetd-p7 33: with T-890 and T-892, 6 tests of p7_exec are green; the list keeps the 2 usage-limit tests for T-892 (`5ae8a6bc`)
- fleetd-p7 33: the 2 usage-limit tests of p7_exec have the owner T-895 (the restart on the fallback uses the old claim spawn) (`e7f7af51`)
- Merge branch 'fleet/fleetd-build' into fleet/fleetd-build-T603-the-e2e-test-of-exec-mode-a-task-runs-to (`22c4e039`)
- Merge branch 'fleet/fleetd-build' into fleet/fleetd-build-T603-the-e2e-test-of-exec-mode-a-task-runs-to (`0829c310`)
- fleetd-p7 33: p7_exec shows the health events of its profiles in the diagnostics of a failed wait (`f70633cb`)
- Merge remote-tracking branch 'origin/fleet/fleetd-build-T603-the-e2e-test-of-exec-mode-a-task-runs-to' into fleet/fleetd-finish-T9-c6-land-t-603-exec-mode-e2e-tests (`23fb60d0`)
- fleetd-finish C6 T-9: p7_exec removes agy from the bin folder and ignores :566 until T-895 (`00828996`)
- T-9 comment sweep: p7_exec drops 39 comments and names the agy removal skip_agy_conformance (T-183) (`1525e74f`)

<details><summary>4 files changed</summary>

- `crates/horch-e2e/expected-failures/p7_exec.txt`
- `crates/horch-e2e/src/agent.rs`
- `crates/horch-e2e/src/exec.rs`
- `crates/horch-e2e/tests/p7_exec.rs`

</details>

Landed by opus-25 at `1525e74f2c29`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-9 -->

<!-- horch:task T-52 -->
### T-52 Fix R3 weak tests: verify the end-to-end push-dispatch verifier chain (T-601) and non-legacy planned task kinds in task.yaml (FR-4, RR-3, FM-9)

- R3 weak tests: p4_verifier runs the real push dispatch (fleet workspace, fake agent through the launcher), the 4 red tests pass, and new tests cover the push chain (T-601), SR-3 off-host check and planned non-legacy kinds in task.yaml (FR-4, RR-3, FM-9; T-52) (`16edf4cf`)
- Comment sweep T-52: delete 33 narration comments in task_files.rs and p4_verifier.rs (`39de338b`)

<details><summary>2 files changed</summary>

- `crates/horch-core/tests/task_files.rs`
- `crates/horch-e2e/tests/p4_verifier.rs`

</details>

Landed by qa-engineer-37 at `39de338b6ff7`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-52 -->

<!-- horch:task T-10 -->
### T-10 C7 land T-895: restart on fallback after a usage limit through push dispatch

- RR-11: a usage limit moves the task to ready for its fallback, and the push dispatch binds A2 (`0c225a3d`)
- Order of 1 pass: the files of an attempt act after its first heartbeat, and rung 5 does not judge an attempt with no recorded process (`ecb45971`)
- Merge branch 'fleet/fleetd-build' into fleet/fleetd-build-T895-a-task-that-restarts-on-its-fallback-aft (`0fc0b0d3`)
- Merge origin/fleet/fleetd-build-T895-a-task-that-restarts-on-its-fallback-aft (T-10, C7) (`4890da31`)
- T-10 comment sweep: drop 23 comments of the T-895 diff (T-185) (`6e8aa00a`)
- T-10 comment sweep: first_line is a String, and p7_exec names its fleet root outside the worker sandbox (T-185) (`51d0b243`)

<details><summary>11 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`
- `crates/horch-core/src/fleetd/task_execution/application/launch.rs`
- `crates/horch-core/src/fleetd/task_execution/application/quota.rs`
- `crates/horch-core/tests/fleetd_pass_p2.rs`
- `crates/horch-core/tests/fleetd_pass_p7.rs`
- `crates/horch-e2e/expected-failures/p7_exec.txt`
- `crates/horch-e2e/tests/p7_exec.rs`
- `crates/horch/src/cmd/fleetd.rs`
- `crates/horch/src/cmd/recipes.rs`
- `crates/horch/src/cmd/spawn.rs`
- `crates/horch/src/cmd/taskledger_spawn.rs`

</details>

Landed by opus-28 at `51d0b2433896`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-10 -->

<!-- horch:task T-81 -->
### T-81 CI FIX-8: migrate stale unit tests in horch cmd/recipes.rs, cmd/rescuecmd.rs and cmd/sessionscmd.rs

- CI FIX-8: migrate 4 stale unit tests of horch cmd (T-81) (`1e79a53b`)
- Comment sweep of T-81: remove 2 narrating doc comments in test code (T-186) (`07d0bfed`)

<details><summary>3 files changed</summary>

- `crates/horch/src/cmd/recipes.rs`
- `crates/horch/src/cmd/rescuecmd.rs`
- `crates/horch/src/cmd/sessionscmd.rs`

</details>

Landed by qa-engineer-38 at `07d0bfedd847`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-81 -->

<!-- horch:task T-188 -->
### T-188 Land the work of task T-11 (unit C8, T-897: worker question in a push attempt) with a Check that has no suite that is red on the base

- T-897: a question file of a push attempt gives 1 questions row with its owner (`fbab334c`)
- T-897: fmt; scope the fake lead ask check; move the owner-note test to T-562 (`051891d9`)
- Merge branch 'fleet/fleetd-build' into fleet/fleetd-build-T897-a-question-of-a-worker-in-a-push-attempt (`b172eed7`)
- Merge remote-tracking branch 'origin/fleet/fleetd-build-T897-a-question-of-a-worker-in-a-push-attempt' into fleet/fleetd-finish-T11-c8-land-t-897-worker-question-in-a-push (`7b4af3e7`)
- T-11 C8: p4_question_file gets its questions rows from the fleetd outbox ingest, since T-897 removed workers::sync_questions (`d9bb464d`)
- T-11 C8: p5_cli_writers asserts that triage DELETE queues 1 triage_apply job, since T-888 retired the refusal (`3e76075b`)
- T-188: remove the fm16 entry of p6_sub_orchestrators expected failures, since the T-897 outbox ingest writes the questions row (3 of 3 green) (`05021c70`)
- Comment sweep T-188: delete 58 comments (121 lines) that retell code or narrate tests in 9 files (T-189) (`02b8964e`)

<details><summary>33 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/adapters/watchdog_duties.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_cli.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`
- `crates/horch-core/src/fleetd/ideas/application/questions.rs`
- `crates/horch-core/src/fleetd/task_execution/adapters/outbox_writer.rs`
- `crates/horch-core/src/fleetd/task_execution/adapters/result_reader.rs`
- `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`
- `crates/horch-core/src/fleetd/task_execution/application/ingest.rs`
- `crates/horch-core/src/fleetd/task_execution/application/outbox_ingest.rs`
- `crates/horch-core/src/fleetd/task_execution/domain/outbox.rs`
- `crates/horch-core/src/fleetd/task_execution/domain/task_files.rs`
- `crates/horch-core/src/taskledger/tasks.rs`
- `crates/horch-core/src/taskledger/workers.rs`
- `crates/horch-core/tests/fleetd_handlers_cli.rs`
- `crates/horch-core/tests/fleetd_pass_p2.rs`
- `crates/horch-core/tests/idea_release.rs`
- `crates/horch-core/tests/outbox_ingest.rs`
- `crates/horch-core/tests/outbox_writer.rs`
- `crates/horch-core/tests/question_answers.rs`
- `crates/horch-core/tests/question_flow.rs`
- and 13 more files

</details>

Landed by opus-30 at `02b8964edc4d`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-188 -->

<!-- horch:task T-104 -->
### T-104 CI FIX-10: fix unexplained CLI test failures in horch p5_cli_writers.rs and r_cli_cmds_ledger.rs

- T-104 FIX-10: r_cli_cmds_ledger serves session_records with an in-process fleetd, since T-884 sends the legacy session writes as requests that need the operator token (`52f46526`)
- T-104 comment sweep: r_cli_cmds_ledger drops 2 comments that retell the test setup and the World field order (`0b9024e9`)

<details><summary>1 file changed</summary>

- `crates/horch/tests/r_cli_cmds_ledger.rs`

</details>

Landed by qa-engineer-42 at `0b9024e97d1e`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-104 -->

<!-- horch:task T-191 -->
### T-191 Land the fix of task T-103 (4 unit tests of the horch binary) with a valid Check command

- CI FIX-10: update 3 horch unit tests to the Phase 5-7 behavior - ctx_06 expects the 256,000 window and the astra windows key, trf_45 serves worker_progress from an in-process fleetd and makes its state dir, the ledger check test drops the retired ledger add (T-103) (`155e3b7d`)
- Comment sweep T-191: remove 4 narrating comments from the ctx_06 and trf_45 tests (T-194) (`13ff1dd0`)

<details><summary>3 files changed</summary>

- `crates/horch/src/cmd/context.rs`
- `crates/horch/src/cmd/messaging.rs`
- `crates/horch/src/cmd/taskledger.rs`

</details>

Landed by qa-engineer-43 at `13ff1dd0d10b`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-191 -->

<!-- horch:task T-12 -->
### T-12 C9 land T-562: questions, replans, and needs-you e2e tests plus the owner-note test from T-897

- p6_questions: e2e tests of RR-12, OR-3, FM-21 and FM-35 through the binary (T-562) (`a5993f3f`)
- Merge T-562: e2e tests of questions, replans and needs-you (C9, T-12) (`e777b038`)
- T-12 C9: p6_questions adds the owner-note test answer_of_the_owner_reaches_the_next_attempt, moved from T-897 (051891d9) (`060834a7`)
- T-12 C9: p6_questions fm35 answers from inside the owner process, and fm21 drives the clock in steps under the launch timeout (`2ad5fdc5`)
- T-12 C9: the fm35 owner watcher reads the question with the read builtin and calls /bin/sleep, since the PATH of a pane has no /bin; World::context shows the watcher log (`67b1f5ba`)
- T-12 comment sweep: p6_questions drops 67 comments, and a const assert holds DRIVE_STEP_S under START_TIMEOUT_S (`f41fadf7`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/tests/p6_questions.rs`

</details>

Landed by opus-31 at `f41fadf71980`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-12 -->

<!-- horch:task T-200 -->
### T-200 Land the work of task T-13 (unit C10, T-893: stop and stall of a push attempt) with a Check that has no test that is red on the base

- T-893: e2e tests of a stop and a stall of a push attempt; worker_stop ends the held attempt (`09a03f9f`)
- T-893: each stop that fleetd sends leaves 1 stops row (`887aec10`)
- T-893: the stop and stall tests of taskledger.rs run on the push flow and leave the list (`a71944f5`)
- T-893: the whole-flow test stays listed with its review-path cause, owner T-900 (`724523e6`)
- T-893: rustfmt (`4afb0d10`)
- Merge branch 'fleet/fleetd-build' into fleet/fleetd-build-T893-a-stop-and-a-stall-of-a-push-attempt-end (`f7c0eb90`)
- Merge remote-tracking branch 'origin/fleet/fleetd-build-T893-a-stop-and-a-stall-of-a-push-attempt-end' into fleet/fleetd-finish-T13-c10-land-t-893-stop-and-stall-of-a-push (`8f3bf5b8`)
- Comment sweep T-200: delete 33 comments that the diff added, and name the fast gate step of push_fleet (`ba13db71`)

<details><summary>8 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_worker.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`
- `crates/horch-core/src/fleetd/task_execution/adapters/sqlite.rs`
- `crates/horch-core/src/fleetd/task_execution/application/commands.rs`
- `crates/horch-core/src/taskledger/stop.rs`
- `crates/horch-e2e/expected-failures/taskledger.txt`
- `crates/horch-e2e/tests/p5_push_stop.rs`
- `crates/horch-e2e/tests/taskledger.rs`

</details>

Landed by opus-33 at `ba13db711a4f`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-200 -->

<!-- horch:task T-86 -->
### T-86 CI FIX-8: migrate stale tests in horch-e2e done_budget.rs, fakes.rs, fleet18_brief_worktree.rs and fleet18_messaging.rs

- CI FIX-8: migrate the stale tests of done_budget, fakes, fleet18_brief_worktree and fleet18_messaging to the Phase 6 setup (T-86) (`5b23ea14`)
- T-86 comment sweep: drop 23 comments (52 lines) from the migrated e2e tests (`7a996993`)

<details><summary>4 files changed</summary>

- `crates/horch-e2e/tests/done_budget.rs`
- `crates/horch-e2e/tests/fakes.rs`
- `crates/horch-e2e/tests/fleet18_brief_worktree.rs`
- `crates/horch-e2e/tests/fleet18_messaging.rs`

</details>

Landed by qa-engineer-39 at `7a996993f955`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-86 -->

<!-- horch:task T-80 -->
### T-80 CI FIX-8: migrate stale tests in horch-core stop.rs, context_prime.rs, stack_edges.rs and worktree_disk.rs

- T-80 CI FIX-8: the stale horch-core tests seed an idea in flight (FR-12) and serve the session records of the fleet (`ff7bde99`)
- Comment sweep T-205: delete 4 comments that the T-80 diff added, and name the idea reset of worktree_disk World::task (`de351e86`)

<details><summary>4 files changed</summary>

- `crates/horch-core/src/taskledger/stop.rs`
- `crates/horch-core/tests/context_prime.rs`
- `crates/horch-core/tests/stack_edges.rs`
- `crates/horch-core/tests/worktree_disk.rs`

</details>

Landed by qa-engineer-44 at `de351e869959`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-80 -->

<!-- horch:task T-201 -->
### T-201 C11 T-900: complete code-sweep review tasks of a push attempt and make taskledger.txt empty

- T-900: a code-sweep review task of a push attempt ends done by its findings file, and horch result needs no worktree for it (`18b12b38`)
- T-900: an e2e test proves a push review task ends done and its task lands; the whole-flow entry names its new cause, owned by T-207 (`5c5ac1fb`)
- T-201 comment sweep: remove 8 comments that retell the code; rename review_task to end_review_by_findings (`b7eb3160`)

<details><summary>4 files changed</summary>

- `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`
- `crates/horch-e2e/expected-failures/taskledger.txt`
- `crates/horch-e2e/tests/taskledger.rs`
- `crates/horch/src/cmd/resultcmd.rs`

</details>

Landed by opus-34 at `b7eb3160bcff`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-201 -->

<!-- horch:task T-76 -->
### T-76 CI FIX-3: fix the 33 rustdoc errors in 19 files of horch-core

- CI FIX-3: rustdoc -D warnings passes: doc-comment edits only in 29 files (private links to code spans, fn/mod disambiguation, HTML tags in backticks, redundant and unresolved links) (T-76) (`76cbafec`)
- comment sweep T-76: move the set_wait doc from attempt_mode back to set_wait (`ccd7614d`)

<details><summary>29 files changed</summary>

- `crates/horch-core/src/execution/store.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_client.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/adapters/peer_process.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`
- `crates/horch-core/src/fleetd/ideas/application/park_cancel.rs`
- `crates/horch-core/src/fleetd/integration/adapters/exec_push.rs`
- `crates/horch-core/src/fleetd/integration/application/commands.rs`
- `crates/horch-core/src/fleetd/integration/application/judge_branch.rs`
- `crates/horch-core/src/fleetd/integration/application/new_features.rs`
- `crates/horch-core/src/fleetd/task_execution/application/lease.rs`
- `crates/horch-core/src/fleetd/task_execution/application/outbox_ingest.rs`
- `crates/horch-core/src/fleetd/task_execution/application/quota.rs`
- `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`
- `crates/horch-core/src/fleetd/worker_profiles/adapters/launcher.rs`
- `crates/horch-core/src/fleetd/worker_profiles/application/drift_idea.rs`
- `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`
- `crates/horch-core/src/fleetd/worktree_custody/application/create.rs`
- `crates/horch-core/src/harness/codex.rs`
- `crates/horch-core/src/harness/launch.rs`
- `crates/horch-core/src/roster/validation.rs`
- and 9 more files

</details>

Landed by opus-36 at `ccd7614ddc6d`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-76 -->

<!-- horch:task T-48 -->
### T-48 Fix R6-F1: process conformance before dispatch and fail closed on missing or invalid conformance.json (FR-8, FM-19, SR-8)

- fleetd-finish 80: the pass acts on conformance results before dispatch, holds a changed CLI until its conformance run is acted on, and marks the profiles of a CLI unhealthy on a missing or invalid conformance.json (R6-F1, FR-8, FM-19, SR-8) (T-48) (`0aed5afd`)
- comment sweep T-48: delete 12 comments that restate the conformance-before-dispatch code and tests (T-212) (`3015e16f`)

<details><summary>3 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`
- `crates/horch-core/src/fleetd/worker_profiles/application/health.rs`
- `crates/horch-core/tests/fleetd_pass_p7.rs`

</details>

Landed by opus-hardening-9 at `3015e16f30a4`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-48 -->

<!-- horch:task T-78 -->
### T-78 CI FIX-5: fix the 16 Windows test compile errors in horch-core

- CI FIX-5: the horch-core lib test compiles on Windows: unix-only test helpers carry #[cfg(unix)] in land, pr, codex and outbox (T-78) (`afdf7c94`)
- CI FIX-10 (part): each_launch_gets_only_its_own_rules checks horch sessions, since Phase 6 retired the horch spawn rule (T-78) (`78684eea`)

<details><summary>4 files changed</summary>

- `crates/horch-core/src/harness/codex.rs`
- `crates/horch-core/src/taskledger/finish/land.rs`
- `crates/horch-core/src/taskledger/finish/pr.rs`
- `crates/horch-core/src/taskledger/outbox.rs`

</details>

Landed by qa-engineer-46 at `78684eeadc84`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-78 -->

<!-- horch:task T-77 -->
### T-77 CI FIX-4: fix the clippy 1.100 chunks_exact lint in worktree_custody/adapters/git.rs

- CI FIX-4: project_program uses as_chunks::<3> for the clippy 1.100 chunks_exact_to_as_chunks lint (T-77) (`17375eca`)
- CI FIX-4: clear the clippy -D warnings errors of release-lint and all-targets: collapse an if, box Prepared::Pane, drop needless &mut and mut, alias 2 complex types, remove 1 unused test helper (T-77) (`9fd0ead9`)
- Comment sweep T-77: remove the comment on the dead_code allow of support::fleetd (`54ebe0e8`)

<details><summary>28 files changed</summary>

- `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`
- `crates/horch-core/tests/check_sandbox.rs`
- `crates/horch-core/tests/fleetd_handlers_worker.rs`
- `crates/horch-core/tests/fleetd_job_result_file.rs`
- `crates/horch-core/tests/fleetd_jobs.rs`
- `crates/horch-core/tests/fleetd_pass_p3.rs`
- `crates/horch-core/tests/fleetd_pass_p7.rs`
- `crates/horch-core/tests/gate_jobs.rs`
- `crates/horch-core/tests/idea_lead_persona.rs`
- `crates/horch-core/tests/idea_rollup.rs`
- `crates/horch-core/tests/p7_conformance_job.rs`
- `crates/horch-core/tests/p7_docs_sync_job.rs`
- `crates/horch-core/tests/rebase_job.rs`
- `crates/horch-core/tests/stack_edges.rs`
- `crates/horch-core/tests/stacks.rs`
- `crates/horch-core/tests/trial_merges.rs`
- `crates/horch-core/tests/verifier_review.rs`
- `crates/horch-core/tests/worktree_jobs.rs`
- `crates/horch-core/tests/worktree_reconcile_acting.rs`
- `crates/horch-core/tests/worktree_remove.rs`
- and 8 more files

</details>

Landed by opus-38 at `54ebe0e80372`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-77 -->

<!-- horch:task T-193 -->
### T-193 CI FIX-10: fix unexplained unit test failures in horch-core codex.rs, launch.rs and schema.rs

- CI FIX-10: trf_48 builds its version-14 ledger with phase_5_jobs, and the git-env launch child test gets fake bwrap and socat (T-193) (`7c65f1ec`)
- T-193 comment sweep: remove the comment that restates the sandbox_tools_path call in git_env_agent_launch_child (`89bc78c3`)

<details><summary>2 files changed</summary>

- `crates/horch-core/src/harness/launch.rs`
- `crates/horch-core/src/taskledger/schema.rs`

</details>

Landed by qa-engineer-47 at `89bc78c3cd64`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-193 -->

<!-- horch:task T-182 -->
### T-182 The verdict of a failed task check does not show why the sandbox refused (the check stderr is dropped)

- T-182: the verdict of a failed task check keeps the end of the check output and names the sandbox rules (`d4de0b67`)
- comment sweep T-182: delete narration comments in the check verdict code and tests (`768f122d`)
- T-182: the check output file is private to fleetd, and quoted_output escapes hidden characters (`202b5ae8`)

<details><summary>3 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`
- `crates/horch-core/tests/check_sandbox.rs`
- `crates/horch-e2e/tests/p4_verifier.rs`

</details>

Landed by opus-39 at `202b5ae84fbb`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-182 -->

<!-- horch:task T-15 -->
### T-15 C12 T-882: resume under push dispatch and migrate 4 retired horch ledger run new tests to ledger_seed

- T-15: g51 and g53 tests of fleet18_observability seed their run and worker, and serve fleetd on the state ledger (`e410b9d4`)
- T-15: ledger_stop and g52_concurrent run under push dispatch (`3f2914e7`)
- T-15: the spawn tests of fleet18_observability read the attempt spawn of fleetd; 2 tests of retired behaviors are deleted (`6c8a4f45`)
- T-15 comment sweep: delete 13 comments that the diff added to fleet18_observability.rs (`617e8546`)

<details><summary>3 files changed</summary>

- `crates/horch-e2e/expected-failures/fleet18_observability.txt`
- `crates/horch-e2e/src/ledger_seed.rs`
- `crates/horch-e2e/tests/fleet18_observability.rs`

</details>

Landed by opus-35 at `617e854623d3`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-15 -->

<!-- horch:task T-209 -->
### T-209 `horch ledger status --json` failed 1 time with "attempt to write a readonly database": a read command runs a write statement

- T-209: a read command that gets SQLITE_READONLY names its statement and ledger; tests prove the reader commands write nothing (`a48a62a4`)
- T-209 comment sweep: remove 10 comments that retell the code (`001e73c2`)

<details><summary>3 files changed</summary>

- `crates/horch-core/src/taskledger/store.rs`
- `crates/horch/src/cmd/taskledger.rs`
- `crates/horch/tests/status_read_only.rs`

</details>

Landed by opus-37 at `001e73c290b9`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-209 -->

<!-- horch:task T-171 -->
### T-171 A fleetd that a stand-in start launches inherits the env of the calling process, so a worker can arm HORCH_FAULT

- fleetd-finish 156: a fleet process stops and starts no fleetd, so a worker cannot give fleetd its env or arm HORCH_FAULT (T-171) (`c6d46db8`)
- fleetd-finish 156: fm12 waits until the 1st-pass heartbeat of the hung fleetd is past the hang limit, not 32 s after the row (T-171) (`c9a29775`)
- comment sweep T-171: remove 8 comments in p0_fleetd_hang.rs and cmd/fleetd.rs (`2007fe09`)

<details><summary>2 files changed</summary>

- `crates/horch-e2e/tests/p0_fleetd_hang.rs`
- `crates/horch/src/cmd/fleetd.rs`

</details>

Landed by opus-hardening-10 at `2007fe09c16e`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-171 -->

<!-- horch:task T-213 -->
### T-213 The Context warning repeats on each progress line of a worker in a fleetd attempt

- T-213: a worker in a fleetd attempt gets the Context warning 1 time (`2e62640c`)
- T-213 comment sweep: remove 20 comments that retell the code (`78c01c74`)

<details><summary>5 files changed</summary>

- `crates/horch-core/src/fleetd/task_execution/adapters/outbox_reader.rs`
- `crates/horch-core/src/fleetd/task_execution/application/outbox_ingest.rs`
- `crates/horch-core/src/fleetd/task_execution/domain/outbox.rs`
- `crates/horch-e2e/tests/context_warn_once.rs`
- `crates/horch/src/cmd/messaging.rs`

</details>

Landed by opus-41 at `78c01c74f81d`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-213 -->

<!-- horch:task T-195 -->
### T-195 Lost work: an attempt that times out in launching leaves its task running with no lease and no attempt

- fleetd T-195: a start timeout of a task attempt runs rung 6 for its task, and the pass opens an I1 item for a running task with no attempt and no lease (`99905f2e`)
- fleetd T-195: a dead attempt of a leased task runs rung 6 (rung6_if_lost); the e2e test fm21_launch_timeout_gives_the_task_a_retry kills the first launch, so only the start timeout ends it (`cb86e04b`)
- fleetd T-195: rung6_if_lost and lost_tasks read the attempts and leases through leases_store, so fleet_runtime names no table of task_execution (`59080f4f`)
- fleetd T-195: the e2e launch-timeout test fails the first pane run of the task worker through a herdr wrapper, so no wrapper heartbeat can make the attempt live (`284fb53e`)
- fleetd T-195: comment sweep removes 21 comments that the diff added, and puts the doc of verifying back on its function (`f9e16cda`)

<details><summary>3 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`
- `crates/horch-core/tests/fleetd_pass_p2.rs`
- `crates/horch-e2e/tests/p6_questions.rs`

</details>

Landed by opus-40 at `f9e16cda301b`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-195 -->

<!-- horch:task T-236 -->
### T-236 `horch ledger status` writes on its read-only connection: `workers::waiting` ends a wait and sets `due_at`

- T-236: horch ledger status reads waits with no write (`21e73c82`)
- T-236: comment sweep removes 8 comments that the diff added (`8121c66f`)

<details><summary>3 files changed</summary>

- `crates/horch-core/src/taskledger/status.rs`
- `crates/horch-core/src/taskledger/workers.rs`
- `crates/horch/tests/status_read_only.rs`

</details>

Landed by opus-43 at `8121c66f6a28`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-236 -->

<!-- horch:task T-218 -->
### T-218 CI2: third CI run of the run branch after the CI fix units, and the difference to CI1

- CI2: the third CI result of fleetd on 3 OSes (`33f6814d`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/ci-run-3.md`

</details>

Landed by qa-engineer-48 at `33f6814df498`.
<!-- /horch:task T-218 -->

<!-- horch:task T-245 -->
### T-245 CI FIX-15: status words are written as literals in the triage SQL

- CI FIX-15: the triage SQL builds the landed and abandoned words from LandingState (T-245) (`a5884b08`)

<details><summary>1 file changed</summary>

- `crates/horch-core/src/taskledger/triage/mod.rs`

</details>

Landed by qa-engineer-50 at `a5884b089d0d`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-245 -->

<!-- horch:task T-232 -->
### T-232 CI FIX-8: migrate stale tests in horch-e2e taskledger.rs part 2 (lines 1979 to 2869)

- T-90 CI FIX-8: the stale tests of taskledger.rs part 1 seed their runs with ledger_seed and run the finish flow in push form (`77446371`)
- T-232 CI FIX-8: the shared-task e2e test runs horch result in a shell that is the agent of the attempt, so the test can start fleetd (T-171: a fleet process starts no fleetd) (`90102c02`)
- T-232 CI FIX-8: the stale tests of taskledger.rs part 2 run in push form: a shell agent ends with horch ledger finish and fleetd verifies (`37aa09a1`)
- T-232 CI FIX-8: wait_pipeline waits 480 s, since a test with a fix attempt and a 2nd review round takes 110 s alone and timed out at load average 27 (`57a072a7`)
- T-232 comment sweep: remove 143 narrating comment lines from taskledger.rs (`30ed67b9`)

<details><summary>2 files changed</summary>

- `crates/horch-e2e/expected-failures/taskledger.txt`
- `crates/horch-e2e/tests/taskledger.rs`

</details>

Landed by qa-engineer-49 at `30ed67b9962d`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-232 -->

<!-- horch:task T-244 -->
### T-244 CI FIX-17: release-lint fails on 2 unused ExitCode values in the horch bin

- CI FIX-17: release-lint drops 2 ExitCode values on purpose (T-244) (`91687e77`)

<details><summary>2 files changed</summary>

- `crates/horch/src/cmd/compact.rs`
- `crates/horch/src/cmd/messaging.rs`

</details>

Landed by opus-48 at `91687e772910`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-244 -->

<!-- horch:task T-248 -->
### T-248 CI FIX-20: 2 tests of p5_route_ledger_task run as a fleet process and are refused

- T-248: the 2 HORCH_ROLE tests of p5_route_ledger_task assert the refusal of the CLI guard and of fleetd for a raw request (`24b36769`)
- T-257: comment sweep removes 2 doc comments that the T-248 diff added (`932e8dc6`)

<details><summary>1 file changed</summary>

- `crates/horch/tests/p5_route_ledger_task.rs`

</details>

Landed by qa-engineer-52 at `932e8dc6f30b`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-248 -->

<!-- horch:task T-247 -->
### T-247 CI FIX-19: 9 messaging e2e tests wait for a codex worker that horch does not trust

- CI FIX-19: the messaging e2e tests wait for the fake codex call that logs a session id, not for session_id in the ledger record; the worker wrapper keeps its session records in its process since 8db249b8 (T-899); the codex trust note was not the cause (T-247) (`0e29639d`)
- comment sweep: remove the running_worker comment that retells the worker wrapper behavior (T-258) (`d8e07bcc`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/tests/messaging.rs`

</details>

Landed by qa-engineer-51 at `d8e07bcc2e7b`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-247 -->

<!-- horch:task T-242 -->
### T-242 CI FIX-11: 42 horch-core tests build the fakes with a cargo profile taken from a folder name

- CI FIX-11: the horch-core tests take the profile of the fakes from the target layout, not from the last folder name (T-242) (`ba430aa8`)
- T-259: comment sweep removes 9 comments that the T-242 diff added (`add9ff3b`)

<details><summary>5 files changed</summary>

- `crates/horch-core/tests/p7_conformance.rs`
- `crates/horch-core/tests/p7_context_limit.rs`
- `crates/horch-core/tests/p7_operator_boundary.rs`
- `crates/horch-core/tests/p7_wrapper_exec.rs`
- `crates/horch-core/tests/support/fakes.rs`

</details>

Landed by opus-46 at `add9ff3b5730`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-242 -->

<!-- horch:task T-261 -->
### T-261 Land the work of task T-243 (CI FIX-14: the test targets compile for Windows) with a Check that works on this host

- CI FIX-14: the test targets compile for Windows (T-243) (`e1c5ce74`)
- CI FIX-14: the 2 TRF-39 register tests of messaging.rs are Unix only (T-243) (`616bd41e`)
- Comment sweep T-261: remove 5 comments that restate the cfg(unix) gates (T-263) (`b64876cc`)

<details><summary>19 files changed</summary>

- `crates/horch-core/tests/common/warm.rs`
- `crates/horch-core/tests/godot_wave.rs`
- `crates/horch-core/tests/i308_triage_apply.rs`
- `crates/horch-core/tests/idea_lead_persona.rs`
- `crates/horch-core/tests/intake_roster.rs`
- `crates/horch-core/tests/p5_claude_sandbox.rs`
- `crates/horch-core/tests/p7_claude_exec.rs`
- `crates/horch-core/tests/r_workspace_arrange.rs`
- `crates/horch-core/tests/status_parked_disk.rs`
- `crates/horch-core/tests/triage_apply_job.rs`
- `crates/horch-core/tests/w3_finish_steps.rs`
- `crates/horch-core/tests/worktree_create.rs`
- `crates/horch-core/tests/worktree_jobs.rs`
- `crates/horch-e2e/src/process.rs`
- `crates/horch-e2e/tests/fleet18_messaging.rs`
- `crates/horch-platform/tests/sandbox.rs`
- `crates/horch/src/cmd/messaging.rs`
- `crates/horch/src/cmd/recipes.rs`
- `crates/horch/src/cmd/taskledger_watch.rs`

</details>

Landed by opus-50 at `b64876cc3d8e`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-261 -->

<!-- horch:task T-241 -->
### T-241 CI FIX-12: unit tests of horch-core launch with no sandbox PATH on Linux

- CI FIX-12: horch-core unit tests launch with fake bwrap and socat on PATH (T-241) (`32e6b042`)
- CI FIX-12: 3 launch tests with their own PATH get fake bwrap and socat (T-241) (`9126bcca`)
- CI FIX-12: hdf_05 makes its sandbox fakes with a platform-neutral call (T-241) (`c66abcb3`)
- Comment sweep T-241: remove 9 comments that the diff added (T-264) (`f5ca170f`)

<details><summary>7 files changed</summary>

- `crates/horch-core/src/harness/launch.rs`
- `crates/horch-core/tests/baseline_oracles.rs`
- `crates/horch-core/tests/context_launch.rs`
- `crates/horch-core/tests/execution_plan.rs`
- `crates/horch-core/tests/harness_defaults.rs`
- `crates/horch-core/tests/p7_conformance_job.rs`
- `crates/horch-core/tests/p7_context_256k.rs`

</details>

Landed by opus-45 at `f5ca170f1dcb`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-241 -->

<!-- horch:task T-233 -->
### T-233 CI FIX-8: migrate stale tests in horch-e2e taskledger.rs part 3 (lines 2870 to 3362)

- T-233 CI FIX-8: the stale tests of taskledger.rs part 3 seed their runs with ledger_seed (`3e9ce80c`)
- T-233 comment sweep: remove 7 narration and history comments from taskledger.rs (`de48d4d2`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/tests/taskledger.rs`

</details>

Landed by qa-engineer-54 at `de48d4d201f0`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-233 -->

<!-- horch:task T-240 -->
### T-240 CI FIX-13: the e2e harness gives fleetd a PATH with no bwrap and socat on Linux

- CI FIX-13: the e2e harness puts links to the real bwrap and socat on the sealed PATH on Linux (T-240) (`2ab5bc1c`)
- Comment sweep T-240: delete 3 comments in the e2e harness; name the fleetd sandbox tool list (T-266) (`1c36812f`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/src/harness.rs`

</details>

Landed by opus-44 at `1c36812f8961`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-240 -->

<!-- horch:task T-252 -->
### T-252 CI FIX-23: e2e tests of p0_liveness, skills_exposure, startup_error and i308 triage are red with no owner

- CI FIX-23: the i308 helper waits for the abandoned tree after the cancel; fm5 declares the fast gate and the run workspace, so the pass dispatches the fallback (T-252) (`1a692fae`)
- T-252: comment sweep removes 4 comments that the diff added (`016a6f3f`)

<details><summary>2 files changed</summary>

- `crates/horch-e2e/tests/i308_fleet_steward.rs`
- `crates/horch-e2e/tests/p0_liveness.rs`

</details>

Landed by qa-engineer-53 at `016a6f3f69f9`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-252 -->

<!-- horch:task T-246 -->
### T-246 CI FIX-18: e2e tests time out because the profile antigravity is unhealthy at boot

- CI FIX-18: sr3 uses the off-host address 192.0.2.1, not the LAN address of the host (T-246) (`2ca577e1`)
- CI FIX-18: the live fleetd writes the judge ticket of a --direct idea (T-246) (`a5bbc8df`)
- CI FIX-18: on Linux the e2e harness root is under /var/tmp, not /tmp (T-246) (`8bfd800c`)

<details><summary>3 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`
- `crates/horch-e2e/src/harness.rs`
- `crates/horch-e2e/tests/p4_verifier.rs`

</details>

Landed by opus-51 at `8bfd800cf709`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-246 -->

<!-- horch:task T-269 -->
### T-269 watchdog_liveness: after a SIGTERM to fleetd during a reap, the fleetd row does not name the new fleetd in 120 s

- T-269: the watchdog_liveness fixture records the fake agent as supervisor, not PID 1; PID 1 matched session 1 of a terminal that launchd starts, so fleetd refused the test (T-171 rule) (`7e59906f`)
- T-272: comment sweep of T-269 removes 1 comment that the diff added (`4b337865`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/tests/watchdog_liveness.rs`

</details>

Landed by opus-53 at `4b337865a1e4`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-269 -->

<!-- horch:task T-268 -->
### T-268 p1_orphans: the operator-boundary probe of the fake agent leaves a request file of mode 0644 that a fleetd pass can read (2 tests fail by a race)

- p1_orphans: fleetd in the FM-22 tests finds no agent CLI, so no conformance probe writes a request file (T-268) (`f1b0b81e`)
- p1_orphans: a named helper replaces the comment on the agent CLI removal (T-268 comment sweep) (`38fa0616`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/tests/p1_orphans.rs`

</details>

Landed by opus-52 at `38fa06164f8c`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-268 -->

<!-- horch:task T-267 -->
### T-267 Linux: fleetd finds no bwrap for the check sandbox, so each persona that is not codex is held (`no_check_sandbox`), also in production

- fleetd: find the Linux check sandbox on the PATH of the fleetd process (T-267) (`4ae0e6c8`)
- Comment sweep of T-267: delete 6 comments that retell the code (T-277) (`9629bdd7`)

<details><summary>3 files changed</summary>

- `crates/horch-core/src/fleetd/worker_profiles/adapters/check_sandbox.rs`
- `crates/horch-core/tests/check_sandbox.rs`
- `crates/horch/src/cmd/taskledger_watch.rs`

</details>

Landed by opus-hardening-12 at `9629bdd7dd14`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-267 -->

<!-- horch:task T-275 -->
### T-275 FM-22: the orphans stop 17 s after the start of fleetd in 2 of 63 runs under load (limit 10 s): fleetd waits 12 s in `close_pane`

- FM-22: the orphans test proves the stop by the count of fleetd passes, not by wall time (T-275) (`6b4c095f`)
- FM-22: remove the comments that retell the orphans test code (T-275) (`a437011f`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/tests/p1_orphans.rs`

</details>

Landed by opus-55 at `a437011fa3f0`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-275 -->

<!-- horch:task T-281 -->
### T-281 The pass of fleetd calls herdr with no time limit: a herdr that does not answer holds the pass

- fleetd-finish 197: each herdr call of the fleetd pass has a 5 s time limit (T-281) (`ee782e94`)
- fleetd-finish 197: remove the comments that retell the herdr time limit code and its test (T-281) (`76409277`)

<details><summary>2 files changed</summary>

- `crates/horch/src/cmd/fleetd.rs`
- `crates/horch/tests/fleetd_herdr_timeout.rs`

</details>

Landed by opus-56 at `76409277c6d4`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-281 -->

<!-- horch:task T-274 -->
### T-274 Land the work of task T-89 (CI FIX-8: the stale tests of p2_leases, p2_release, p3_refusals, p3_release, p3_unlanded, p3_worktrees) with a check limit of 3600 s

- CI FIX-8: p2_leases uses ledger_seed, names its run, waits for the cancel and the rescue pass (T-89) (`4d928989`)
- CI FIX-8: p2_release migrates with the store, makes the test process the wrapper of the attempt and names its run (T-89) (`1bff5864`)
- CI FIX-8: p3_release migrates with the store, makes the test process the wrapper of the attempt and names its run (T-89) (`4bfcc701`)
- CI FIX-8 WIP: p3_refusals seeds with ledger_seed and passes; gate-tree test is the new form and an expected failure. (`74852b48`)
- CI FIX-8 WIP: claimed commits the gates of the project (T-89) (`c75ea1bb`)
- CI FIX-8 WIP: cleanup and dismiss tests use the rescued-tree form; owner of the gate test is T-199 (T-89) (`9d3e17e6`)
- CI FIX-8 WIP: drop tests use the custody form; scratch drop is split (T-89) (`bdffd94b`)
- CI FIX-8: p3_unlanded seeds with ledger_seed, parks the task as a fixture (T-206 gap), passes through fleetd passes; rr15_landed is an expected failure (T-207); RR-13 head-moved test is an expected failure (T-204) with a green test that no commit is lost (`3728697b`)
- CI FIX-8: p3_worktrees seeds with ledger_seed, binds through fleetd passes, runs 1 test at a time; fm27 split (T-207), fm28 shows the hold by a task with no attempt (`db4bd822`)
- CI FIX-8: evidence of the product faults that the migrated e2e tests found (T-89) (`0c3986e1`)
- Comment sweep T-89: delete narration comments that the migrated e2e tests added (`26c3600d`)
- CI FIX-8: rr6 waits until no tree of the failed attempt is reserved, created, active or orphaned (`d1ae777c`)
- CI FIX-8: p2_release and p3_release run the worker commands under a wrapper process of its own, since a fleet process starts no fleetd (T-171) (`eb0984b8`)
- Comment sweep T-89 (round 2): delete 6 narration comments in the CI FIX-8 diff (`6e046b08`)
- CI FIX-8: fm28 keeps the lease of its live attempt, and the pass helper of p3_worktrees waits 300 s for jobs (T-89) (`c4616744`)
- CI FIX-8: p3_worktrees queues and binds its tasks while the test fleetd is down (T-89) (`2ba8cb46`)
- Restore the security comments of T-182 that the comment sweep T-89 removed (`9642a990`)
- CI FIX-8: fm27_trees_that_are_not_landed binds a live agent with a long lease, since the watchdog ended the attempt of a dead stand-in agent before the tree was active (T-89) (`5f05b050`)
- Comment sweep T-274: delete 6 comments that the diff adds in the e2e tests (T-280) (`c0128e1d`)

<details><summary>11 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`
- `crates/horch-core/tests/check_sandbox.rs`
- `crates/horch-e2e/expected-failures/p3_refusals.txt`
- `crates/horch-e2e/expected-failures/p3_unlanded.txt`
- `crates/horch-e2e/expected-failures/p3_worktrees.txt`
- `crates/horch-e2e/tests/p2_leases.rs`
- `crates/horch-e2e/tests/p2_release.rs`
- `crates/horch-e2e/tests/p3_refusals.rs`
- `crates/horch-e2e/tests/p3_release.rs`
- `crates/horch-e2e/tests/p3_unlanded.rs`
- `crates/horch-e2e/tests/p3_worktrees.rs`

</details>

Landed by qa-engineer-57 at `c0128e1d4aa0`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-274 -->

<!-- horch:task T-249 -->
### T-249 CI FIX-21: 5 red core tests: foreign table read, codex rules snapshot, watch pin, launching state

- CI FIX-21: the ideas and task_execution contexts read events and session_records through the readers of their owners (T-249) (`f342851c`)
- CI FIX-21: fleetd finds the check sandbox on RuntimeContext.inherited.path, so core reads no PATH (T-249) (`791f6e07`)
- CI FIX-21: the codex worker rules snapshot holds the 4 gh rules of SR-3 (T-249) (`a8af201b`)
- CI FIX-21: the 2 launch tests of fleetd_pass put the launched process of the no-task attempt in its pane (T-249) (`0f89d7cb`)
- CI FIX-21: watch_once_reports_in_text_and_json pins the dead verdict of an unproven attempt (T-249) (`f0a907f2`)
- Comment sweep T-249: remove 5 test comments that retell the code (T-284) (`0f232b12`)
- CI FIX-21: the watch test world checks every 3600 s, so no check is due under load (T-249) (`b674a390`)

<details><summary>16 files changed</summary>

- `crates/horch-core/src/execution/store.rs`
- `crates/horch-core/src/fleetd/ideas/application/answers.rs`
- `crates/horch-core/src/fleetd/task_execution/application/outbox_ingest.rs`
- `crates/horch-core/src/fleetd/worker_profiles/adapters/check_sandbox.rs`
- `crates/horch-core/tests/check_sandbox.rs`
- `crates/horch-core/tests/fixtures/harness/codex_rules_worker.rules`
- `crates/horch-core/tests/fleetd_pass.rs`
- `crates/horch-core/tests/fleetd_pass_p4a.rs`
- `crates/horch-core/tests/fleetd_pass_p4b.rs`
- `crates/horch-core/tests/fleetd_pass_p6b.rs`
- `crates/horch-core/tests/fleetd_pass_p7.rs`
- `crates/horch-core/tests/p5_profile_validation.rs`
- `crates/horch-core/tests/sr3_no_agent_push.rs`
- `crates/horch/src/cmd/fleetd.rs`
- `crates/horch/src/cmd/jobcmd.rs`
- `crates/horch/tests/w3_ledger_cli_pins.rs`

</details>

Landed by opus-57 at `b674a390772b`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-249 -->

<!-- horch:task T-234 -->
### T-234 CI FIX-8: migrate stale tests in horch-e2e taskledger.rs part 4 (lines 3363 to end)

- T-234 CI FIX-8: the last 11 stale tests of taskledger.rs seed their runs with ledger_seed and use the push form (`85e9f2f7`)
- Comment sweep T-234: delete 30 comments that T-234 added to taskledger.rs (T-285) (`82e80099`)
- T-234 CI FIX-8: claim_and_fail waits for the rescue of the orphaned tree before its bind, since the 2nd bind of a_task_whose_attempts_all_fail_is_failed_and_its_replan_requeues_it panicked 'its rescue comes first' under load (`72978e8e`)

<details><summary>2 files changed</summary>

- `crates/horch-e2e/expected-failures/taskledger.txt`
- `crates/horch-e2e/tests/taskledger.rs`

</details>

Landed by qa-engineer-55 at `72978e8e0436`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-234 -->

<!-- horch:task T-291 -->
### T-291 Fix R4-F4: read base branch tips through the sandboxed git gateway before opening the pass write transaction (SR-4, RR-5)

- fleetd-finish 73: the fleet pass reads the base tips of its binds through the sandboxed git gateway before the write transaction, so no bind runs git in it (R4-F4, SR-4, RR-5) (T-291) (`898c28d4`)
- fleetd-finish 73: the comment sweep removes 7 comments that retell the code of the base tips (T-291) (`c807b3a1`)

<details><summary>3 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`
- `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`
- `crates/horch-core/tests/push_dispatch.rs`

</details>

Landed by opus-hardening-14 at `c807b3a13a9e`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-291 -->

<!-- horch:task T-276 -->
### T-276 A job host `horch job run N` is alive 25 s after SIGTERM in e2e teardown under load (a leaked process)

- fleetd-finish 195: a SIGTERM kills every group that kill_group_on_termination registered; e2e teardown waits for a process in its exit, and still fails on a process that runs after the grace (T-276) (`1064a2c6`)
- T-276 comment sweep: remove comments that retell the code (`08c6d4ff`)

<details><summary>4 files changed</summary>

- `crates/horch-core/tests/job_host_stop.rs`
- `crates/horch-e2e/src/harness.rs`
- `crates/horch-platform/src/signal.rs`
- `crates/horch-platform/src/unix/signal.rs`

</details>

Landed by opus-54 at `08c6d4ffd934`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-276 -->

<!-- horch:task T-288 -->
### T-288 RR-14: `horch ledger done --unlanded` in a fleetd attempt does not park the task

- fleetd-finish 167: done --unlanded in a fleetd attempt parks the task (RR-14) (T-288) (`88de7014`)
- fleetd-finish 167: the park fixture of p3_unlanded runs horch ledger done --unlanded; drop unlanded: None from the TaskFacts literals that ceee8d7b hit by mistake (T-288) (`4e4d4df7`)
- fleetd-finish 167: the comment sweep removes 8 comments that retell the code of T-288 (T-296) (`1bd038bd`)

<details><summary>24 files changed</summary>

- `crates/horch-core/src/fleetd/task_execution/application/ingest.rs`
- `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`
- `crates/horch-core/src/fleetd/task_execution/domain/outbox.rs`
- `crates/horch-core/src/fleetd/task_execution/domain/result.rs`
- `crates/horch-core/src/fleetd/task_execution/domain/task_files.rs`
- `crates/horch-core/src/messaging/report.rs`
- `crates/horch-core/tests/fleetd_pass_p4a.rs`
- `crates/horch-core/tests/fleetd_pass_p5.rs`
- `crates/horch-core/tests/fleetd_pass_p7.rs`
- `crates/horch-core/tests/outbox_ingest.rs`
- `crates/horch-core/tests/outbox_writer.rs`
- `crates/horch-core/tests/result_ingest.rs`
- `crates/horch-core/tests/task_files.rs`
- `crates/horch-core/tests/task_files_p6.rs`
- `crates/horch-e2e/tests/fakes.rs`
- `crates/horch-e2e/tests/p1_pass.rs`
- `crates/horch-e2e/tests/p3_unlanded.rs`
- `crates/horch-e2e/tests/p5_untrusted_files.rs`
- `crates/horch-e2e/tests/p7_launcher.rs`
- `crates/horch/src/cmd/messaging.rs`
- and 4 more files

</details>

Landed by opus-60 at `1bd038bd7f9c`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-288 -->

<!-- horch:task T-297 -->
### T-297 R8a: early end review of the change of the run fleetd-finish (the tip of 2026-11-21)

- R8: the end review of the change of this run: `git diff fleet/fleetd-build...HEAD` in your worktree (the isolation fix, T-898, T-899, the 8 other landed task branches, T-900, T-882, the gap fixes) (`50fe4705`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/reviews/end-review-a.md`

</details>

Landed by codex-sol-16 at `50fe47051cfb`.
<!-- /horch:task T-297 -->

<!-- horch:task T-294 -->
### T-294 A cancel of a running task never completes: the task stays `running` with no lease and no live attempt (I1)

- fleetd-finish 192: a cancel of a running task completes; the pass opens no I1 item for a task whose cancel waits (T-294) (`b0a932ec`)
- comment sweep T-294: remove 18 narration comments from the cancel fix and its tests (`6710d16a`)

<details><summary>4 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`
- `crates/horch-core/tests/cancel_completes.rs`
- `crates/horch-e2e/expected-failures/taskledger.txt`
- `crates/horch-e2e/tests/taskledger.rs`

</details>

Landed by opus-62 at `6710d16a28d7`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-294 -->

<!-- horch:task T-303 -->
### T-303 R8-F4 and R8-F5 (Minor): 2 comment sweeps removed the reason of code that needs it

- Restore the reasons of has_session_records_table and the dead_code allow of support::fleetd (T-303) (`c4594687`)

<details><summary>2 files changed</summary>

- `crates/horch-core/src/execution/store.rs`
- `crates/horch/tests/support/mod.rs`

</details>

Landed by qa-engineer-58 at `c4594687bbf7`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-303 -->

<!-- horch:task T-172 -->
### T-172 SR-8: no agent code runs before fleetd has recorded the process of its attempt

- fleetd-finish 157: failing tests: no agent code runs before fleetd records its wrapper (SR-8, FM-32, T-172) (`455f0881`)
- fleetd-finish 157: no agent code runs before fleetd has recorded the process of its attempt (SR-8, FM-32, T-172) (`ae08ee09`)
- fleetd-finish 157: the push attempt kill test waits for the agent PID, which a later heartbeat records, since the wrapper is recorded before its agent starts (SR-8, T-172) (`4ebdb75e`)
- fleetd-finish 157: failing tests: a git writer that waits behind a running writer of its repository starts before newer writers (T-172) (`343d912f`)
- fleetd-finish 157: the git writers of 1 repository start first in, first out, so a waiting rescue is not starved by newer fetches (T-172) (`7c32d9d6`)
- fleetd-finish 157: fr10_e waits on the fleet clock until the start timeout of the held attempt has passed, since the fleet clock lags the wall clock when passes are slow (SR-8, T-172) (`b8f29798`)
- Comment sweep of T-172: remove 29 comments that restate code, private and test docs, and 1 banner (T-298) (`43408dc8`)
- fleetd-finish 157: restore 14 comments of the comment sweep that give a security rule, an invariant or a design id (SR-8, FM-32, T-172) (`44923267`)

<details><summary>16 files changed</summary>

- `crates/horch-core/src/execution/lifecycle.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/adapters/peer_process.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/domain/authorize.rs`
- `crates/horch-core/src/fleetd/task_execution/adapters/attempt_files.rs`
- `crates/horch-core/src/taskledger/supervisor.rs`
- `crates/horch-core/tests/fleetd_pass_p4a.rs`
- `crates/horch-core/tests/fleetd_pass_p5.rs`
- `crates/horch-core/tests/p6_authorize.rs`
- `crates/horch-core/tests/peer_process.rs`
- `crates/horch-core/tests/wrapper_ingest.rs`
- `crates/horch-e2e/src/fr10_agent.rs`
- `crates/horch-e2e/tests/p5_push_attempt_process.rs`
- `crates/horch-e2e/tests/p6_fr10_approval.rs`
- `crates/horch/src/cmd/idea/mod.rs`

</details>

Landed by opus-hardening-11 at `449232676a77`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-172 -->

<!-- horch:task T-301 -->
### T-301 R8-F2 (Important): SR-3/SR-4: on macOS a task check can connect to the own LAN address of the host

- R8-F2: record that no Seatbelt form limits a macOS check to loopback (T-301) (`5c60b42d`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/macos-check-sandbox-lan.md`

</details>

Landed by opus-hardening-15 at `5c60b42d1ce2`.
<!-- /horch:task T-301 -->

<!-- horch:task T-287 -->
### T-287 RR-13: a drop of a worktree whose HEAD moved during the checks is accepted (the refusal is lost)

- RR-13: fleetd refuses a drop whose tree HEAD moved during the checks (T-287) (`1f919fea`)
- p3_refusals: claimed() seeds and binds its task while the test fleetd is down (T-287) (`10f010a6`)
- fleetd: a job host of a pass runs the sibling horch, not this binary (T-287) (`38736b49`)
- Comment sweep of T-287: delete 5 comments that retell the code (T-305) (`4d2c7ff3`)

<details><summary>6 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs`
- `crates/horch-core/tests/fleetd_handlers_custody.rs`
- `crates/horch-e2e/expected-failures/p3_refusals.txt`
- `crates/horch-e2e/tests/p3_refusals.rs`
- `crates/horch/src/cmd/fleetd.rs`
- `crates/horch/tests/p5_route_ledger_task.rs`

</details>

Landed by opus-hardening-13 at `4d2c7ff3cdb0`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-287 -->

<!-- horch:task T-302 -->
### T-302 R8-F3 (Important): SR-8: fleetd still accepts the dead request kind `spawn_adopt`

- fleetd-finish R8-F3: retire the request kind spawn_adopt (T-302) (`fb15c55f`)
- T-302 comment sweep: remove 2 comments that retell code or park a retired statement (`2c6ef0b4`)

<details><summary>4 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_worker.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs`
- `crates/horch-core/tests/fleetd_handlers_worker.rs`
- `crates/horch/tests/p6_retired_worker_commands.rs`

</details>

Landed by opus-hardening-16 at `2c6ef0b41ace`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-302 -->

<!-- horch:task T-254 -->
### T-254 CI FIX-25: e2e tests of p0_landing, p6_fr10_approval, p6_sub_orchestrators and the lifecycle matrix are red with no owner

- T-254 CI FIX-25: a worker_launch_failed report ends the push attempt of that worker crashed at once, so rung 6 queues the task and the bundle test of p0_landing does not wait for the start timeout (`d8a08e35`)
- comment sweep T-254: remove 7 comments from the launch-failed fix and its tests (`ec95e902`)
- T-254: restore the comment that says why the bundle test of p0_landing sets HORCH_FAULT=drop-watch-events; the orchestrator asked for it, and the comment sweep removed it (`529edb1d`)
- T-254: after the rebase on T-302, register reads deps again for worker_launch_failed, and fleetd_handlers_worker.rs gets back the bind helper that T-302 removed with the spawn_adopt test; the T-254 test uses it (`86d91196`)
- comment sweep T-254: remove 4 comments (private fn doc, 2 test docs, test const doc) (`0069a171`)

<details><summary>7 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_worker.rs`
- `crates/horch-core/src/fleetd/task_execution/application/launch.rs`
- `crates/horch-core/tests/fleetd_handlers_worker.rs`
- `crates/horch-e2e/tests/p0_landing.rs`
- `crates/horch/src/cmd/fleetd.rs`
- `crates/horch/tests/p6_retired_worker_commands.rs`

</details>

Landed by opus-58 at `0069a17133f7`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-254 -->

<!-- horch:task T-312 -->
### T-312 E1-prep: trial merge of origin/main into the run branch, and the plan for each conflict

- E1-prep: the plan to merge origin/main e5170752 into fleet/fleetd-finish, with a rule for each of the 26 conflicts (T-312) (`51f007f6`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/merge-main-plan.md`

</details>

Landed by opus-65 at `51f007f60880`.
<!-- /horch:task T-312 -->

<!-- horch:task T-278 -->
### T-278 p7_exec: 2 tests fail under load: `horch result: no live attempt of this process`, and no Codex exec run in 120 s

- p7_exec: the 2-attempt tests set worktree_list_period_s = 5, so the rescue of the tree of A1 does not use 60 s of the 120 s wait for A2 (T-278) (`b61faac2`)
- comment sweep T-278: remove the comment on FLEET_TOML in p7_exec.rs; commit b0d07560 records the reason for worktree_list_period_s = 5 (`99ee10c5`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/tests/p7_exec.rs`

</details>

Landed by opus-64 at `99ee10c55df7`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-278 -->

<!-- horch:task T-292 -->
### T-292 p1_pass: the heartbeat gap of a scan of 10000 folders is over 30 s

- fleetd-finish 42 stage 1: sync_native reads import_findings 1 time per statement (T-292) (`6d9e4a1d`)
- fleetd-finish 42 stage 1: sqlite::rows reads the tasks of the live attempts in 1 statement (T-292) (`ddceb62c`)
- fleetd-finish 42 stage 1: lease::check_i1_within repairs at most a budget of leases (T-292) (`650da1b5`)
- fleetd-finish 42 stage 2: the pass pages the I1 lease repairs and reads the personas of the workers in 1 statement (T-292) (`6c453b24`)
- fleetd-finish 42 stage 2: the disk budget reads the trees in 1 statement (T-292) (`d15dd771`)
- fleetd-finish 42 stage 2: dispatch::choices reads the running tasks and their Own files in 2 statements (T-292) (`7d8096f9`)
- fleetd-finish 42 stage 2: the sub-orchestrator need reads a failed task of an idea in 1 statement (T-292) (`cc0d7e23`)
- fleetd-finish 42: the scan test of 10000 folders keeps the heartbeat under 30 s; remove its expected failure (T-292) (`8b81e16e`)
- fleetd-finish 42 stage 2: reconcile reads the trees with an active removal in 1 statement (T-292) (`74c79ed6`)
- Comment sweep T-292: delete 36 narrating doc comments, name the start-of-pass persona read, put worker_personas below quota_hinted (T-316) (`3f6321ff`)
- fleetd-finish 42: restore the comments of the bounds and of the whole steps that the sweep removed (T-292) (`b7ccafca`)

<details><summary>10 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`
- `crates/horch-core/src/fleetd/ideas/application/sub_orchestrators.rs`
- `crates/horch-core/src/fleetd/task_execution/adapters/sqlite.rs`
- `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`
- `crates/horch-core/src/fleetd/task_execution/application/lease.rs`
- `crates/horch-core/src/fleetd/worktree_custody/application/disk.rs`
- `crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs`
- `crates/horch-core/src/taskledger/import.rs`
- `crates/horch-core/src/taskledger/testing.rs`
- `crates/horch-e2e/expected-failures/p1_pass.txt`

</details>

Landed by opus-61 at `b7ccafcae3ec`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-292 -->

<!-- horch:task T-300 -->
### T-300 R8-F1 (Important): RR-12: an answered question does not resume the session of the worker

- fleetd-finish 199: an answered question resumes the session of the worker (RR-12, R8-F1, T-300) (`583b4402`)
- fleetd-finish 199: fm21_launch_timeout makes a first launch with no report, since T-254 ends a reported launch failure crashed at once (T-300) (`0b860e79`)
- comment sweep T-300: delete 20 comments that retell code or test names (T-317) (`90a1fa49`)
- fleetd-finish 199: restore 7 comments of the comment sweep that give a security rule, a design id or a reason (RR-12, T-254, T-300) (`9da64804`)

<details><summary>8 files changed</summary>

- `crates/horch-core/src/execution/model.rs`
- `crates/horch-core/src/execution/plan.rs`
- `crates/horch-core/src/fleetd/task_execution/application/launch.rs`
- `crates/horch-e2e/tests/p6_questions.rs`
- `crates/horch-e2e/tests/p6_worker_question.rs`
- `crates/horch/src/cmd/fleetd.rs`
- `crates/horch/src/cmd/spawn.rs`
- `crates/horch/src/cmd/taskledger_spawn.rs`

</details>

Landed by opus-63 at `9da64804637f`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-300 -->

<!-- horch:task T-314 -->
### T-314 E1-prep-2: prove the trial merge of main: run the suites that the plan marks "not verified", and fix the trial

- E1-prep-2: the proof of the trial merge of main: each suite, 9 fixes on keep/fleetd-finish-trial-merge-main 1e782060, the schema lines for the operator (T-314) (`5d1bf996`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/merge-main-proof.md`

</details>

Landed by opus-66 at `5d1bf9962b2b`.
<!-- /horch:task T-314 -->

<!-- horch:task T-307 -->
### T-307 A launch whose host call fails waits for the start timeout before the retry

- T-307: a launch whose host call fails ends its push attempt crashed in the same pass, as a reported failed spawn does (T-254) (`72f78a71`)
- comment sweep T-307: delete 4 comments that retell the test code (T-318) (`e845d9b4`)

<details><summary>2 files changed</summary>

- `crates/horch-core/src/fleetd/task_execution/application/launch.rs`
- `crates/horch-core/tests/push_dispatch.rs`

</details>

Landed by opus-67 at `e845d9b47cc9`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-307 -->

<!-- horch:task T-309 -->
### T-309 CI FIX-22: 5 tests red on Linux only: loopback in the sandbox, reply timeout, gate group kill, NULL name, operator boundary

- CI FIX-22: Linux job sandboxes get a PID namespace that a group KILL ends; 2 Linux test faults fixed (T-309) (`b29ac09b`)
- T-309 comment sweep: remove 11 comments that retell code or tests (`c9476163`)
- T-309: restore 3 comments of the sweep that give a security reason or an invariant, and name the FR-10 reason of host PIDs (`d70a4032`)

<details><summary>8 files changed</summary>

- `crates/horch-core/src/fleetd/integration/adapters/exec_gate.rs`
- `crates/horch-core/src/harness/claude.rs`
- `crates/horch-core/tests/gate_jobs.rs`
- `crates/horch-platform/src/linux/sandbox.rs`
- `crates/horch-platform/src/mac/sandbox.rs`
- `crates/horch-platform/src/sandbox.rs`
- `crates/horch-platform/src/windows/sandbox.rs`
- `crates/horch-platform/tests/channel.rs`

</details>

Landed by opus-hardening-18 at `d70a4032a47a`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-309 -->

<!-- horch:task T-313 -->
### T-313 A large ready queue: the dispatch step runs SQL for each ready task above the pass budget

- fleetd-finish 206: dispatch::choices and read_base_tips read the ready tasks in a fixed number of statements (T-313) (`0b81e22e`)
- fleetd-finish 206: comment sweep of the T-313 diff removes 16 comments (T-313) (`571d5000`)
- fleetd-finish 206: restore 2 comments of the sweep on the ready rows reader (T-313) (`895e3848`)

<details><summary>2 files changed</summary>

- `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`
- `crates/horch-core/tests/push_dispatch.rs`

</details>

Landed by opus-68 at `895e38485023`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-313 -->

<!-- horch:task T-324 -->
### T-324 Pre-gate of Phase 5: run the Phase 5 gate rows now, on the tip of 2026-11-21, and report each red row

- fleetd-finish 211: the pre-gate of Phase 5 on tip 895e3848 - 1 red row, arch_scan fitness_no_context_touches_foreign_tables (owner T-327) (T-324) (`ba493a2c`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/pre-gate-p5.md`

</details>

Landed by opus-70 at `ba493a2cdd89`.
<!-- /horch:task T-324 -->

<!-- horch:task T-320 -->
### T-320 The operator-boundary probe counts "file not found" as "refused": on Linux it passes rules that allow the token read

- T-320: the operator-boundary probe proves that it sees the real fleet root; "not found" is a refusal only then (`c6dede2f`)
- T-320: the test machines of p7_conformance.rs are in /var/tmp on Linux; 1 test pins "not run" for a folder that the sandbox does not see (`0061d502`)
- comment sweep T-320: delete 17 comments that retell the code (T-328) (`99414fd1`)
- T-320: restore 4 comments of the sweep 66212b65 that state the security rule of the fix (`7f276a1a`)

<details><summary>6 files changed</summary>

- `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs`
- `crates/horch-core/src/fleetd/worker_profiles/application/conformance.rs`
- `crates/horch-core/src/fleetd/worker_profiles/domain/conformance.rs`
- `crates/horch-core/tests/p7_conformance.rs`
- `crates/horch-core/tests/p7_operator_boundary.rs`
- `crates/horch-e2e/src/probe.rs`

</details>

Landed by opus-hardening-19 at `7f276a1a27e7`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-320 -->

<!-- horch:task T-325 -->
### T-325 Pre-gate of Phase 6: run the Phase 6 gate rows now, on the tip of 2026-11-21, and report each red row

- fleetd-finish 212: pre-gate of Phase 6 at 895e3848 - each rule row green, arch_scan foreign-table guard red 3 of 3 (owner T-327) (T-325) (`5bc2d5e2`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/pre-gate-p6.md`

</details>

Landed by opus-71 at `5bc2d5e27302`.
<!-- /horch:task T-325 -->

<!-- horch:task T-323 -->
### T-323 Pre-gate of Phase 4: run the Phase 4 gate rows now, on the tip of 2026-11-21, and report each red row

- fleetd-finish 210: the pre-gate of Phase 4 on tip 895e3848 - 2 red rows: arch_scan foreign tables (14 lines of T-292, T-313, T-300) and 2 phase 4 lines of taskledger.txt (T-286) (T-323) (`99bf2a9f`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/pre-gate-p4.md`

</details>

Landed by opus-69 at `99bf2a9f2c6e`.
<!-- /horch:task T-323 -->

<!-- horch:task T-329 -->
### T-329 Draft of the description of the pull request to main: what fleetd changes, in detail

- fleetd-finish 214: draft of the description of the pull request to main (T-329) (`4e806d0b`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/pr-description.md`

</details>

Landed by opus-72 at `4e806d0bde6d`.
<!-- /horch:task T-329 -->

<!-- horch:task T-310 -->
### T-310 CI3: fourth CI run of the run branch (early signal while the last fix tasks run), and the difference to the third run

- CI3: the fourth CI result of fleetd on 3 OSes (`437190bc`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/ci-run-4.md`

</details>

Landed by qa-engineer-59 at `437190bc5ecd`.
<!-- /horch:task T-310 -->

<!-- horch:task T-339 -->
### T-339 CI4 FIX-35: Windows: the roster parser does not accept CRLF (369 tests)

- FIX-35: the roster parser accepts CRLF line ends (T-339) (`e120be16`)
- Comment sweep T-339: remove 2 doc comments that retell the CRLF code in roster/parser.rs (T-345) (`a850c183`)
- FIX-35: restore the comment that says why the roster parser accepts CRLF (T-339) (`f02070a6`)

<details><summary>1 file changed</summary>

- `crates/horch-core/src/roster/parser.rs`

</details>

Landed by opus-80 at `f02070a620cb`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-339 -->

<!-- horch:task T-286 -->
### T-286 Worktree lifecycle: the states `landing` and `landed` of the design do not exist, so the tree of landed work is never removed at the land

- fleetd-finish 168: worktree states landing and landed; the tree of landed work is removed at the land of its branch (T-286) (`497d9cf7`)
- comment sweep of T-286: delete 37 comments that retell code, name provenance or mark sections (T-326) (`9649d2bb`)
- fleetd-finish 168: restore 7 comments of the T-326 sweep that state why a tree moves, with the design ids (T-286) (`aa56d46b`)

<details><summary>13 files changed</summary>

- `crates/horch-core/src/fleetd/worktree_custody/adapters/sqlite.rs`
- `crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs`
- `crates/horch-core/src/fleetd/worktree_custody/application/remove.rs`
- `crates/horch-core/src/fleetd/worktree_custody/domain/worktree.rs`
- `crates/horch-core/tests/worktree_reconcile.rs`
- `crates/horch-core/tests/worktree_reconcile_acting.rs`
- `crates/horch-e2e/expected-failures/p3_unlanded.txt`
- `crates/horch-e2e/expected-failures/p3_worktrees.txt`
- `crates/horch-e2e/expected-failures/taskledger.txt`
- `crates/horch-e2e/tests/p3_refusals.rs`
- `crates/horch-e2e/tests/p3_unlanded.rs`
- `crates/horch-e2e/tests/p3_worktrees.rs`
- `crates/horch-e2e/tests/taskledger.rs`

</details>

Landed by opus-59 at `aa56d46b9ba0`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-286 -->

<!-- horch:task T-334 -->
### T-334 CI4 FIX-29: the 4 skills tests of skills_exposure.rs

- CI4 FIX-29: skills_exposure.rs counts no launch of the fleetd conformance job as the worker's agent (T-334) (`e15b0fcd`)
- T-346: comment sweep of T-334 removes the doc comment of agent_calls in skills_exposure.rs (`a99af2bb`)
- T-334: restore the comment of agent_calls that states why a conformance launch is not the worker's agent (`b1d5de88`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/tests/skills_exposure.rs`

</details>

Landed by opus-76 at `b1d5de880899`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-334 -->

<!-- horch:task T-331 -->
### T-331 CI4 FIX-26: the docs job: 1 rustdoc error in ingest.rs

- CI4 FIX-26: the docs job: ingest.rs doc names verifier::park_unlanded in code font, as the item is private (rustdoc::private_intra_doc_links, T-331) (`f82ac305`)

<details><summary>1 file changed</summary>

- `crates/horch-core/src/fleetd/task_execution/application/ingest.rs`

</details>

Landed by qa-engineer-60 at `f82ac305cf28`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-331 -->

<!-- horch:task T-341 -->
### T-341 CI4 FIX-38: Windows: tests that use /tmp, node, or expect LF text (51 tests)

- CI4 FIX-38: Windows tests use a temp folder that exists, a file URL for node, and LF-normalized Python output (T-341) (`6c0fba97`)
- Comment sweep T-341: remove 5 comments that restate the Windows temp folder branch (T-344) (`ee4af89f`)

<details><summary>7 files changed</summary>

- `crates/horch-core/tests/context_prime.rs`
- `crates/horch-core/tests/godot_wave.rs`
- `crates/horch-core/tests/p5_claude_sandbox.rs`
- `crates/horch-core/tests/p7_claude_exec.rs`
- `crates/horch-core/tests/p7_conformance.rs`
- `crates/horch-core/tests/pi_compact.rs`
- `crates/horch/tests/r_cli_cmds_ledger.rs`

</details>

Landed by qa-engineer-61 at `ee4af89f4a6a`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-341 -->

<!-- horch:task T-348 -->
### T-348 Land the work of task T-340 (CI4 FIX-36: a Windows verbatim path never reaches a child) with a Check that works on this host

- CI4 FIX-36: Windows: a verbatim path (\\?\) never reaches a child that start::Command starts (T-340) (`c95240f8`)
- Comment sweep T-348: remove 2 comments that retell code (T-349) (`5b189db0`)
- T-348: restore the comment that says when a path keeps its verbatim prefix (T-340) (`84043e89`)

<details><summary>9 files changed</summary>

- `crates/horch-core/src/fleetd/task_execution/application/launch.rs`
- `crates/horch-core/tests/bisect.rs`
- `crates/horch-platform/src/linux/path.rs`
- `crates/horch-platform/src/mac/path.rs`
- `crates/horch-platform/src/path.rs`
- `crates/horch-platform/src/start.rs`
- `crates/horch-platform/src/unix/path.rs`
- `crates/horch-platform/src/windows/path.rs`
- `crates/horch-platform/tests/path.rs`

</details>

Landed by qa-engineer-62 at `84043e89a474`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-348 -->

<!-- horch:task T-342 -->
### T-342 CI4 FIX-39: Windows: 4 tests of fleet18_messaging hang for hours, and the job has no time limit

- fleetd-finish 227: Windows: a detached child inherits no std handle of its parent; Harness::run has a time limit; CI jobs have timeout-minutes (T-342) (`b7c65960`)
- Comment sweep T-342: remove 8 comments that retell the code or a CI anecdote (T-350) (`aeee8bd8`)
- fleetd-finish 227: restore the reason of the platform test of the detached child after the comment sweep (T-342) (`e533f6e8`)

<details><summary>10 files changed</summary>

- `.github/workflows/ci.yml`
- `crates/horch-core/src/runtime/process.rs`
- `crates/horch-e2e/src/harness.rs`
- `crates/horch-e2e/tests/fleet18_messaging.rs`
- `crates/horch-platform/src/group.rs`
- `crates/horch-platform/src/linux/group.rs`
- `crates/horch-platform/src/mac/group.rs`
- `crates/horch-platform/src/unix/group.rs`
- `crates/horch-platform/src/windows/group.rs`
- `crates/horch-platform/tests/group.rs`

</details>

Landed by opus-82 at `e533f6e84751`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-342 -->

<!-- horch:task T-330 -->
### T-330 E1-prep-3: origin/main moved (3 new commits, PR #363 and #364, Release v0.5.2): bring them into the trial merge and prove it

- fleetd-finish 215: proof of the trial merge of origin/main at v0.5.2 (T-330) (`90072c5f`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/merge-main-proof-2.md`

</details>

Landed by opus-73 at `90072c5f88bd`.
<!-- /horch:task T-330 -->

<!-- horch:task T-351 -->
### T-351 Scope report: what fleetd needs to run its workers on the second pane host (hc: herdr-core and the Herdr desktop app)

- fleetd-finish 230: scope report of an hc adapter for fleetd: 15 gaps, 2 security findings, 9 units, 3 options (T-351) (`ea2b564c`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/hc-adapter-scope.md`

</details>

Landed by opus-86 at `ea2b564ce633`.
<!-- /horch:task T-351 -->

<!-- horch:task T-290 -->
### T-290 CI FIX-24: the 4 restore tests of p3_worktrees_cli wait for a rescue that does not come

- CI FIX-24: the 4 restore tests of p3_worktrees_cli set worktree_list_period_s = 5, so the rescue of the tree of T1 does not use 60 s of the 120 s wait (T-290) (`686940dd`)
- Comment sweep T-290: remove the doc comment on World::with_a_rescuable_tree that explains the fleetd rescue timing (T-353) (`09353487`)
- T-290: restore the comment of with_a_rescuable_tree that states why the 4 restore tests list each 5 s (T-353 sweep) (`090f02ca`)

<details><summary>1 file changed</summary>

- `crates/horch/tests/p3_worktrees_cli.rs`

</details>

Landed by opus-85 at `090f02ca61c5`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-290 -->

<!-- horch:task T-337 -->
### T-337 CI4 FIX-33: Linux: 4 security tests whose fixture is hidden by the sandbox (/tmp and loopback)

- CI4 FIX-33: 4 Linux security tests see the real target; the p7 gate script keeps its temp fleet in /var/tmp on Linux (T-337) (`215dc0c1`)
- CI4 FIX-33 comment sweep: drop 7 comments; SANDBOX_HAS_ITS_OWN_LOOPBACK names the Linux network namespace (T-337) (`f4b1e4b9`)
- T-337: restore 3 comments of the sweep 7e0cc4e1 that give the proof of the SR-3 test, the temp folder rule of the gate script and the drift idea filter (`737e1e71`)

<details><summary>4 files changed</summary>

- `crates/horch-core/tests/sr3_no_agent_push.rs`
- `crates/horch-e2e/tests/p4_verifier.rs`
- `crates/horch-e2e/tests/p5_operator_boundary.rs`
- `scripts/live/p7-real-conformance.sh`

</details>

Landed by opus-hardening-20 at `737e1e7147f0`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-337 -->

<!-- horch:task T-333 -->
### T-333 CI4 FIX-28 and FIX-31: tests that fail with "2 active runs ... Name one with --run", and the probe request file of the polite-stop test

- CI4 FIX-28 and FIX-31: the tests name their run with --run; the polite-stop tests run no conformance job (T-333) (`61874795`)
- Comment sweep T-333: remove 3 comments that restate why the tests name their run (T-352) (`d0d3b0e0`)
- T-333: restore the 3 comments of the sweep 685881fc that say why a test names its run (`656cc945`)

<details><summary>4 files changed</summary>

- `crates/horch-e2e/tests/p0_liveness.rs`
- `crates/horch-e2e/tests/p4_clock_and_heartbeat.rs`
- `crates/horch-e2e/tests/p4_release.rs`
- `crates/horch-e2e/tests/taskledger.rs`

</details>

Landed by opus-75 at `656cc9455f32`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-333 -->

<!-- horch:task T-355 -->
### T-355 Windows: test code that starts its own git with a canonicalize() path (about 100 tests)

- Windows: test code that starts its own git gets a plain path from for_child (T-355) (`f18eb76e`)

<details><summary>8 files changed</summary>

- `crates/horch-core/src/execution/lifecycle.rs`
- `crates/horch-core/src/fleetd/integration/application/commands.rs`
- `crates/horch-core/src/taskledger/finish/land.rs`
- `crates/horch-core/src/taskledger/finish/pr.rs`
- `crates/horch-core/tests/bundles.rs`
- `crates/horch-core/tests/w3_finish_steps.rs`
- `crates/horch-core/tests/worktree_create.rs`
- `crates/horch-core/tests/worktree_jobs.rs`

</details>

Landed by qa-engineer-63 at `f18eb76e9663`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-355 -->

<!-- horch:task T-332 -->
### T-332 CI4 FIX-27: Linux: the PATH of the pass helpers of p4_no_loss and p6_features

- CI4 FIX-27: the pass helpers of p4_no_loss and p6_features keep host-bin on the Linux PATH (T-332) (`e8eafe97`)
- CI4 FIX-27: i10 waits for the rescue ref, not for exactly 1 pass (T-332) (`c52341e0`)
- T-332 comment sweep: remove 5 comments from the PATH change of the pass tests (`0fbc7a9b`)
- T-332: restore the comments of the comment sweep that state the PL-5 pass budget and the no-CLI invariant of the pass PATH (`03a12e87`)

<details><summary>2 files changed</summary>

- `crates/horch-e2e/tests/p4_no_loss.rs`
- `crates/horch-e2e/tests/p6_features.rs`

</details>

Landed by opus-74 at `03a12e8736db`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-332 -->

<!-- horch:task T-327 -->
### T-327 arch_scan is red: 14 SQL lines of 3 landed tasks read a table of another context

- fleetd-finish 213: the multi-statement set readers of dispatch and the reap rows read 1 snapshot (T-327) (`a5987157`)
- fleetd-finish 213: each context reads the tables of another context through the reader of the owner (T-327) (`5efc8c11`)
- fleetd-finish 213: the detached-child test keeps no Child of the helper, so clippy::zombie_processes is clean (T-327) (`c65383a0`)
- fleetd-finish 228: a conformance check that fleetd cannot prove is "not proved"; agy is unsupported and makes no drift idea (T-347) (`f21c54e2`)
- fleetd-finish 228: the waits of the no-fleetd conformance test have the bound of 1 suite, DEADLINE plus 10 times FIRST_EVENT_LIMIT (T-347) (`8f0e1802`)
- fleetd-finish 228: the SR-8 test compares the full state of the ledger again; T-347 stops the drift idea of the unsupported agy (T-347) (`96eba319`)
- fleetd-finish 213: the fixture of the landing reads tests is TreeWorld, not Ledger (no_core_type_is_named_plain_ledger) (T-327) (`23bf4f4b`)
- fleetd-finish 213: the SQL trace of a read-only CLI counts the one_snapshot savepoint as a read (T-327) (`10584db0`)
- fleetd-finish 213: a test pins that the trace counts only the one_snapshot savepoint as a read (T-327) (`d025b4c0`)
- comment sweep T-327: delete 86 added comment lines on private items and test code (T-358) (`16d454b9`)
- fleetd-finish 213: restore the comments of the sweep 77439432 that give a snapshot invariant, a statement bound, a wait bound or a design rule (T-327) (`e9e07c76`)

<details><summary>31 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/adapters/jobs_store.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/adapters/sqlite.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`
- `crates/horch-core/src/fleetd/ideas/adapters/ideas_store.rs`
- `crates/horch-core/src/fleetd/ideas/adapters/questions_store.rs`
- `crates/horch-core/src/fleetd/ideas/application/sub_orchestrators.rs`
- `crates/horch-core/src/fleetd/integration/adapters/sqlite.rs`
- `crates/horch-core/src/fleetd/task_execution/adapters/sqlite.rs`
- `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`
- `crates/horch-core/src/fleetd/task_execution/application/launch.rs`
- `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs`
- `crates/horch-core/src/fleetd/worker_profiles/application/conformance.rs`
- `crates/horch-core/src/fleetd/worker_profiles/application/health.rs`
- `crates/horch-core/src/fleetd/worker_profiles/domain/conformance.rs`
- `crates/horch-core/src/fleetd/worktree_custody/adapters/sqlite.rs`
- `crates/horch-core/src/fleetd/worktree_custody/application/disk.rs`
- `crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs`
- `crates/horch-core/src/taskledger/runs.rs`
- `crates/horch-core/src/taskledger/sql_words.rs`
- `crates/horch-core/src/taskledger/store.rs`
- and 11 more files

</details>

Landed by opus-84 at `e9e07c766c1d`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-327 -->

<!-- horch:task T-359 -->
### T-359 The record of the last statement of a read-only ledger is 1 process-wide slot: a flaky test and a wrong error text

- fleetd-finish 233: the record of the last statement belongs to its read-only Store, not to 1 process-wide slot (T-359) (`8235f257`)
- fleetd-finish 233: the comment sweep removes the field comment on Store, since the SAFETY comment in on_statement states the drop order (T-359) (`59fbae13`)
- fleetd-finish 233: restore the field comment on Store that says why conn is declared before record (T-359) (`71ad6e5c`)

<details><summary>2 files changed</summary>

- `crates/horch-core/src/taskledger/store.rs`
- `crates/horch/src/cmd/taskledger.rs`

</details>

Landed by opus-88 at `71ad6e5cdc06`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-359 -->

<!-- horch:task T-338 -->
### T-338 CI4 FIX-34: Linux: worktree waits and launch timeouts of 8 tests

- CI4 FIX-34: 3 Linux test-setup faults of 6 named tests; fm2 and fr2 green on the tip (T-338) (`30bf47d7`)
- CI4 FIX-34: fr2 counts the ideas of each idea-lead launch over all ideas, since fleetd adds the idea of a failed conformance (FM-19) (T-338) (`d5fa03f4`)
- CI4 FIX-34: fm2 reads the push attempt of its worker, since the agent with no result can end its attempt before the status read (T-338) (`c7e167b5`)
- Comment sweep of T-338: delete 8 comments, name the sealed PATH, the PATH without sandbox tools and the failing job host (T-360) (`46881419`)
- T-338: restore 6 comments of the sweep b922fd25 that give a security rule or a design id (SR-4, the job host stub, rung 6 of T-307, FM-19, T-240, the sandbox-only git of the worktree job) (`5a6a8cb0`)
- T-338: restore the fm2 comment of the sweep b922fd25 that gives why the test reads the attempt and not the status row (`f6d8431b`)
- T-338: drop the fr2 change of e472ab67; T-327 stops the drift idea that the test must not accept (`adba2299`)

<details><summary>5 files changed</summary>

- `crates/horch-e2e/tests/fakes.rs`
- `crates/horch-e2e/tests/fleet18_observability.rs`
- `crates/horch-e2e/tests/p0_dispatch.rs`
- `crates/horch-e2e/tests/p6_sub_orchestrators.rs`
- `crates/horch/src/cmd/taskledger_watch.rs`

</details>

Landed by opus-79 at `adba229984ed`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-338 -->

<!-- horch:task T-364 -->
### T-364 Land the work of task T-336 (CI4 FIX-32: 5 tests of taskledger.rs and the unknown-teammate reason) with a Check of its own tests

- CI4 FIX-32: the fake codex sandbox confines a check on Linux; the check-timeout proof finds the processes on the host (T-336) (`a8d376a1`)
- CI4 FIX-32: a_launch_of_an_unknown_teammate_leaves_no_live_row matches the whole end reason of a failed spawn (T-336) (`8734f885`)
- T-364 comment sweep: drop 2 narration comments in fake-codex.rs and taskledger.rs (`92fb4a6b`)
- T-364: restore the doc of confined in fake-codex.rs that says what the fake sandbox confines (`4b818dd1`)

<details><summary>2 files changed</summary>

- `crates/horch-e2e/src/bin/fake-codex.rs`
- `crates/horch-e2e/tests/taskledger.rs`

</details>

Landed by qa-engineer-64 at `4b818dd16eb5`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-364 -->

<!-- horch:task T-363 -->
### T-363 Flaky under load: AgentShell reads an empty exit-code file (taskledger.rs:1660, ParseIntError Empty)

- taskledger e2e: each AgentShell has its own folder, and the shell writes its code file in 1 step (T-363) (`d2e710fd`)
- T-363 comment sweep: drop the AgentShell::start comment that explains the per-shell folder (T-371) (`58e00249`)
- T-363: restore the AgentShell::start comment of the sweep 256a96a2 that gives why each shell has its own folder (`09c74092`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/tests/taskledger.rs`

</details>

Landed by opus-89 at `09c7409299f9`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-363 -->

<!-- horch:task T-356 -->
### T-356 The test file p3_worktrees takes 12 minutes: find where the time goes and make it short without a weaker test

- T-356: p3_worktrees runs in 196 to 331 s, not 680 s: no docs jobs in the test world, a list period of 10 s and a real pass period of 1 s (`f7c102c8`)
- T-356 comment sweep: remove 8 comments in p3_worktrees.rs, and fleet_toml() takes the list period from LIST_PERIOD_S (`d2247974`)
- T-356: restore the comment of the RR-10 bound in rr10 that the comment sweep 908de05a removed (`03cb8d6f`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/tests/p3_worktrees.rs`

</details>

Landed by opus-87 at `03cb8d6f4b1c`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-356 -->

<!-- horch:task T-319 -->
### T-319 Linux: each stop of a job ends its sandbox (worktree jobs, and a KILL of the job group with a gate)

- fleetd-finish 208: each stop of a job ends its sandbox on Linux; a stop sends TERM first and the KILL after the grace of the recorded TERM (T-319) (`7a8e2a27`)
- Comment sweep T-319: remove 111 comment lines that narrate the code (T-367) (`d63d174e`)
- T-319: restore the comments of the sweep 18c1299c that give a limit, a rule or the fault that a test proves (`9e039412`)
- T-319 complexity sweep: hung_fetch_process_past_deadline_is_killed_and_releases_git_writer_for_queued_rescue (cognitive complexity 32, max 25) moves the start of the TERM-ignoring fetch, the TERM then KILL after the grace, and the checks of the rescue into 3 helpers; each assert is unchanged (`0f57b2ce`)
- T-319 comment sweep: worktree_jobs.rs drops 4 comments on the hung fetch helpers that retell the helper names and asserts (`59b47969`)

<details><summary>8 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/holds.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/domain/jobs.rs`
- `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`
- `crates/horch-core/tests/gate_jobs.rs`
- `crates/horch-core/tests/worktree_jobs.rs`
- `crates/horch-core/tests/worktree_jobs_stop.rs`
- `crates/horch-core/tests/worktree_rescue.rs`

</details>

Landed by opus-hardening-22 at `59b479699632`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-319 -->

<!-- horch:task T-365 -->
### T-365 Job processes of fleetd outlive the teardown of an e2e world by 25 s ("the test leaked N process(es)")

- T-365: the harness teardown stops each job of the world by the group that its jobs row records (TERM, the stop grace, KILL, then EXIT_LIMIT), since a job outlives its fleetd by design (design.md, "Long work is a job") and need not name the harness root (`2ddb204d`)
- T-365 comment sweep: drop 5 narration comments in harness.rs (`2d938cd7`)
- T-365: restore the docs of stop_jobs and job_groups and the reason of the teardown test that the sweep 0d756470 cut (`008e75a4`)

<details><summary>1 file changed</summary>

- `crates/horch-e2e/src/harness.rs`

</details>

Landed by opus-hardening-23 at `008e75a454df`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-365 -->

<!-- horch:task T-335 -->
### T-335 CI4 FIX-30: the steward triage tests, g50 advisory note, and rr10 adoption

- CI4 FIX-30: the g50 launch wait has the harness DEADLINE, and rr10 bounds the adoption by the steps of the test (T-335) (`e76dc9b0`)
- CI4 FIX-30: the 2 other launch waits of fleet18_observability have the harness DEADLINE, and g52 waits for the agent PID (T-335) (`25ff0e00`)
- T-374: comment sweep of T-335: drop 5 comments, the launch waits use LAUNCH_WAIT (`0cce44b2`)
- T-335: restore 2 comments of the sweep 2e6b06de that give the reason of a bound: the measured launch time of LAUNCH_WAIT and the rr10 step bound (RR-10) (`05ff4081`)

<details><summary>2 files changed</summary>

- `crates/horch-e2e/tests/fleet18_observability.rs`
- `crates/horch-e2e/tests/p3_worktrees.rs`

</details>

Landed by opus-77 at `05ff408109f3`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-335 -->

<!-- horch:task T-368 -->
### T-368 Flaky or a real race: fsx::tests::dirlock_concurrent_breakers_keep_one_holder failed 1 time

- fleetd-finish 237: a lock break removes only the lock it judged stale; a release holds the breaker guard (T-368) (`2dda73a9`)
- T-368 comment sweep: fsx.rs drops 9 comments on the lock release and break; the release guard is _breakers_excluded, stale_lock returns a StaleLock with the judged nonce, and the test receiver is next_took_lock (`bfe98185`)
- T-368: restore the reasons of the lock fix after the comment sweep, and add the Limits doc at remove_lock (`5ab3293c`)

<details><summary>1 file changed</summary>

- `crates/horch-core/src/fsx.rs`

</details>

Landed by opus-90 at `5ab3293ca92f`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-368 -->

<!-- horch:task T-379 -->
### T-379 Delta review (jobs-and-signals): the stop of a job, process groups, signal handlers and the sandbox of a job

- T-379: review jobs and signals delta (`12640aa6`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/reviews/delta-review-jobs-and-signals.md`

</details>

Landed by codex-sol-17 at `12640aa61a32`.
<!-- /horch:task T-379 -->

<!-- horch:task T-380 -->
### T-380 Delta review (ledger-reads-and-locks): set readers of a pass, snapshots, the read-only ledger and the folder lock

- fleetd-finish 243: review ledger reads and folder lock (`692668db`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/reviews/delta-review-ledger-reads-and-locks.md`

</details>

Landed by codex-sol-18 at `692668dbb50d`.
<!-- /horch:task T-380 -->

<!-- horch:task T-381 -->
### T-381 Delta review (custody-and-conformance): worktree custody, the reconciler, landings, the conformance verdicts and the question resume

- fleetd-finish 244: review custody and conformance delta (`25660806`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/reviews/delta-review-custody-and-conformance.md`

</details>

Landed by codex-sol-19 at `256608069bd4`.
<!-- /horch:task T-381 -->

<!-- horch:task T-377 -->
### T-377 Linux proof of each test that needs git 2.41 or newer: a test image with a new git, and where the product checks the git version

- T-377: Linux proof of each test that needs git 2.41: image t-git241-linux:local with git 2.51.2, 19 tests 3 of 3 green, and the gap list of the git version check (`269cae95`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/linux-git-241-proof.md`

</details>

Landed by opus-91 at `269cae9501e0`.
<!-- /horch:task T-377 -->

<!-- horch:task T-383 -->
### T-383 Review findings D242-F3 (Important) and D242-F4 (Minor): the Windows detached child fails closed, and 1 invariant comment of RR-13 comes back

- T-383: D242-F3 the Windows detached spawn fails closed when a std handle flag cannot be read or cleared, and restores every handle; D242-F4 restore the RR-13 invariant comment above refuse_moved_head (`c67ba24f`)
- T-387: comment sweep of T-383 - delete 9 doc comments in windows/group.rs that restate the code or its tests (`64a680f6`)
- T-383: restore the fail-closed doc of spawn_without_inherited_stdio that the sweep 64a680f6 removed (D242-F3) (`621c1960`)

<details><summary>2 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs`
- `crates/horch-platform/src/windows/group.rs`

</details>

Landed by opus-93 at `621c19605ecb`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-383 -->

<!-- horch:task T-382 -->
### T-382 Review finding D243-F1 (Critical): 2 live holders of 1 folder lock after the 5 s guard timeout of a release

- fleetd-finish 245: a release without the breaker guard removes nothing and marks its lock released (T-382) (`249faec1`)
- T-388: comment sweep of T-382 in fsx.rs - 14 comments removed, 1 kept (`9e17b608`)
- T-382: restore the lock invariants that the comment sweep 6d7835e7 removed: why a release holds the breaker guard, why a release without it marks and removes nothing, the stale-holder release check, the doc of the mark, the guard wait, and why the put-back never runs with a working guard (`72b48f3e`)

<details><summary>1 file changed</summary>

- `crates/horch-core/src/fsx.rs`

</details>

Landed by opus-92 at `72b48f3eecce`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-382 -->

<!-- horch:task T-390 -->
### T-390 1 file with each open decision of the operator, with the facts, the options and the recommendation

- fleetd-finish 251: 1 file with each open decision of the operator, with the facts, the options and the recommendation (T-390) (`1eddd07e`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/operator-decisions.md`

</details>

Landed by opus-96 at `1eddd07ee301`.
<!-- /horch:task T-390 -->

<!-- horch:task T-369 -->
### T-369 2 stop gaps that task T-319 found: the window before the handler of a check group, and a git that runs in the fleetd process with no stop path

- fleetd-finish 238 F4: a stop between the launch of a check or a gate and the store of its group ends that group (T-369) (`7d08119a`)
- fleetd-finish 238 F3: each git of the fleetd process stops at a time limit of 10 s; the stop sends TERM to its own group, then KILL after 1 s (T-369) (`65e6f563`)
- fleetd-finish 238 F3: a .lock file that stays after the KILL of a git of fleetd at its time limit gets 1 needs-you item until the operator removes it (T-369) (`f6482589`)
- fleetd-finish 238: the test module of the stop window imports EnvSource, so the arc_05 fix compiles (T-369) (`bf60e496`)
- fleetd-finish 238: a failed git read of the project file at a feature tip is a failed line of the pass; a project file that does not parse keeps its path (T-369) (`af81a088`)
- fleetd-finish 238 D242-F1: a stop of a job signals its group only while the leader runs with the recorded start; a leaderless group in the place of the recorded group gets no signal. D242-F2 is fixed by the hold of the termination handler in the F4 commit of T-369 (T-369) (`dbf50db6`)
- fleetd-finish 238 D242-F1: the leader-group stop of a task attempt signals only while the leader runs with the recorded start; the orphan-group stop of FM-22 names its limit (T-369) (`76ace426`)
- fleetd-finish 238 D242-F1: the teardown of the e2e harness signals a recorded pane or job group only while its leader runs with the recorded start (T-369) (`edf19799`)
- fleetd-finish 238: the lock test accepts the Linux text of the stop, where the sandbox of git ends on the TERM and SIGKILL ends the rest of its group (T-369) (`6702b94f`)
- T-392: comment sweep of T-369 - delete 27 comments that restate the code or the test names, keep the contracts, platform facts and D242-F1/T-369 reasons (`7ac2c2a8`)

<details><summary>25 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/adapters/effects.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`
- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`
- `crates/horch-core/src/fleetd/integration/adapters/exec_gate.rs`
- `crates/horch-core/src/fleetd/integration/application/branches.rs`
- `crates/horch-core/src/fleetd/integration/application/features.rs`
- `crates/horch-core/src/fleetd/integration/application/rebase.rs`
- `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`
- `crates/horch-core/tests/fleetd_pass_p4b.rs`
- `crates/horch-core/tests/worktree_jobs_stop.rs`
- `crates/horch-e2e/src/harness.rs`
- `crates/horch-marketplace/src/git.rs`
- `crates/horch-platform/src/group.rs`
- `crates/horch-platform/src/linux/group.rs`
- `crates/horch-platform/src/linux/signal.rs`
- `crates/horch-platform/src/mac/group.rs`
- `crates/horch-platform/src/mac/signal.rs`
- `crates/horch-platform/src/signal.rs`
- `crates/horch-platform/src/unix/group.rs`
- `crates/horch-platform/src/unix/signal.rs`
- and 5 more files

</details>

Landed by opus-hardening-24 at `7ac2c2a8be0b`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-369 -->

<!-- horch:task T-391 -->
### T-391 Refresh the draft of the pull request description for the changes since tip 99bf2a9f

- fleetd-finish 252: the draft of the pull request description is true for tip 72b48f3e: the section of the changes since the first draft, the new schema numbers, the open delta findings and the proof labels (T-391) (`8847bb7a`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/pr-description.md`

</details>

Landed by qa-engineer-67 at `8847bb7a306e`.
<!-- /horch:task T-391 -->

<!-- horch:task T-389 -->
### T-389 The operator learns that git is too old only when the first rescue fails: check the git version in horch doctor and at the start of fleetd

- fleetd-finish 250: horch doctor and the start of fleetd check the git version with the rule of the rescue (T-389) (`513b73e1`)
- T-393: comment sweep of T-389: remove 8 comments that retell the code (`92beba7a`)
- T-389: restore the 2 comments of the sweep d794abe2 that give a rule: the git that the doctor checks (SR-4), and the 1 check of the rescue rule before the fleetd row and any job (`9d8e3bdc`)

<details><summary>3 files changed</summary>

- `crates/horch-core/src/fleetd/worktree_custody/adapters/rescue_git.rs`
- `crates/horch/src/cmd/doctor.rs`
- `crates/horch/src/cmd/fleetd.rs`

</details>

Landed by opus-95 at `9d8e3bdc53aa`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-389 -->

<!-- horch:task T-386 -->
### T-386 Review finding D244-F3 (Important): a resume of a session that is gone burns an attempt, and at the last attempt no fresh worker gets the answer (RR-12)

- fleetd-finish 249: a resume that the CLI refuses burns no attempt and gives a fresh attempt at once (RR-12, D244-F3, T-386) (`dc93558a`)
- comment sweep T-386: remove 10 comments that retell the code, and rename Refuse::Every to Refuse::AllButFirst (`25aaa3de`)
- T-386: restore 3 comments of the sweep 25aaa3de: the bound of the attempt floor, the retry budget that used() counts, and the wait for the CLI call (`3d3e9a8a`)

<details><summary>5 files changed</summary>

- `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`
- `crates/horch-core/src/fleetd/task_execution/application/launch.rs`
- `crates/horch-core/src/fleetd/task_execution/application/stall.rs`
- `crates/horch-core/src/fleetd/task_execution/domain/retry.rs`
- `crates/horch-e2e/tests/p6_worker_question.rs`

</details>

Landed by opus-94 at `3d3e9a8ad45f`. comment-sweep: swept, complexity-sweep: 0 violations.
<!-- /horch:task T-386 -->

<!-- horch:task T-396 -->
### T-396 Delta review 2: the fixes of the review findings (signals, the folder lock, the git limit, the resume, the git version check)

- T-396: review the fixes of delta findings (`3acb0212`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/reviews/delta-review-2-fixes-of-findings.md`

</details>

Landed by codex-sol-20 at `3acb02123666`.
<!-- /horch:task T-396 -->

<!-- horch:task T-376 -->
### T-376 CI5: fifth CI run of the run branch (early signal while the last fix tasks run), and the difference to the fourth run

- CI5: the fifth CI result of fleetd on 3 OSes (run 44890393381, head 05ff4081) (`77fff9e1`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/ci-run-5.md`

</details>

Landed by qa-engineer-65 at `77fff9e14e24`.
<!-- /horch:task T-376 -->

<!-- horch:task T-378 -->
### T-378 Early full gate on macOS: the 3 gate commands of the goal on the tip of the run branch, and each red test with its cause

- T-378: early full gate on macOS: fmt and clippy green, 5 red tests in 4 e2e binaries, each with its cause (tip 05ff4081) (`688feefb`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/full-gate-macos-early.md`

</details>

Landed by qa-engineer-66 at `688feefb2b45`.
<!-- /horch:task T-378 -->

<!-- horch:task T-394 -->
### T-394 E1-prep-4: bring the fix wave of the run branch into the trial merge of main, and prove it

- E1-prep-4: proof 3 of the trial merge: the fix wave of the run branch at 3acb0212 in the trial, keep branch e3574c64 (T-394) (`a85aac5a`)

<details><summary>1 file changed</summary>

- `docs/specs/fleetd/gates/merge-main-proof-3.md`

</details>

Landed by opus-97 at `a85aac5a3ece`.
<!-- /horch:task T-394 -->
