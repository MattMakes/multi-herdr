# T-4: teardown leak evidence (failure 2)

Source: unit C1 (T-4, land of T-898), worktree
`fleet/fleetd-finish-T4-c1-land-t-898-fr-10-fleet-process-securi`, macOS
aarch64, 2026-11-20. Ticket 43 of fleetd-finish owns the fix.

## Count

3 of 30 runs of `scripts/dev-sandbox.sh cargo test -p horch-e2e --test
p6_fr10_approval` failed in teardown. Each run starts the 6 tests of the file
in parallel.

## Check that fails

`crates/horch-e2e/src/harness.rs:672`, in `Drop for Harness`: "the test leaked
<n> process(es) that outlived teardown by 5s; teardown killed them". The grace
is `REAP_GRACE` (5 s).

## Failures

Run A:

- `fix_a_idea_lead_attempt_stays_live_while_its_agent_runs`:
  - `36417 target/debug/horch job run 2 (ran at the first SIGTERM)`

Run B (2 tests of the same run):

- `fr10_b_agent_after_its_attempt_ended_cannot_approve_and_is_stopped`:
  - `72860 target/debug/horch job run 6 (ran at the first SIGTERM)`
  - `78016 target/debug/horch worker idea-lead-3 (ran at the first SIGTERM)`
- `fr10_a_agent_of_a_live_attempt_cannot_approve`:
  - `72735 target/debug/horch job run 2 (ran at the first SIGTERM)`
  - `72763 target/debug/horch job run 4 (ran at the first SIGTERM)`
  - `72816 target/debug/horch job run 6 (ran at the first SIGTERM)`

Run C: 1 more teardown failure of the same file. Its log was not kept.

## Host load

The load average of the host was 18 to 28 (`uptime`: 18.68 23.98 28.66)
during these runs. Other fleet workers ran builds and tests at the same time.
The failing runs took 70 to 74 s. A run with no failure takes 25 to 40 s.
Each test printed "has been running for over 60 seconds".

## What the evidence shows

- Each leaked process is a `horch job run <n>` or a `horch worker` of the
  test fleetd. Each one was alive at the first SIGTERM of teardown and still
  alive 5 s later.
- The failure is not in `fleet_process`, and not in the FR-10 rule.
- The cause is in the stop path of a job process or a worker after fleetd
  ends, or in the 5 s grace of the harness under load.
