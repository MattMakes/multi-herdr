# Early full gate on macOS

Task T-378, ticket `docs/tickets/fleetd-finish/241-full-workspace-gate-on-macos-early.md`.
Date: 2026-11-21. Host: macOS (Darwin 25.5.0), aarch64, 1 host that other fleets use.

Commit tested: `05ff408109f34d10dce7df94084c9b8979cb2cc9` (the worktree base, the tip of
`fleet/fleetd-finish` when T-378 started). The branch has moved since: its tip is now `77fff9e1`
(the CI5 report). This report does not cover that move.

Each claim has a label: **verified** (I saw it in a log, a file or a command in this run) or
**not verified** (I infer it). I fixed nothing.

## Result of each command

| Command | Exit | Passed | Failed | Time | Load (1 min) |
|---|---|---|---|---|---|
| `cargo fmt --all --check` | 0 | - | - | not timed | 20 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | - | 0 warnings | 29 s | 25 |
| `scripts/dev-sandbox.sh cargo test --workspace --locked --no-fail-fast` | **101** | 5274 | **5** (in 4 of 385 test binaries) | 6073 s (101 min), 02:37 to 04:18 MST | 24.5 at start, 21.3 at end, 17 to 27 between |

All 3 results are **verified**.

- The clippy run used the plain `-D warnings` form of the ticket. It is not the `--release --bins` form of CI.
- The test run used the default thread count, because the load was below 32 (24.5).
- Binaries: 381 green and 4 red. 31 tests are ignored. The log is not in the repository (scratchpad of the session).
- No test of the run leaked a process. No harness message `leaked N process(es)` is in the log (**verified**).
- After the run, the 5 red tests were run alone 3 times each (15 runs). The load of those runs was 10 to 27.

The gate is **red**: done criterion 3 is not met on this commit.

## Each red test

All 5 red tests stay red alone, 3 of 3 (**verified**). None is on the list of `crates/horch-e2e/expected-failures/` (**verified**, grep of the folder). So no row has cause class (d), and no row has cause class (c).

| Test | File | Message | Red alone (n of 3) | Cause class | Owner |
|---|---|---|---|---|---|
| `g56_later_spawn_receives_recorded_fact` | `crates/horch-e2e/tests/fleet18_brief_worktree.rs:352` | `timed out after 120 s waiting for worker brief with project facts` (`harness.rs:1409`) | 3 of 3 | (b) stale test setup | none yet |
| `arc_14_provenance_on_spawn_substitute_resume` | `crates/horch-e2e/tests/routing_provenance.rs:139` | `timed out after 120 s waiting for the refusal of the researcher launch` | 3 of 3 | (b) stale test of a retired text | T-397 |
| `the_hermetic_story` | `crates/horch-e2e/tests/scenario.rs:188` | `timed out after 120 s waiting for the refusal of the launch` (after PASS 1 to 7) | 3 of 3 | (b) stale test of a retired text | T-397 and ticket 255 |
| `the_hermetic_story_with_an_ollama_that_starts_in_7_s` | `crates/horch-e2e/tests/scenario.rs:188` | the same | 3 of 3 | (b) stale test of a retired text | T-397 and ticket 255 |
| `spawn_fails_with_the_error_of_a_worker_whose_brief_is_missing` | `crates/horch-e2e/tests/startup_error.rs:224` | `assert!(reason.starts_with("no brief at "))` fails; the reason is `its spawn failed: no brief at ...` | 3 of 3 | (b) stale test of a retired text | T-397 |

### Cause of each row

**`arc_14`, `the_hermetic_story`, `the_hermetic_story_with_an_ollama_that_starts_in_7_s`, `spawn_fails_..._brief_is_missing`.**

- Since T-254 (commit `893eb71d`), fleetd ends a failed spawn with the end reason `its spawn failed: <cause>` (**verified**: the end reason in each panic dump starts with `its spawn failed: REFUSED: ...`).
- The first 3 tests wait for the old text `launch_failed: REFUSED: ...`. The 4th asserts that the reason starts with `no brief at `. The fleetd behavior is right in each case. The test text is old (**verified**: I compared the dumps with the test lines).
- Commit `645b8a0b` (T-397, "move 3 stale refusal asserts to the end reason of T-254") changes exactly these 4 tests. It is on branch `fleet/fleetd-finish-T397-2-hermetic-story-tests-and-maybe-1-routi` only. It is not on the tested commit (**verified**: `git merge-base --is-ancestor`).
- I did not run the tests with that commit. That the commit makes them green is **not verified**.
- `ai_docs/plans/fleetd-finish/decisions.md` (10:56Z, T-384) names the 2 scenario tests and ticket 255 for them. The same file (11:43Z) names T-397 for `startup_error.rs`.

