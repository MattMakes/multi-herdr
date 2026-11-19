# fleetd applies the triage decisions ARCHIVE, DELETE, LAND and PR through 1 job kind

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo test -p horch-core --test triage_apply_job && cargo test -p horch-core --test fleetd_single_writer && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-core/src/taskledger/triage/apply.rs`, `crates/horch-core/tests/triage_apply_job.rs`

## Goal

`horch worktree triage` with a reply ARCHIVE, DELETE, LAND or PR works again: the CLI sends 1
`worktree_triage` request, fleetd queues 1 job, the job does the git and gh work in its sandbox,
and fleetd writes the rows from the job result file. No process other than fleetd writes the
ledger.

## Context

- Before Phase 5 the CLI did the git and gh work of these 4 decisions and wrote the ledger
  (`triage_apply_direct` in `crates/horch/src/cmd/worktree.rs`, and
  `crates/horch-core/src/taskledger/triage/apply.rs`). T-469 made the handler for KEEP and
  FINISH only. T-885 made the CLI refuse the 4 decisions with a text "fleetd does not apply them
  in this version". This ticket gives them back.
- Read the done summaries of T-469, T-883 and T-885 first (`horch ledger show T-<n>`). T-883
  moved the job process writes to the result file `run/jobs/<id>-<run>.json` and the ingest.
  Follow that pattern exactly.
- The design is `docs/specs/fleetd/design.md`: the worktree custody rules (only fleetd removes a
  worktree; RR-13 refusals for a dirty tree, a stash, unpublished unique commits, a moved HEAD),
  the rescue ref before each removal, and SR-3 (no push or PR outside the rules of fleetd).

## Steps

1. Write the tests first in `crates/horch-core/tests/triage_apply_job.rs`: 1 test for each of the
   4 decisions through the request, the job and the ingest, and 1 test for each refusal of RR-13.
   Use a fake `gh` for PR. Check: red first.
2. Add the job kind. If the job kind list is a CHECK of the schema, add the kind inside the
   schema step of Phase 5 in `crates/horch-core/src/taskledger/schema.rs`; do not add a new
   schema version (the branch is not released). Run the schema tests of Phases 5 to 7.
3. The handler of `worktree_triage` queues the job for these 4 decisions. The job runs in the
   sandbox with the git runner of fleetd. It writes a rescue ref before each removal. It puts its
   facts in the result file, and the ingest writes the rows.
4. Remove the refusal text of T-885 in the CLI, and pin the new flow with 1 CLI test.
5. Run the Check.

## Constraints

- Simplified Technical English. No em dash.
- 1 door: each state change goes through `transition()`. Effects run after the commit.
- The ingest treats the result file as untrusted input.
- Do not add an exception to an arch rule.
- Use a timeout on each test command. Each test uses temporary folders. Never use the live
  ledger. No test calls the real `gh` or a real remote.
- NEVER use `ANTHROPIC_API_KEY`.

## Done when

- The Check passes, and the done summary names the job kind, the schema change and each file
  that you changed.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
