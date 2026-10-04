# D22 install-checks report

Unit `install-checks`, branch `ds/install-checks`. Two defects from the
operator's first install of `design-skills` (2026-10-04), fixed at the root.

## 1. `horch smoke messaging` always failed

### Cause (evidence)

`Herdr::wait_output` ran `herdr wait output <pane> --match ... --timeout ...`.
herdr 0.8.2 has no top-level `wait` command:

```
$ herdr wait output x --match y --timeout 10; echo $?
unknown command: wait
run 'herdr --help' for usage
2
```

`wait_output` returned `status.success()`, so exit 2 read as "timeout" and
the smoke reported `FAIL: SMOKE_TEST_42 not observed` at once, although the
text was on the pane. The command is now `herdr pane wait-output`. Its 0.8.2
answers, captured on the operator's server:

| case | exit | output |
|---|---|---|
| match | 0 | stdout `{"id":"cli:pane:wait-output","result":{"type":"output_matched","matched_line":...}}` |
| timeout | 1 | stderr `{"error":{"code":"timeout","message":"timed out waiting for output match"},...}` |
| no pane | 1 | stderr `{"error":{"code":"pane_not_found",...}}` |

Other hypotheses checked: the echoed command line cannot match, because
`echo SMOKE_TEST_$((40+2))` does not contain `SMOKE_TEST_42`. Source and
timing played no part.

### Callers of `wait_output`

Only one: `crates/horch/src/cmd/smoke.rs` (`smoke messaging`). Spawn,
`horch tell` and messaging delivery do not use it (`delivery::send_line`
polls `pane read`). So only the smoke check was affected. All other herdr
calls in `workspace/herdr.rs` use the `pane`/`workspace`/`tab`/`agent`/
`integration` nouns, which 0.8.2 has.

### Fix

- `crates/horch-core/src/workspace/herdr.rs`: `wait_output` runs
  `herdr pane wait-output <pane> --match <needle> --source recent-unwrapped
  --timeout <ms>` (the source is the one the smoke failure dump prints).
  New `wait_outcome`: exit 0 is a match, herdr's `timeout` error code is
  `Ok(false)`, anything else (unknown command, missing pane) is an `Err`.
  A future CLI change now fails loudly instead of reading as a timeout.
- Fake herdr (`crates/horch-e2e/src/bin/fake-herdr.rs`):
  - A top-level command 0.8.2 does not have exits 2 with
    `unknown command: <noun>`, as real herdr does. Before, the fake
    answered every unknown call with success, which hid this bug.
  - `pane wait-output` matches pane text with 0.8.2's exit codes and JSON;
    it takes no state lock while it polls.
  - `pane read` prints the pane text (command output plus the typed line).
  - New scenario `shell`: `agent prompt` fails (no agent), `send-text`
    types, `send-keys <pane> enter` runs the typed line into the pane output.

### Tests

- `workspace::herdr::tests::wait_outcome_tells_a_timeout_from_an_error`
  (the 3 real 0.8.2 answers plus the unknown-command answer).
- `horch-e2e/tests/fakes.rs::fake_herdr_answers_wait_output_like_herdr_0_8_2`.
- `horch-e2e/tests/smoke.rs::smoke_messaging_sees_the_message_run_in_pane_b`:
  runs `horch smoke messaging` end to end against the fake. With the old
  `wait_output` it fails with the operator's exact message
  (`FAIL: SMOKE_TEST_42 not observed on pane w1:p2`); with the fix it passes.
- Real server: `./target/debug/horch smoke messaging` prints `PASS` (exit 0)
  and closes its workspace.

### Cleanup

Closed the leftover `horch smoke test` workspaces: `w2H`, `w2K`, `w2N`,
`w2P`, `w2Q` (each label was checked before the close). `w2F`, `w2G`,
`w2B`, `w18`, `w28` were not touched.

## 2. A crashing harness showed as `available`

### Cause

- `quota_probe::harness_version` returned `None` both for "no answer" and for
  "ran and failed", so agent-list printed `(no answer)`.
- `AgentRow.available` was `found && state != Exhausted`: a `broken` pool
  still counted as available.
- Routing judged only pools. `pi --version` failing marks the `local` pool
  broken, but pools are shared: a `pi` teammate on an `anthropic/` or
  `openai/` model draws on the `claude`/`codex` pool, so routing could
  still choose a `pi` that cannot start. Nothing checked claude, codex,
  opencode, prime or antigravity binaries at all.

