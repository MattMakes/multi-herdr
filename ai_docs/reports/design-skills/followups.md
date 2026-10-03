# D10 followups: report

Unit: `followups`. Branch: `ds/followups`. Author: opus-31.

## Summary

- The Antigravity gaps from D08 are closed, except `horch cost`. That gap is documented below.
- `horch agent-list` prints skill exposure in kebab case.
- The skills README describes 2 upstream families.
- I found and fixed 5 causes of load flakes: 3 product bugs and 2 test-infrastructure bugs. They cover the 6 named flakes and 2 flakes found while I reproduced. Each cause has a regression test or a measurement.
- The full gate passed 3 times in a row under load. See "Evidence".

## Step 1: Antigravity gaps and `as_str`

| Gap | Change |
|---|---|
| `dataset/preflight.rs` `kind_of` lacks Antigravity | `kind_of` now searches `HarnessKind::ALL`, so a new kind cannot be missed again. Unit test `kind_of_names_every_harness`. |
| `FAKES` does not install `fake-antigravity` | `("agy", "fake-antigravity")` is in `FAKES`. `with_agy` in `tests/lifecycle.rs` only sets `HORCH_ANTIGRAVITY_BIN` now (see cause 4). |
| quota display does not show `google` | `POOLS` has `google` before `local`. `horch quota` always lists it. The telemetry screen lists it only when a reading exists (orchestrator decision): `pool_table` has a new `only_with_reading` argument. Both telemetry goldens are unchanged. Unit test `google_pool_row_only_with_a_reading_on_the_screen`. The `horch quota` help line names `google`. |
| `horch cost` reads no agy usage | Not fixed. The research report finds no local usage or transcript file: usage is only in the `/usage` TUI panel and in the headless JSON, and horch runs the TUI. `telemetry/readers.rs` still reports "unknown agent 'antigravity'". |
| `skills=PluginDir` | `SkillExposure::as_str` gives `none`, `plugin-dir`, `skill-flag`, `config-paths`, `codex-home`. `harness/inventory.rs` uses it (1 line, approved). `agent_list.rs` expects `plugin-dir`. |

## Step 2: skills README

The intro now names 2 families. They are the process skills from `MattMakes/skill-marketplace` and the design skills from the 7 MIT design repositories. The intro also names the 2 bundles that belong to neither family (`orchestrate`, `skill-creator`). One sentence under "Source mapping" says that a path with no repository name is a `skill-marketplace` path. I did not change any table rows.

## Step 3: flake causes

The table below gives each cause, its fix and its evidence. "Under load" means 18 CPU burners plus 2 loops that run every workspace test binary (scripts below).

| # | Flake | Cause | Fix | Evidence |
|---|---|---|---|---|
| 1 | `mea_10` ("locking ledger ... No such file or directory") | Product, `fsx.rs`. 2 processes saw the same stale `DirLock` and both broke it. The slower one renamed away the new lock that the faster one had just made. The faster one then failed to write `owner` (ENOENT), or held a lock that no longer existed. Stale ledger locks are normal: the coordinator kills panes. | A breaker first takes `<name>.lock.break/` and checks again that the lock is stale. A process that cannot take it waits and retries. A breaker dir older than 5 s (`BREAKER_STALE_AFTER`) was left by a killed process and is removed, so it never blocks. | `dirlock_concurrent_breakers_keep_one_holder` (8 threads, 1 dead lock, 10 rounds: exactly 1 holder, no error) failed 3 of 3 before the fix with the same ENOENT. `dirlock_left_breaker_does_not_block`. `mea_10` passed 10 of 10 under load. |
| 2 | `cmp_07_pane_vanished` ("claude: horch done failed"), found while reproducing | Product, `execution/lifecycle.rs`. The coordinator closes the pane of a candidate whose record is done. `horch done` closes the same pane. When the coordinator wins, `horch done` gets "no pane" and exits 1. | `horch done` treats a failed close as success when `pane get` shows the pane is gone. | `done_close_is_idempotent` with `FakeWorkspace`: close fails and pane gone, so done succeeds; close fails and pane still there, so done returns the error. It fails without the fix. |
| 3 | `cmp_04` (C frozen with an empty numstat), `cmp_05` (spawned 1 of 2) | Test infra, `fake-herdr`. `pane close` kills a pane's process group. A fake-herdr call in that group can hold the state lock, so the lock stays stale for 10 s. Then several waiters break it at once (I logged 3 PIDs breaking 1 lock together). 2 calls then read and write the state concurrently, and a pane can be lost from the state. A lost pane reads as vanished, so the candidate ends before it writes anything. A real herdr call is a request to a server that finishes even when the client dies. | Each fake-herdr call runs in a copy of itself in a new process group (`process_group(0)`), so a pane kill cannot interrupt a call that holds the lock. The state is saved through a temp file and a rename. | The temporary lock logging showed stale breaks in 4 of about 80 passing test runs before the fix. The `cmp_0*` tests passed 10 of 10 under load at load 43. |
| 4 | `cmp_05`, `cmp_13`, `sec_03` (PRE-06 "harness version unresolved", exit 4); `agent_list_no_probe_runs_no_binary` | macOS scans every new executable on its first run. The harness copied 8 fakes per test, and the agent-list test writes fresh scripts. I measured 60 concurrent first runs at load 18: copied binaries had median 2.88 s, p90 5.15 s, max 5.63 s. Fresh scripts had max 4.95 s. A second run of the same file takes 0.015 s. The old probe limit was 5 s. | The harness hard-links the fakes (first run of a link: 0.013 s), with a copy as fallback when linking fails. `Harness::write_bin` removes an entry before it writes, because writing through a link changes the built file. I confirmed that `std::fs::copy` onto its own hard link empties it (`Ok(0) a=0 b=0`). `check_built_fakes` fails a test when a built fake in `target/` changed size or is empty. It runs at harness creation and drop. The orchestrator's 15 s probe limit stays. | `cmp_13`, `sec_03`, `cmp_05` passed 10 of 10 under load. `agent_list` passed 10 of 10 under load. A temporary test that wrote through `bin/claude` failed with "built fake ... changed from 1576560 to 4 bytes". The dataset target went from 50 s to 42 s. |
| 5 | `jdg_e2e_round_decided` (no `judge.started`), found in the first set of gates under load | Product. `judge_job.rs` started the heartbeat thread, and a fast judge could set `stop` before the thread wrote its first beat. `complete()` emitted `judge.started` only from a heartbeat, so no event was written. | Job side: the first heartbeat is written before the judge runs. Coordinator side (`competition/judging.rs`, as the orchestrator asked): `ensure_started` records `judge.started` before `judge.completed` or `judge.failed` when none exists for the attempt. It uses pid 0 when no heartbeat exists. The idempotency key makes it happen once. | `heartbeat_is_written_before_the_judge_runs`. `jdg_started_recorded_when_the_job_left_no_heartbeat` (new no-heartbeat `FakeJob` variants in `tests/judging.rs`) fails without the coordinator fix. `jdg_*` passed 10 of 10 under load. |

