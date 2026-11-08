# 40: Fix the 7 Rust test failures from the audio-reviewer teammate commit

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-hardening` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test --workspace --no-fail-fast`
**Own:** `teammates/audio-reviewer.md`, `skills/audio-review/`, `crates/horch-core/tests/`, `crates/horch/tests/`, `crates/horch-core/tests/fixtures/`

## Goal

`cargo test --workspace --no-fail-fast` passes on `fleet/solo-w68-hark`, except for a test that the T-38 audit names as flaky on this host.

## Evidence

- T-33 (integration, commits ed3b976b and 80a2a07e) ran `cargo test` on this branch: 2796 passed, 8 failed.
- 7 failures come from base commit 6f65bac4 (T-24, the audio-reviewer teammate): missing oracle files, and roster counts 78/77 and 83/82 (tests expect the old teammate count).
- 1 failure is a herdr delivery timeout on this host. Do not fix it here: the test-suite audit (T-38) owns flaky tests. Name it in DONE.
- The T-24 ticket check ran only `skills_catalog` and `golden_prompts`, so it missed these.

## Steps

1. Run `cargo test --workspace --no-fail-fast` and list the failures.
2. For each roster-count test, update the expected count to include audio-reviewer and sound-engineer, and check that the test asserts the list of names, not only a count. If it asserts only a count, add the 2 names to its assertion.
3. For each missing oracle file, generate it the way the repo makes the others (read the test and its fixture folder), and review the content by eye before you commit it.
4. Run `cargo test --workspace --no-fail-fast` and `cargo clippy --workspace --all-targets -- -D warnings`.
5. Commit exact files. Run `horch ledger finish`, then `horch ledger done`.

## Constraints

- Do not edit `skills/hark/`. Another task owns it.
- Do not change a test to skip.

## Done when

- The check passes, or the only failure is the flaky herdr timeout named in DONE.
