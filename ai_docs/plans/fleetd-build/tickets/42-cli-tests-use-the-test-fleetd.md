# Each CLI test of a routed command runs with a test fleetd that serves the channel, and no such test is red

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch --locked --no-fail-fast && cargo test -p horch-e2e --locked --no-fail-fast && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch/tests/`, `crates/horch-e2e/tests/`, `crates/horch-e2e/src/`

## Goal

Each test that runs a `horch` command that Phase 5 routes through a request (the channel or a
request file) starts a test fleetd that serves it. No test of `crates/horch` or
`crates/horch-e2e` is red for the cause "no fleetd serves the request".

## Context

- Tickets 15 to 28 of `docs/tickets/fleetd-p5/` (T-470, T-471, T-472, T-473, T-474, T-476, T-477)
  route the `horch` commands through requests. Before ticket 41 (T-490) wired the channel server
  into fleetd, a CLI test with no server found no socket, wrote a request file and found no rows.
  T-470 named about 31 red test files: `crates/horch/tests/ledger_cli.rs`,
  `trf_11_add_map_cli.rs`, `w3_ledger_cli_pins.rs` and about 23 files of `crates/horch-e2e/tests`.
  The orchestrator let those tasks land with these tests red, and this ticket corrects them.
- Read the done summaries of T-470, T-471, T-472, T-473, T-474, T-476, T-477 and T-490 first
  (`horch ledger show T-<n>`). They name the in-process ChannelServer of the new tests and the
  test fleetd of `crates/horch-e2e/src/fleetd.rs`.
- Other tasks run beside this one. Merge the run branch often.

## Steps

1. Run the Check with `--no-fail-fast` and list each red test with its cause. Put the count in a
   progress note.
2. Write or extend 1 shared helper that starts a test fleetd that serves the channel and the
   request files in temporary folders, and stops it at the end of the test. Tests that start a
   real fleetd must not conflict on the fleetd row: 1 fleetd for each temporary ledger.
3. Move each red test to the helper. Do not make an assert weaker. Run the tests of the file
   after each move.
4. A red test with another cause: fix it if the fix is small and name it in the done summary, or
   send QUESTION with the cause.
5. Run the Check. Each red test is yours to fix.

## Constraints

- Simplified Technical English. No em dash.
- Test code and test support only. If product code has a fault, send QUESTION.
- No test depends on the speed of the host. Wait for an event, not for a time.
- Each test uses temporary folders. Never use the live ledger. Never start a development fleetd
  outside a test that uses temporary folders. Never install a development build.
- Do not add a test to an expected-failures list.
- NEVER use `ANTHROPIC_API_KEY`.
- Send `horch ledger progress` each 5 min in long test runs.

## Done when

- The Check passes with no red test.
- The done summary names the helper, the count of moved tests, and each test fixed for another
  cause.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
