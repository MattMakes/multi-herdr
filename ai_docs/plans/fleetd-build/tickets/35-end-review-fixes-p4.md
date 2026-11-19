# Fix the Critical and Important findings of the end review of Phase 4

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo test --workspace --locked --no-fail-fast && cargo fmt --all --check`
**Own:** `crates/`, `docs/specs/fleetd/reviews/end-review-p4.md`

## Goal

Each Critical and each Important finding of `docs/specs/fleetd/reviews/end-review-p4.md` is
fixed with a test, or has a written reason why it is not a fault.

## Context

- The findings file comes from the end review task of Phase 4. Read it first.
- The spec is `docs/specs/fleetd/design.md`. Decisions are in
  `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-build/decisions.md`.
- Other tasks run beside this one. Merge the run branch often.

## Steps

1. For each Critical and Important finding, in the order of the file: write 1 test that shows the
   fault (red), fix the fault, and see the test green. Commit each fix alone, with the finding id
   in the message.
2. If a finding is not a fault, write the reason below the finding in the file. If you are not
   sure, send QUESTION.
3. If a fix needs a design change or a schema step, send QUESTION and continue with the next
   finding.
4. Add a line "Status: fixed in <commit>" or "Status: not a fault, <reason>" to each finding.
   List each Minor finding that you did not fix under "Left open".
5. Run the Check. Each red test is yours to fix.

## Constraints

- Simplified Technical English. No em dash.
- 1 door: each state change goes through `transition()`.
- Do not make an assert weaker, do not add a test to an expected-failures list, and do not add an
  exemption to a guard test.
- Each test uses temporary folders. Never use the live ledger. Never install a development build.
- NEVER use `ANTHROPIC_API_KEY`.
- Send `horch ledger progress` each 5 min in long test runs.

## Done when

- The Check passes, and each Critical and Important finding has a Status line.
- The done summary gives the count of fixed findings, of findings that are not a fault, and of
  findings left open.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
