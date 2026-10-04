# D19 dataset-tidy: the D18 follow-ups, test teardown, pid reuse, clippy

Branch `ds/dataset-tidy`, worker opus-47. Plan:
`ai_docs/plans/design-skills/d19-dataset-tidy.md`, plus items 6 to 8 that
the orchestrator added during the unit.

Merge 1 held items 1 to 6 and the pid-reuse fix. Merge 2 adds items 7 and
8 (2 flakes): diagnostics and the loaded runs below. Neither flake
reproduced, so neither has a proven root cause.

| Item | Commit | State |
|---|---|---|
| 1. Round projection file in the design | `Dataset: Drop the round projection file no code writes` | Done |
| 2. A candidate's `horch done` re-tiles the dataset workspace | `CLI: Leave the dataset layout to the coordinator on a candidate's done` | Done |
| 3. fake-herdr state lock break | `Tests: Break fake-herdr's state lock only for a dead holder` | Done |
| 4. Leaked `horch worker` and fake agent after a test | `Tests: Kill a test's processes at teardown and fail a test that leaks one` | Done |
| 4b. Pid reuse in fake-herdr kills (found in item 4) | `Tests: Signal a pane's process group only while it is the same group` | Done |
| 5. Clippy | `Lint: Fix every workspace clippy warning; gate on clippy -D warnings` | Done |
| 6. Coordinator test for operator skills (added) | `Tests: A dataset candidate's record lists its operator skills` | Done |
| 7. `arc_26_e2e_lifecycle_matrix_{codex,opencode}` session id timeout (added) | `Tests: Name the exit status of an empty fake --version, and the pane output of a session id timeout` | Not reproduced; diagnostics added |
| 8. `fake_prime_creates_session_file` empty `--version` stdout (added) | same commit | Not reproduced; diagnostics added |

The gate (`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`) is green
after each commit.

## 1. The round projection file

No code reads or writes `experiments/<exp>/rounds/<round>.json`. `status`,
`rebuild` and `export` fold the events on each read (MEA-05). A stored
projection would be a second copy that can go stale. So I deleted it, with
the orchestrator's approval (option 1):

- Design §2.1: the layout line and the `round_file` line of the struct
  sketch are gone. A paragraph under the layout says why no projection is
  stored.
- `DatasetPaths::rounds_dir` and `DatasetPaths::round_file` are deleted
  (`measure/paths.rs`). They had no caller outside tests.
- `tests/measure.rs` (layout table) and `tests/promotion.rs` (traversal
  checks): the 4 lines that called them are removed. No other assertion
  changed.

## 2. A candidate's `done` and the dataset layout

Decision: the re-tile is wrong there. The coordinator owns the dataset
workspace layout (the `watch` root pane and the candidate panes) and spawns
candidates with `tiling: Disabled`. The candidate's own `horch done` still
started a detached `horch tile` or `horch balance` against that workspace.

Fix (`crates/horch/src/cmd/messaging.rs`): `done` reads its own ledger
record. When the record is a candidate or a judge
(`ExecutionKind::Candidate`/`Judge`), the settle step does nothing. A judge
has no pane, so it never runs `done` in a pane; the check covers it for
completeness. A record that cannot be read or typed settles as before.

Test: `done_settles_only_outside_the_coordinator_layout` (worker, candidate,
judge, missing record).

Evidence, `cmp_05_e2e_candidates_in_dataset_workspace`, fake herdr calls:

| | `tab list` | `workspace list` | `pane get` |
|---|---|---|---|
| Before | 2 | 3 | 14 |
| After | 0 | 1 | 10 |

The 2 `tab list` calls and the extra calls came from the 2 detached
`horch tile` processes, one for each candidate.

## 3. fake-herdr's state lock

Measured with temporary timing in fake-herdr (removed). Load: 18 `yes`
processes, the `lifecycle` and `dataset` e2e binaries 9 times, load average
28 to 62, 7624 herdr calls:

