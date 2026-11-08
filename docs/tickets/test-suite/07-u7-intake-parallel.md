# 07: U7: skill-intake suite runs in parallel, and the gate runs the hark and one-page-design suites

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-hardening` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `bash scripts/phase-gate.sh`
**Own:** `scripts/phase-gate.sh`, `scripts/skill-intake/tests/test_roles.py`, `skills/skill-intake/scripts/intakelib/task.py`

## Goal

Apply unit U7 of the audit. Also apply the audit's Q2 (decided by the orchestrator): `scripts/phase-gate.sh` runs the hark pytest suite (`-m 'not ml and not live'`, with the hark venv when it exists, else a clear skip line) when `skills/hark/` changed against the base, and the one-page-design suite when `skills/one-page-design/` changed. Report gate time before and after.

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

