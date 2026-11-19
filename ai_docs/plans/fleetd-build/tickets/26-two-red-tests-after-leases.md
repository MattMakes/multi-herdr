# Make 2 tests green that the lease task left red on the run branch

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch-core --test w3_finish_steps --test trf_35_park_unlanded && cargo test -p horch-core --lib taskledger && cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-core/tests/w3_finish_steps.rs`, `crates/horch-core/tests/trf_35_park_unlanded.rs`

## Goal

These 2 tests pass on the run branch, and each one still proves its rule:

- `worktree_new_refuses_a_shared_task_and_returns_an_existing_worktree`
  (`crates/horch-core/tests/w3_finish_steps.rs`, about line 1431)
- `trf_35_done_unlanded_parks_the_task_and_requeue_resumes_it_on_the_same_worktree`
  (`crates/horch-core/tests/trf_35_park_unlanded.rs`, about line 164)

## Context

Task T-263 (leases, ticket 08 of Phase 2) made `tasks::claim` refuse a second attempt while the
first attempt of the task is live (FM-14). The Check of T-263 did not run these 2 test files. The
worker of T-288 found them red on the base. Its note: the tests requeue a task by SQL and do not
end the first attempt.

2 cases are possible for each test:

- Test fault: the test takes a shortcut (an UPDATE by SQL) that the product never takes. Then the
  test must use the real path: the product command that requeues or parks, which releases the
  lease and ends the old worker row (see T-263: handoff and requeue release the lease in their
  transaction).
- Product fault: the real path of a user (`horch ledger done --unlanded`, then
  `horch ledger requeue`, then a claim; or `horch ledger worktree new` for a shared task) is
  refused now. Then the product is wrong, and work that is parked cannot be resumed (RR-17). In
  this case do not change the product here: send QUESTION with the exact command sequence and the
  error, then end your turn.

## Steps

1. Run both tests on the run branch and read each failure.
2. For each test, do the same steps through the product functions that the CLI calls, in a
   temporary ledger. Decide: test fault or product fault. Write the proof in the commit message.
3. Test fault: replace the SQL shortcut with the product path. Keep each assert.
4. Run the whole workspace 1 time with `cargo test --workspace --locked --no-fail-fast` and give
   the counts in your done summary. Name each other test that is red and not on a list of expected
   failures.
5. Run the Check.

## Constraints

- Before the commit, run `git remote -v` and `git config user.email`. The remote must be
  `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send BLOCKED.
- No co-author line. Never write the em dash character. NEVER use `ANTHROPIC_API_KEY`.
- Do not use a shared `CARGO_TARGET_DIR`. Never install a development build. Each test uses
  temporary folders and a temporary git repository, never the live ledger and never the repository
  `/Users/MASCOTT/projects/mh/multi-herdr`.
- Do not make an assert weaker. Do not put a test on a list of expected failures.
- Run `horch ledger progress` each 5 min during a long run.
- If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes, and the done summary says "test fault" or "product fault" for each test.