| | p50 | p99 | p99.9 | max |
|---|---|---|---|---|
| Lock wait | 2.2 ms | 61.9 ms | 237 ms | 424 ms |
| Lock hold | 3.2 ms | 106 ms | 297 ms | 501 ms |

So the old numbers (break after 10 s of age or 30 s of waiting) were never
reached in these runs. But the break rule itself was wrong in 2 ways:

- It could break a live lock. A waiter that waited 30 s removed the lock of
  whatever call held it at that moment. A slow holder then loses the state.
- It spun for ever when the harness root was gone. A detached `horch tile`
  calls herdr after `Harness::drop` deletes the temp dir. `create_dir` then
  fails with `NotFound`, nothing breaks the loop, and the call polls every
  5 ms for ever. I found pid 67482 (`horch tile --workspace w1` from
  `integ-ds/target`) and 2 fake herdr children stuck like this for 5 min.
  The orchestrator killed them.

Is a break still needed after D10 (each call in its own process group)? Yes:
only a SIGKILL of the call itself now leaves a stale lock, but V3 showed that
`syspolicyd` can send exactly that SIGKILL.

Fix (`crates/horch-e2e/src/bin/fake-herdr.rs`):

- The holder writes its pid into `<lock>/pid`.
- A waiter checks the holder once a second. It takes the lock over only
  when that pid no longer runs, or when the lock dir has had no `pid` file
  for 10 s (the holder died between `mkdir` and the write). A live holder
  is never broken.
- A `create_dir` error other than `AlreadyExists` ends the call with exit 1
  and `the harness is gone`.

Tests (`tests/fakes.rs`), both red on the old lock and green on the new one:

- `fake_herdr_fails_when_the_harness_root_is_gone`.
- `fake_herdr_breaks_only_a_dead_holders_lock`: a dead holder's lock is
  taken over at once; a live holder's lock is kept for 3 s and released
  only by its holder.

After the fix, 4 more loaded runs of `lifecycle` and `dataset` passed
(load 28 to 41).

## 4. Process leaks after a test

Root cause: `Harness::drop` deleted the temp dir and nothing else. fake-herdr
`exec` starts each pane command (`horch worker` and its agent) in its own
process group. Only `pane close` (through `horch done`) or `workspace close`
killed that group. A test that failed before `horch done` left the group
running, and a `stay` fake runs for ever. The 13 h leak of
`tel_session_recorded_when_agent_ends_fast` was such a test: V3 found that
`syspolicyd` killed a fake there, so the test failed before `done`.

Live evidence when I started: `horch worker codex-sol-1` (pid 49930) and its
fake `codex` (pid 50025) from `mh-wt/followups`, 13.5 h old, plus 2 similar
leaks from other worktrees (`arc26-opencode`, `arc26-codex`).

Fix (`crates/horch-e2e/src/harness.rs`, `crates/horch-e2e/src/process.rs`):

- `Harness::drop` calls `reap`. It sends SIGTERM to the process group of
  each pane that fake-herdr recorded and that is still the same group (see
  4b), and to the group of each process whose command line names the
  harness root (the fakes are started by path from `<root>/bin/`).
- It waits up to 5 s (`REAP_GRACE`). A process of those groups, or one that
  names the root, that still runs then is a leak: teardown sends SIGKILL to
  its group, and the test fails with the list (only when the test has not
  failed already).
- The test process's own group is never signalled.

Tests:

- `teardown_kills_the_open_panes`: a pane left open is killed, and the test
  passes.
- `teardown_fails_a_test_that_leaks_a_process`: a process that ignores
  SIGTERM fails the test and is killed.

Both are red without `reap`. The full `horch-e2e` suite passes with the
check: no existing test leaks.

Note: `ps -E` does not show other processes' environments in this
environment, so the check matches the command line and the recorded groups,
not `HORCH_FAKE_LOG` in the environment.