`cmp_13`, `cmp_05` and `sec_03` (cause 4) and the `agent_list` flake failed only in gate logs from before 04:51. At 04:51 the orchestrator raised the probe limit to 15 s. The measurement gives the cause, and the hard links remove it for the e2e tests.

## Evidence: 3 full gates under load

`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` ran with 12 CPU burners and 2 loops over every workspace test binary (57 binaries). Results on `49ce3ff`, rebased on `design-skills` at `45a8fff`:

- Gate 1: GREEN in 226 s, load average 20.7 / 15.6 / 16.3.
- Gate 2: GREEN in 226 s, load average 22.5 / 17.9 / 17.0.
- Gate 3: GREEN in 229 s, load average 22.1 / 19.0 / 17.7.

An earlier set failed in gate 2 with cause 5. I fixed it and restarted the count. The scripts were `/tmp/opus31/gateload.sh` (target in a loop, with load) and `/tmp/opus31/gateunderload.sh` (gate with load). A CPU burner alone (load 58) did not reproduce any flake. The process churn of parallel test suites is what matters.

## Files changed outside the first plan list (each approved by the orchestrator)

- `crates/horch-core/src/harness/inventory.rs` (1 line, `as_str`).
- `crates/horch/src/main.rs` (1 doc line, `horch quota` help).
- `crates/horch/src/cmd/telemetry.rs`, `cmd/quotacmd.rs` (the pool display).
- `crates/horch-core/src/fsx.rs` (cause 1).
- `crates/horch-core/src/execution/lifecycle.rs` (cause 2).
- `crates/horch-e2e/tests/lifecycle.rs` (`with_agy` copy removed, cause 4).
- `crates/horch/src/dataset/judge_job.rs`, `crates/horch-core/src/competition/judging.rs`, `crates/horch-core/tests/judging.rs` (cause 5).

No oracle or golden changed.

## Decisions

- Not `File::lock` in fake-herdr: the workspace declares `rust-version = "1.85"`, and `File::lock` needs 1.89.
- The fsx fix keeps the `create_dir` lock and its on-disk shape, so old binaries that use the mkdir spinlock still exclude new ones.
- I added no retries. Every fix removes a race or a cause.

## Gotchas

- `cargo test -p horch-e2e` does not rebuild the `horch` binary. Run `cargo build --workspace --bins` first, or e2e tests run a stale `horch`.
- `HORCH_E2E_KEEP=1` keeps a test's temp dir. Judge bundle dirs are mode 0500: run `chmod -R u+w` before you delete them.
- Never write into a harness `bin/` entry: it can be a hard link to `target/debug/fake-*`. Use `Harness::write_bin`. `check_built_fakes` catches a mistake. After a mistake, delete the damaged `target/debug/fake-*` and rebuild.

## Not done, and follow-ups (outside my scope)

- `horch cost` for agy: no local usage source exists (see step 1).
- `teammates/_base/fleet-orchestrator.md:115` says `horch quota` shows "claude, codex, opencode-zen, local". It now also shows `google`. The golden in `golden_prompts.rs:349` holds the same text. Teammates are outside my scope.
- `horch done` calls `pane get` before it closes the pane. If the coordinator closes the pane between `mark_done` and that call, `horch done` still exits 1. The window is smaller than the close race in cause 2, and I did not see it fail.
- fake-herdr reuses a pane id after `pane close` (`w1:p2` again). I do not know if real herdr reuses ids. If it does, a late close by id can hit a newer pane.
- fsx residual: if a breaker dies inside its microsecond window and 2 processes then remove its stale breaker dir at the same moment, the double break can return. This needs 2 rare events together.
- `tests/promotion.rs:89` makes a new `bin/sh` symlink. It writes no file content, so I did not convert it to `write_bin`.
