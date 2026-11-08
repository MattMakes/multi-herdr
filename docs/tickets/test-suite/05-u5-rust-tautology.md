# 05: U5: Rust tautology rewrites and duplicate removals

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-hardening` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch-core && scripts/check-req-coverage.sh`
**Own:** `crates/horch-core/tests/telemetry_collect.rs`, `crates/horch-core/src/taskledger/finish/land.rs`, `crates/horch-core/src/routing/decision.rs`, `crates/horch-core/tests/context_roster.rs`, `crates/horch-core/tests/intake_roster.rs`, `crates/horch-core/tests/harness_defaults.rs`, `docs/specs/telemetry.md`, `docs/specs/context-policy.md`

## Goal

Apply unit U5 of the audit: rewrite T2 and T3 with the oracle and mutation of the audit table; remove the 4 D1 tests and their names in the spec tables. Also re-read every other test in these 6 test files against the Definitions, and rewrite or remove any further tautological test you find, with a mutation for each rewrite.

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