## 4b. Pid reuse: a fake-herdr close can kill another process group

Found while I wrote item 4. fake-herdr's `pane close` and `workspace close`
ran `kill -TERM -<pid>` on the pid that `pane run` recorded. The claude and
pi fakes exit at once, so that pid is free long before the close. Under gate
load, macOS pids wrap in well under 1 h (I saw pids cycle from 14xxx to
72xxx in about 40 min). If a new process group leader gets the pid in that
window, the close kills that whole group. The leader of a `just gate` run in
another worktree is such a leader: this is a probable cause of the SIGTERM
that ended a gate in `mh-wt/integ-ds`. I did not reproduce that kill; the
mechanism is certain, the attribution is not.

Fix:

- `pane run` records the leader's start time (`ps -o lstart`, to the second)
  next to its pid, in `state.started`.
- `kill_group` signals the group only while it is the same group
  (`process::is_same_group`): the leader runs with the recorded start time,
  or the leader ended and the group still has members (a group id is not
  given out again while the group has members). A group without a recorded
  start time is not killed.
- `reap` uses the same check for recorded groups.

Test: `a_reused_group_id_is_not_the_same_group` (unit test in
`process.rs`).

## 5. Clippy

`cargo clippy --workspace --all-targets` had 35 warnings in 23 sites (the
D18 list plus `herdr-install`, `horch`, `horch-e2e`). All are fixed:

- Machine fixes: `clone_on_copy` (`preflight.rs`, `export.rs`),
  `needless_question_mark` (`main.rs`), `nonminimal_bool` (`smoke.rs`,
  `baseline_oracles.rs`), `filter_next` to `rfind` (`tile.rs`),
  `cloned_ref_to_slice_refs` to `std::slice::from_ref` (5 sites),
  `unnecessary_sort_by` to `sort_by_key(Reverse)` (2 sites).
- Doc lints: `operator.rs` (a line that started with `>` read as a quote;
  reworded), `layout.rs` (list indentation), `fake-claude.rs` (a blank line
  before a paragraph).
- `type_complexity`: type aliases `TokenRule` (`redact.rs`) and `ModelRow`
  (`inventory.rs` test).
- 2 `allow`s with a reason: `enum_variant_names` on `Os` (`MacOs` is the
  platform's name) and `large_enum_variant` on `PromotionResult` (one value
  per promotion; a `Box` would only add noise to every caller).

`cargo clippy --workspace --all-targets -- -D warnings` exits 0, so
`scripts/phase-gate.sh` now runs it after the build steps. The new step
already caught 2 `needless_borrow`s in my item 6 test before the commit.

## 6. Coordinator test for operator skills

`crates/horch-core/tests/coordinator.rs`:

- The arc_24 setup (about 150 lines) is now `crash_round(w, roster, shape)`:
  one 2-candidate round in process in which every agent writes `work.txt`
  and crashes. `shape` may change the plan before the round starts.
  `arc_24_candidates_are_ordinary_executions` calls it with `|_| {}`; its
  assertions did not change.
- New `arc_24_candidate_record_lists_operator_skills`: a temp roster overlay
  adds `op-probe` (claude, `skills: [tdd]`, `operator_skills` from
  `~/.agents/skills`), the world's home holds `test-modernizer`, and
  `shape` puts `op-probe` in both slots. Each candidate record lists
  `["tdd", "test-modernizer"]`, the second with an `operator+` version.
  Red without the coordinator's `with_operator_skills` call (`["tdd"]`),
  green with it.

## 7 and 8. The 2 flakes

### What is known

- Item 7: `arc_26_e2e_lifecycle_matrix_{codex,opencode}` waited 30 s for
  the session id and timed out (opus-46: load 27 and 40, 1 in 40 binary
  runs before V3, 1 in 200 after). One instance left a live fake `codex`
  (pid 26533) and its `horch worker` (pid 26337) from `loose-ends`: the
  worker started at 20:28:53 and the agent at 20:28:54. So the agent ran
  within 1 s, and the session discovery did not record its id within the
  next 29 s.