**`g56_later_spawn_receives_recorded_fact`.**

- The test does not reach a worker launch. I reran it once with `HORCH_E2E_KEEP=1` and read the kept ledger (**verified**).
  - The only worker row is the orchestrator pane `w1:p1`. The queued `sonnet` task never starts: no brief file, no `pane run` call.
  - The table `needs_you` holds the item `missing_fast_gate`: `fleetd holds new dispatch ... missing fast gate: add [gates] fast to fleet.toml`.
  - The test writes a `fleet.toml` with only `health_max_load_per_cpu`, `pass_period_s`, `fetch_period_s` and `worktree_list_period_s`. It has no `[gates]` table.
  - Other e2e tests that dispatch (`brief.rs:95`, `lifecycle.rs:46`, `p0_dispatch.rs:53`, `fleet18_observability.rs:362`) write `[gates]\nfast = "true"\nfull = "true"\n`.
- The rule came with commit `cba4667a` ("a project declares its gates in fleet.toml; hold missing fast gate"). The test was migrated in `5b23ea14` (T-86) and was never made to declare a gate (**verified** by `git log` and the file).
- A second `needs_you` item was in the same ledger: `request_file_rejected`, with `mode 0644 is wider than 0600` for a `horch-probe-*.json` request file. I do not know if it matters for this test (**not verified**). A fix ticket must check it.
- That `[gates] fast = "true"` makes the test green is **not verified**.
- `decisions.md` (11:43Z, CI run 5) lists this test as "TO TICKET, no ticket yet", red on macOS and Linux CI too.

### Open tasks T-289 and T-369

None of the 5 red tests belongs to the gate tree work of T-289 or the git time limit of T-369. No red test is red by their open work (**verified** by the 5 causes above). Their own tests were green in this run, but I did not check them one by one.

## Slowest test binaries

Time is the `finished in` value that cargo prints for the binary. All are in `crates/horch-e2e/tests/`.

| # | Binary | Time | Result |
|---|---|---|---|
| 1 | `p1_pass` | 405.0 s | green |
| 2 | `taskledger` | 347.0 s | green |
| 3 | `p1_jobs` | 323.0 s | green |
| 4 | `p4_clock_and_heartbeat` | 321.2 s | green |
| 5 | `p3_worktrees` | 254.7 s | green |
| 6 | `p6_worker_question` | 198.6 s | green |
| 7 | `p6_judge_pair` | 152.7 s | green |
| 8 | `i308_fleet_steward` | 152.2 s | green |
| 9 | `scenario` | 141.1 s | red (2 tests wait 120 s each, in parallel) |
| 10 | `p3_refusals` | 137.9 s | green |

The 11th and 12th binaries are `routing_provenance` (131.1 s, red) and `fleet18_brief_worktree` (128.0 s, red). Each red test that waits costs 120 s of its binary time. The whole run took 101 min, so the 4 red binaries cost about 7 min of it. All times are **verified**, at a load of 17 to 27. They are not a measure at low load.

## For the orchestrator

Proposed tickets, 1 for each cause group. All 5 red tests are in the cause class (b): the product is right and the test is old.

1. **Land T-397 (4 tests, text of the end reason).** Group: `arc_14`, `the_hermetic_story`, `the_hermetic_story_with_an_ollama_that_starts_in_7_s`, `spawn_fails_with_the_error_of_a_worker_whose_brief_is_missing`.
   - Commit `645b8a0b` already changes them. It waits at its sweep step (decisions.md 11:46Z, T-403), and the usage limit holds the sweep.
   - Action: land T-397 on the run branch. Then run `cargo test -p horch-e2e --locked --test scenario --test routing_provenance --test startup_error` 3 times. Close ticket 255 as a duplicate when T-397 lands, or name the owner of the `scenario.rs` change.
2. **New ticket: `g56_later_spawn_receives_recorded_fact` declares a gate.**
   - File: `crates/horch-e2e/tests/fleet18_brief_worktree.rs`, in the `fleet.toml` text at line 369.
   - Action: add `[gates]\nfast = "true"\nfull = "true"\n` as the other dispatch tests do. Check the `request_file_rejected` item (mode 0644 of `horch-probe-*.json`) in the same run.
   - Check: the test is green alone 3 of 3, and the file is green as a whole.
3. **Gate rule for the end tasks.** The gate must run `cargo test --workspace --locked` as a whole on the tip after the 2 tickets above. This early gate shows that the narrow Check of the fix tasks missed these 5 tests. The 5 tests were red since T-254 (`893eb71d`) or T-86 (`5b23ea14`).

What I did not check:

- the 31 ignored tests (not a gate failure);
- a second run for flakes: the 381 green binaries ran once, so a test that fails 1 time in many is not covered;
- the `--release --bins` clippy form of CI.
