# 02: U1: e2e harness skips the 5 s delivery probe, and stale couriers are stopped

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-hardening` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch-e2e --no-fail-fast`
**Own:** `crates/horch-e2e/src/harness.rs`, `crates/horch-e2e/tests/e2e.rs`

## Goal

Apply unit U1 of the audit. Also apply the audit's Q3 option 1 (decided by the orchestrator): the e2e harness writes each courier pid to a file, and `Harness::new` stops stale couriers from earlier runs. Report each binary's time before and after. Also make these 2 tests reliable on this host, at their root cause (they block every workspace check now): `doctor_extras_watchdog::doctor_is_quiet_when_no_offered_teammate_uses_such_a_harness` fails 3 of 3 with a herdr delivery timeout, and `fleet18_observability::g52_concurrent_workers_get_their_own_session_and_transcript` fails on 'database is locked' (audit F4; #344's Store::open dir lock may already fix it after ticket 01: run it 20 times to confirm).

## Context

- Operator request (2026-11-08): make the test suite faster and more reliable; remove tautological tests, or rewrite them so they are anti-tautological. All of it merges into PR #344.
- The audit is `docs/specs/test-suite-audit.md` (on this run branch). Read its Definitions and your unit section first. Baseline: cold `cargo test --workspace` 406 s, warm 285-327 s.
- Anti-tautological means: the expected value comes from an independent source (hand-computed value, spec table, reviewed fixture, property), and a plausible 1-line mutation of the code under test makes the test fail. Show each mutation failing (then revert it) and name it in your commit message.
- Never use a fixed `/tmp` path. Never raise a timeout to hide slowness: macOS scans each new executable for 14-35 s at first exec; tests warm fakes instead.
- Before finish, run `cargo test --workspace --no-fail-fast` and `cargo clippy --workspace --all-targets -- -D warnings`. Run gates miss the horch crate tests.

## Steps

1. Read the audit unit and the code it names. Measure the before state (time, failure rate or the tautology).
2. Make the change. For each rewritten test, show the mutation failing, then revert it.
3. Run the check, then the workspace test and clippy runs named in Context.
4. Commit exact files. Run `horch ledger finish`, then `horch ledger done` with before/after numbers and each mutation.

## Done when

- The check passes. The DONE report gives the before and after numbers.

