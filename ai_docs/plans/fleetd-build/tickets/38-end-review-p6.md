# End review of Phase 6 of fleetd: 1 findings file for correctness and security

<!-- horch-ticket: 1 -->
**Persona:** primary `codex-reviewer`, fallback `opus-hardening` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/reviews/end-review-p6.md`
**Own:** `docs/specs/fleetd/reviews/end-review-p6.md`

## Goal

The file `docs/specs/fleetd/reviews/end-review-p6.md` lists each real fault of the Phase 6 code,
with its severity, so that 1 fix task can correct them.

## Context

- The tasks of Phase 6 landed with no code review for each task (operator decision of
  2026-11-18: build first, review at the end). This task is that review.
- The change to review is the diff of the run branch from the gate commit of Phase 5 to the
  gate commit of Phase 6. Find the commits with
  `git log --oneline -1 -- docs/specs/fleetd/gates/fleetd-p5.md` and
  `git log --oneline -1 -- docs/specs/fleetd/gates/fleetd-p6.md`.
- The spec is `docs/specs/fleetd/design.md`. The tickets are in `docs/tickets/fleetd-p6/` and
  their `MAP.md`.
- Read the watch list first:
  `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-build/for-operator/end-review-watchlist.md`.
  It names choices that landed with no review. Check each line that names a task of Phase 6.
- Read the decision record for deviations:
  `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-build/decisions.md`. A recorded
  decision is not a finding, unless it makes a security fault.

## Steps

1. List the changed product files of the diff by bounded context
   (`crates/horch-core/src/fleetd/<context>/`, `crates/horch-platform/`, `crates/horch/src/`).
2. Review each context for correctness against the design: state changes only through
   `transition()`, effects after the commit, crash safety, lease and wait rules, and each
   requirement that the tickets of Phase 6 name.
3. Review security: sandbox reach, path checks (links, owner, mode, size), what a job process or
   an agent can write, trust of payload facts, secrets in logs, and each SR requirement that the
   tickets of Phase 6 name.
4. Review the tests: an assert that proves nothing, a test in an expected-failures list, a test
   that depends on the speed of the host.
5. Write the findings file. For each finding: an id (P6-F1, P6-F2, ...), the severity (Critical,
   Important, Minor), the file and line, the fault in 1 or 2 sentences, a failure scenario, and
   the fix that you recommend. Put the Critical findings first. Write no finding that you did
   not verify in the code.
6. Commit only the findings file.

## Constraints

- Simplified Technical English. No em dash.
- Do not change product code or tests.
- Do not run the whole test suite. Run 1 test only when you must prove a finding.
- NEVER use `ANTHROPIC_API_KEY`.
- Send `horch ledger progress` each 10 min.

## Done when

- The Check passes, and the done summary gives the count of findings for each severity.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
