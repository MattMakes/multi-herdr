# The production conformance suite passes for each exec CLI: the reference profile, the adapters and the fakes agree with the design

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets --locked && cargo test -p horch-core --test p7_conformance --test p7_conformance_job && cargo test -p horch-e2e --test p7_docs_sync && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs`

## Goal

A conformance run through the binary passes for each exec CLI (claude, codex, agy) with the fake
CLIs, and each check of the suite is true for the launch that fleetd makes for a worker. The e2e
test `passed_conformance_marks_the_profiles_healthy_and_dispatch_resumes` of T-602 is green and
is not on a list of expected failures.

## Context

The worker of T-602 (opus-149) drove fleetd, the docs_sync job, the conformance job and the fake
CLIs through the binary. The docs_sync runs pass. The conformance run fails for each exec CLI:

- claude: "Private home honoured" fails ('the launch names no private config home':
  `reference_profile` has no skills, so the launch has no `--plugin-dir`). "Stop gate" fails (the
  fake writes a result).
- codex: "Exact MCP set" fails (listed `{}`). "Sandbox boundary" fails ('the outside write:
  allowed': `reference_profile` of codex has no `permission_mode`). "Hooks fire" fails. "Stop
  gate" fails.
- agy: "Private home honoured" fails (the adapter uses the operator HOME; the core test
  `private_home_fails_when_the_launch_uses_the_operator_home` expects this fail). "Exact MCP set",
  "Sandbox boundary" and "Stop gate" fail.

Risk that the worker named: with the real CLIs, the private home and sandbox faults fail each real
run too. Then each exec profile is unhealthy and gets no dispatch after the release.

Read first:

- `horch ledger show T-602` (the done summary, the test file name and the line of the expected
  failure).
- `docs/tickets/fleetd-p7/16-fakes.md`, `23-conformance-suite.md`, `25-conformance-job.md` and the
  done summaries of their tasks.
- `docs/specs/fleetd/design.md`: the conformance checks, the private home, the sandbox boundary,
  the MCP set, the hooks and the stop gate of a worker launch.

## Steps

1. Make the fault visible. Run the red test of T-602 and write the failed checks and their
   evidence lines in a progress note. Check: the list agrees with the Context, or you name each
   difference.
2. For each failed check, find which side is wrong: the reference profile of the suite, the
   launch that the adapter makes, or the fake CLI. The rule: the suite must test the launch that
   fleetd makes for a real worker of that CLI. Do not make the reference profile different from a
   real worker profile only to pass. Write the cause of each check in a progress note.
3. Correct each cause in its own commit:
   - a reference profile that does not have what a real profile has (skills, permission mode,
     MCP set): correct `reference_profile`.
   - an adapter that does not give the launch a private home or a sandbox: correct the adapter
     and add 1 unit test.
   - a fake CLI that does not behave as the real CLI does for the check (stop gate, hooks, MCP
     list): correct the fake in `crates/horch-e2e/src`.
   Check: the core tests of conformance pass after each commit.
4. agy: the design decides if agy can have a private home. If the real CLI cannot, the check
   must fail and the profile must stay unhealthy by design; then do not change it, and say so in
   the done summary with the place in the design. If the design and the code do not agree, send
   QUESTION and end your turn.
5. Remove the line of the T-602 test from the list of expected failures. Check: the test is green
   3 times in a row.
6. Run the Check.

## Constraints

- Simplified Technical English in comments and messages. No em dash.
- Never read, set, pass or depend on `ANTHROPIC_API_KEY`.
- Do not make a conformance check weaker and do not remove a check. A check that is wrong by the
  design needs a QUESTION first.
- No new schema step.
- Other tasks change files near yours. Merge the run branch each 30 minutes and before finish.
- Each test command has a time limit. Run only your own test files. Send
  `horch ledger progress` each 5 min in long test runs.

## Done when

- The T-602 test is green and on no list of expected failures, and the Check passes.
- The done summary names: the cause of each failed check, the side that you corrected, the agy
  decision, and each file that you changed.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