- Item 8: `prime-agent --version` printed nothing in the P-UE gate
  (`/tmp/ue-teammates-gate2.log`, base 27d2df6, about 21:00). The test
  checked only stdout, so the exit status is unknown.
- `syspolicyd` killed nothing in that period. Its last "Malware rejection"
  lines are 18:12, 18:37, 18:45, 19:53, 19:58 and 20:03 (all before the V3
  symlink fix). So the V3 cause does not explain either flake. Gotcha: in
  zsh, `log` is a builtin; use `/usr/bin/log show`.
- `cargo test` runs a test binary in the caller's process group. So a
  fake-herdr pid-reuse kill (4b) that hit the `fakes` test would end the
  whole gate, not 1 process. 4b does not explain item 8.

### Loaded runs

The `arc_26_e2e_lifecycle_matrix` tests (5) and the `fake_prime` and
`fake_opencode` tests (3), as one run, under 12 `yes` processes and a
`cargo build --workspace --all-targets` / `cargo clean` loop in a separate
target dir (load average 28 to 49):

| Code | Runs | Test runs | Failures |
|---|---|---|---|
| ds/dataset-tidy (merge 1 code) | 150 | 1200 | 0 |
| design-skills b2ebc7c (before merge 1) | 150 | 1200 | 0 |

The rate seen in gates (1 in 200 binary runs) needs more runs or the real
condition: several worktrees' gates at once.

### Candidate mechanisms (not proven)

For item 7, each discovery poll makes 1 fake herdr call (`pane get`) and,
on a find, 1 ledger write. Before merge 1, a fake herdr call could wait up
to 10 s behind a dead holder's lock, and a ledger `DirLock` waits up to
15 s (`LOCK_STALE_AFTER`) behind a dead holder. Two such waits in one poll,
plus the 0.5 s, 1 s, 2 s, 3 s schedule, pass 30 s. Merge 1 removes the
first wait for a holder that died (the pid check finds it within 1 s).

### What changed

- `tests/fakes.rs`: `version(h, name)` asserts success and a non-empty
  answer, and names the exit status (or signal) and stderr. The
  `opencode` and `prime-agent` version checks use it.
- `tests/lifecycle.rs`: the session id wait of the `arc_26` matrix panics
  with the record and the worker pane's output (fake-herdr `exec` writes
  it to `<log>.pane-<id>.out`). The worker prints there when its session
  discovery does not start.

The next failure of either test names its cause class: a signal, an exit
code, or a worker message.

## Decisions

- Item 1: delete, not build the cache. No reader needs it.
- Item 2: skip by the record's kind, not by `report_to`. The kind says what
  the execution is; `report_to` only says who hears about it.
- Item 3: a liveness check, not new numbers. The measured maximum (0.5 s)
  is far below any break time, so the timing was not the problem; breaking
  a live holder was.
- Item 4: a leak fails the test only when the test has not failed already,
  so the first failure stays the visible one.

## Gotchas

- A red run of the leak test without the fix left a SIGTERM-deaf stub
  running and held the test's stdout pipe, so `cargo test | grep` never
  ended. The test now gives the stub null stdio and kills it itself.
- A child of the test that teardown killed stays a zombie until the test
  reaps it, and `kill -0` still succeeds on a zombie. The leak test waits
  for its child before it checks.
- Kill only by exact pid. Other worktrees run gates on this machine.

## Follow-ups (outside my scope)

- Old leaks from other worktrees were still running when I reported them:
  pid 26533 (fake codex, `arc26-codex`), 50025 with worker 49930
  (`tel-fast-end`), 65343 with worker 65257 (`arc26-opencode`). The
  orchestrator decides.
- `horch done` in a dataset candidate still runs `record_session` and
  `unregister`; both are cheap and correct, so I kept them.