### Fix (option 1, approved by the orchestrator, `quota.rs` granted)

- `quota_probe.rs`: `VersionProbe { Version, Broken(why), NoAnswer }` and
  `probe_version`. `harness_version` stays as a thin wrapper (dataset
  preflight unchanged). `why` is `<bin> --version exited <code>: <line>`,
  where `<line>` is the first line that says "error", else the first
  non-empty line, cut to 200 chars (a Node crash starts with a source
  location; the `Error [ERR_REQUIRE_ESM]: ...` line names the fault).
  `run_short` (the local health check: `pi --version`, `ollama list`) now
  also appends that line to its error.
- `ProbeBins.harnesses`: every harness binary. `probe_all` runs all
  `--version` probes in parallel (`probe_harnesses`) and writes
  `QuotaFile.harnesses: {name: {version, error, probed_at}}`. A failure sets
  `error`; a later success clears it; no answer (not installed, timeout)
  is not a failure and creates no entry.
- `quota.rs`: `QuotaFile.harnesses` is `#[serde(default)]` and skipped when
  empty, so old `quota.json` files read and files without broken data
  are unchanged. `QuotaView::assess(agent, model)` returns `broken` with
  the error as the reason when that harness has an `error`, whatever its
  pool. The gate (`decide`), the eligible set and agent-list all go through
  `assess`, so routing never chooses it after the next quota probe.
- `harness/inventory.rs`: `BinaryFacts.broken`, `AgentRow.broken` and
  `AgentRow.status` (`available` / `unavailable` / `broken`).
  `available = found && !broken && !pool_state.blocks()` (cooling now also
  reads unavailable, which matches what the gate does).
- `cmd/agentlist.rs`: uses `probe_version`; the table shows the `status`
  and `(broken: <why>)` in the version column. `gather` is `pub(crate)`.
- `cmd/doctor.rs`: `horch doctor` probes every harness and prints
  `warning: harness pi is broken: <why>. ...` (not fatal).

On the operator's Mac after the fix:

```
pi  broken  broken  /opt/homebrew/bin/pi  (broken: pi --version exited 1: Error [ERR_REQUIRE_ESM]: require() of ES Module ...)
```

### Tests

- `quota_probe::tests::first_error_line_skips_the_node_source_location`
- `quota_probe::tests::a_crashing_harness_is_broken_until_it_recovers`
  (fake binary exits 1 with stderr; then recovers and the error clears;
  a missing binary is not broken)
- `quota::tests::a_broken_harness_is_broken_whatever_its_pool` (old file
  without `harnesses` reads; `pi` on `anthropic/` is broken while `claude`
  stays ok; round-trip; recovery)
- `inventory::tests::agent_list_inventory_broken_binary_is_not_available`
- `horch/tests/agent_list.rs::agent_list_reports_a_crashing_harness_as_broken`
- `cmd::doctor::tests::a_broken_harness_is_a_doctor_warning`
- `horch-e2e/tests/e2e.rs::quota_probe_records_a_crashing_harness`

## Gotchas and follow-ups

- Routing sees a broken harness only after a quota probe ran (collector
  schedule or `horch quota --refresh`). agent-list and doctor probe live.
- When the cached view says a harness is broken but a live probe now
  succeeds, agent-list still shows `broken` until the next quota probe.
- The quota probe now runs up to 6 `--version` calls in parallel (time
  limit 15 s each, as before for claude/codex).
- `pi` itself is not fixed: it needs Node 22.12 or newer (the operator has
  22.9.0). The operator decides that upgrade.
- Out of scope, seen: `run_short` and `probe_version` read the pipes only
  after exit; a harness that prints more than the pipe buffer (64 KiB)
  before exit would hang to the timeout. No current harness does.
- ARC-22 (`arch_scan`): the first version of `first_error_line` used
  `.contains("error")` and the scan flagged it. The line choice now uses
  the first word of the line (an error kind such as `Error`, `TypeError:`,
  `error:`). That is more precise, and nothing branches on it. No
  `ALLOWED` entry was added.

## Checks

The one full gate, started before the gate-load rule change, passed every
target except `arch_scan` (ARC-22, fixed above). After the fix these
targeted checks are green: `cargo build --workspace --all-targets`; clippy
`-D warnings` on horch-core, horch, horch-e2e; rustfmt on the changed files;
tests horch-core lib (390), `arch_scan`, `routing`, `preflight`, horch bin
(62), `agent_list`, horch-e2e `fakes`, `smoke`, `e2e`.
